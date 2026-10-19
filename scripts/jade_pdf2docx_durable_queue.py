#!/usr/bin/env python3
"""要求を先に永続化し、明示dispatcherだけが待機中の一件を起動する。"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

import run_jade_pdf2docx_pool as pool


def validate_queue(record, token):
    """小さな固定schemaと要求digestを検証し、変換設定の置換を拒否する。"""
    pool.history.validate_token(token)
    if (set(record) != {"schema", "token", "request_sha256", "policy", "manager", "command", "cwd"}
            or type(record["schema"]) is not int or record["schema"] != 1
            or record["token"] != token or not isinstance(record["cwd"], str)):
        raise ValueError("invalid queued request fields")
    pool.validate_policy(record["policy"])
    digest = pool.identity.fingerprint(record["policy"], record["manager"], record["command"], cwd=record["cwd"])
    if record["request_sha256"] != digest:
        raise ValueError("queued request identity mismatch")
    return record


def read_queue(fd, token):
    """nofollowと所有者/単一link検証で公開済みの完全なキューだけを読む。"""
    pool.history.validate_token(token)
    record = pool.history.read_record(fd, token + ".queue.json")
    validate_queue(record, token)
    try:
        os.stat(".pending-" + token + ".queue.json", dir_fd=fd, follow_symlinks=False)
    except FileNotFoundError:
        return record
    raise ValueError("queued request publication incomplete")


def status(directory, token):
    """起動要求記録のない公開済みキューだけをqueuedとし、それ以降は元履歴を返す。"""
    with pool.history.store_directory(directory) as fd:
        with pool.history.directory_gate(fd, pool.history.HISTORY_LOCK_SECONDS):
            record = read_queue(fd, token)
            launched = pool.identity.existing(fd, token, record["request_sha256"], queued=record)
    if launched is not None:
        return pool.history.query(directory, token, inspect=pool.history.inspect_service)
    return dict(token=token, manager=record["manager"], request_sha256=record["request_sha256"],
                state="queued", output="unknown")


def enqueue(directory, token, policy, manager, command):
    """変換を起動せず、一件のコマンド/設定/作業directoryを同期して保持する。"""
    pool.history.validate_token(token)
    record = dict(schema=1, token=token, request_sha256=pool.identity.fingerprint(policy, manager, command),
                  policy=policy, manager=manager, command=command, cwd=os.getcwd())
    validate_queue(record, token)
    with pool.history.store_directory(directory) as fd:
        with pool.history.directory_gate(fd, pool.history.HISTORY_LOCK_SECONDS):
            try:
                previous = read_queue(fd, token)
            except FileNotFoundError:
                if pool.identity.existing(fd, token, record["request_sha256"]) is not None:
                    raise ValueError("request key already admitted outside durable queue")
                pool.identity.check_capacity(fd, token, pool.history.MAX_HISTORY_JOBS)
                pool.history.write_record(fd, token + ".queue.json", record)
                replayed = False
            else:
                if previous != record:
                    raise ValueError("queued request conflict")
                replayed = True
    return dict(status(directory, token), replayed=replayed)


def dispatch(directory, token, runtime_directory=None):
    """未起動だけを起動し、同期済み起動要求があれば元結果を返して再起動しない。"""
    directory = Path(directory)
    with pool.history.store_directory(directory) as fd:
        with pool.history.directory_gate(fd, pool.history.HISTORY_LOCK_SECONDS):
            record = read_queue(fd, token)
            if record["manager"] == "--system" and os.geteuid() != 0:
                raise ValueError("--system requires administrator (sudo)")
            launched = pool.identity.existing(fd, token, record["request_sha256"], queued=record)
    if launched is not None:
        return pool.identity.replay(directory, token)
    if runtime_directory is None:
        runtime_directory = Path("/run/jade-pdf2docx-pool") if record["manager"] == "--system" else Path(
            f"/run/user/{os.geteuid()}/jade-pdf2docx-pool")
    previous = os.open(".", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        working = os.open(record["cwd"], os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            os.fchdir(working)
            return pool.run_job(record["policy"], record["manager"], Path(runtime_directory), record["command"],
                                directory, token, queued=record)
        finally:
            os.close(working)
    finally:
        os.fchdir(previous)
        os.close(previous)


def main():
    """enqueue/status/dispatchを分離し、起動はLinux上の明示dispatchに限定する。"""
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_subparsers(dest="mode", required=True)
    enqueue_parser = modes.add_parser("enqueue")
    enqueue_parser.add_argument("--config", type=Path, required=True)
    enqueue_parser.add_argument("--system", action="store_true")
    for mode in (enqueue_parser, modes.add_parser("status"), modes.add_parser("dispatch")):
        mode.add_argument("directory", type=Path)
        mode.add_argument("token")
    enqueue_parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    try:
        if args.mode == "enqueue":
            if args.system and os.geteuid() != 0:
                raise ValueError("--system requires administrator (sudo)")
            result = enqueue(args.directory, args.token, pool.load_policy(args.config),
                             "--system" if args.system else "--user", args.command)
        elif args.mode == "status":
            result = status(args.directory, args.token)
        else:
            if sys.platform != "linux":
                raise ValueError("queue dispatch requires Linux systemd")
            return dispatch(args.directory, args.token)
        print(json.dumps(result, sort_keys=True), flush=True)
        return 0
    except (pool.PoolBusy, pool.history.HistoryBusy) as error:
        print("queue busy: " + str(error), file=sys.stderr)
        return 75
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print("durable queue failed: " + str(error), file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print("queue dispatch interrupted; inspect original token before retrying", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
