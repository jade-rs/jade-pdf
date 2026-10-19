#!/usr/bin/env python3
"""systemd service名を枠として、複数呼出元に共通の同時実行数を守る。"""
import argparse
from contextlib import ExitStack, contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import time
import uuid

import jade_pdf2docx_job_history as history
import jade_pdf2docx_request_identity as identity
from jade_pdf2docx_slot_retirement import retire_slot
from jade_pdf2docx_job_completion import watch_slot


STARTUP_SECONDS = 5
WRAPPER = Path(__file__).resolve().with_name("run_jade_pdf2docx_systemd.sh")


class PoolBusy(Exception):
    """短い起動受付の期限を超えた場合も、変換を起動せず拒否する。"""


def load_policy(path):
    """管理者の小さな通常JSON fileだけを読み、曖昧な容量を拒否する。"""
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid not in (0, os.geteuid())
                or info.st_mode & 0o022 or info.st_size > 4096):
            raise ValueError("unsafe pool policy file")
        data = source.read(4097)
    if len(data) > 4096:
        raise ValueError("pool policy too large")

    def unique_keys(pairs):
        """同じkeyの後勝ち解釈を禁止する。"""
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate pool policy key")
            result[key] = value
        return result

    try:
        policy = json.loads(data, object_pairs_hook=unique_keys)
    except (UnicodeError, RecursionError) as error:
        raise ValueError("invalid pool policy JSON") from error
    return validate_policy(policy)


def validate_policy(policy):
    """file読込と永続キューで同じpool設定の型・範囲を検証する。"""
    if not isinstance(policy, dict) or set(policy) != {
            "schema", "slice", "total_memory_mib", "job_memory_mib", "slots"}:
        raise ValueError("invalid pool policy fields")
    for key, maximum in (("schema", 1), ("slots", 64),
                         ("total_memory_mib", 8796093022207),
                         ("job_memory_mib", 8796093022207)):
        if type(policy[key]) is not int or not 1 <= policy[key] <= maximum:
            raise ValueError("invalid pool policy " + key)
    # 固定prefixがslice名とservice名に入ってもunit名の128文字上限に収める。
    name = policy["slice"]
    if not isinstance(name, str) or not re.fullmatch(r"jade-pdf2docx[a-zA-Z0-9]{0,89}\.slice", name):
        raise ValueError("invalid pool slice name")
    return policy


@contextmanager
def startup_gate(directory, policy):
    """起動選択だけを排他し、同じpoolの設定fingerprintを再起動まで固定する。"""
    directory.mkdir(mode=0o700, exist_ok=True)
    directory_fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        info = os.fstat(directory_fd)
        if info.st_uid != os.geteuid() or info.st_mode & 0o077:
            raise ValueError("unsafe pool runtime directory")
        fd = os.open(policy["slice"] + ".lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC,
                     0o600, dir_fd=directory_fd)
    finally:
        os.close(directory_fd)
    try:
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid()
                or info.st_mode & 0o077 or info.st_nlink != 1):
            raise ValueError("unsafe pool startup gate")
        deadline = time.monotonic() + STARTUP_SECONDS
        while True:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise PoolBusy("pool busy: startup admission timed out")
                time.sleep(0.02)
        fingerprint = (hashlib.sha256(json.dumps(policy, sort_keys=True).encode()).hexdigest() + "\n").encode()
        previous = os.read(fd, 66)
        if previous and previous != fingerprint:
            raise ValueError("pool policy changed or gate corrupt; stop all jobs before administrator reset")
        if not previous:
            # 先に永続化する。途中で死んだ不完全fileは、次の受付で安全側に拒否する。
            if os.write(fd, fingerprint) != len(fingerprint):
                raise OSError("incomplete pool fingerprint write")
            os.fsync(fd)
        yield
    finally:
        os.close(fd)


def unit_properties(manager, unit):
    """managerから実serviceの登録状態を読む。通信失敗を空き枠と解釈しない。"""
    name = unit if unit.endswith(".slice") else unit + ".service"
    result = subprocess.run(["systemctl", manager, "show", name,
                             "-p", "LoadState", "-p", "Description", "-p", "ControlGroup"],
                            capture_output=True, text=True, timeout=2)
    properties = dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)
    if "LoadState" not in properties or (result.returncode and properties["LoadState"] != "not-found"):
        raise ValueError("cannot inspect pool service: " + result.stderr.strip())
    return properties


def run_job(policy, manager, directory, command, history_directory=None, request_key=None, *, queued=None):
    """空きservice名へ一度だけ起動し、元の変換の終了結果を返す。"""
    digest = None
    if request_key is not None:
        history.validate_token(request_key)
        if history_directory is None:
            raise ValueError("request key requires history directory")
        digest = identity.fingerprint(policy, manager, command)
        if identity.lookup(history_directory, request_key, digest, queued=queued) is not None:
            return identity.replay(history_directory, request_key)
    with ExitStack() as resources:
        with startup_gate(directory, policy):
            if request_key is not None and identity.lookup(history_directory, request_key, digest, queued=queued) is not None:
                return identity.replay(history_directory, request_key)
            for slot in range(1, policy["slots"] + 1):
                unit = f"jade-pdf2docx-{policy['slice'][:-6]}-slot-{slot}"
                # 停止中・failed/inactiveを含め、managerのGC完了までは使用中と数える。
                if unit_properties(manager, unit)["LoadState"] == "not-found":
                    break
            else:
                print("pool busy: all service slots occupied", file=sys.stderr)
                return 75
            # managerのGCだけでは空群の削除を保証しない。旧FDの再利用を起動前に防ぐ。
            retire_slot(manager, policy["slice"], unit, unit_properties)
            watcher = resources.enter_context(watch_slot(manager, policy["slice"], unit, unit_properties))
            token = request_key if request_key is not None else uuid.uuid4().hex
            if request_key is not None:
                if not identity.claim(history_directory, token, digest, manager, unit + ".service", queued=queued):
                    return identity.replay(history_directory, token)
                print(json.dumps(dict(token=token, manager=manager, service=unit + ".service", replayed=False)),
                      flush=True)
            elif history_directory is not None:
                history.prepare(history_directory, token, manager, unit + ".service")
            arguments = ["/bin/sh", str(WRAPPER)]
            if manager == "--system":
                arguments.append(manager)
            arguments += ["--pool", policy["slice"], str(policy["total_memory_mib"]), "--job-token", token]
            if history_directory is not None:
                arguments += ["--history-dir", str(history_directory)]
            conversion = list(command)
            if request_key is not None:
                conversion += ["--publication-receipt", token + ":" + digest]
            arguments += [str(policy["job_memory_mib"]), unit, *conversion]
            print("pool job service: " + unit + ".service; token: " + token, file=sys.stderr, flush=True)
            child = subprocess.Popen(arguments, close_fds=True)
            deadline = time.monotonic() + STARTUP_SECONDS
            while child.poll() is None:
                try:
                    if unit_properties(manager, unit).get("Description") == "jade-pdf2docx-job-" + token:
                        break
                except (OSError, ValueError, subprocess.SubprocessError) as error:
                    print("pool registration observation failed: " + str(error), file=sys.stderr)
                    break
                if time.monotonic() >= deadline:
                    print("pool registration observation timed out; awaiting original job", file=sys.stderr)
                    break
                time.sleep(0.02)
            observation_error = None
            try:
                watcher.capture(token, done=lambda: child.poll() is not None)
            except (OSError, ValueError, subprocess.SubprocessError) as error:
                # 起動済みの子は必ず一度だけ待つ。観測不足で成功を返さない。
                observation_error = error
            # 起動観測の失敗/呼出側死亡でも、固定service名の重複起動をmanagerが拒否する。
            # 受付gateは子へ渡さず、変換の終了待ちより前に解放する。
        result = child.wait()
        if observation_error is not None:
            raise observation_error
        with startup_gate(directory, policy):
            watcher.retire(token)
        return 128 - result if result < 0 else result


def main():
    """既定は同じuser manager、--systemは管理者からの共通pool受付。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--system", action="store_true")
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--history-dir", type=Path)
    parser.add_argument("--request-key")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if len(args.command) < 3 or not Path(args.command[0]).is_absolute():
        parser.error("expected absolute jade-pdf2docx executable, input and output")
    if sys.platform != "linux":
        parser.error("pool admission requires Linux systemd")
    if args.system and os.geteuid() != 0:
        parser.error("--system requires administrator (sudo)")
    manager = "--system" if args.system else "--user"
    directory = Path("/run/jade-pdf2docx-pool") if args.system else Path(f"/run/user/{os.geteuid()}/jade-pdf2docx-pool")
    try:
        return run_job(load_policy(args.config), manager, directory, args.command, args.history_dir, args.request_key)
    except (PoolBusy, history.HistoryBusy) as error:
        print(error, file=sys.stderr)
        return 75
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print("pool admission failed: " + str(error), file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print("pool waiter interrupted; inspect the reported service before retrying", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
