"""永続要求キーを変換設定へ結び付け、同じ要求の再起動を拒否する。"""
import hashlib
import json
import os
from pathlib import Path
import re

import jade_pdf2docx_job_history as history
from jade_pdf2docx_history_capacity import check_capacity, directory_gate


def fingerprint(policy, manager, command, *, cwd=None):
    """入力SHA・出力・全引数・pool設定・作業directoryを同じidentityへ含める。"""
    cwd = os.getcwd() if cwd is None else cwd
    if (not isinstance(cwd, str) or "\0" in cwd or not Path(cwd).is_absolute()
            or not isinstance(command, list) or manager not in ("--system", "--user") or len(command) < 3
            or any(not isinstance(value, str) or "\0" in value for value in command)
            or any(not Path(value).is_absolute() for value in command[:3])):
        raise ValueError("keyed request requires absolute executable, input and output")
    if "--publication-receipt" in command[3:]:
        raise ValueError("publication receipt is assigned by keyed admission")
    positions = [index for index, value in enumerate(command[3:], 3) if value == "--input-sha256"]
    if (len(positions) != 1 or positions[0] + 1 >= len(command)
            or not re.fullmatch(r"[a-f0-9]{64}", command[positions[0] + 1])):
        raise ValueError("keyed request requires one lowercase input SHA-256")
    data = json.dumps(dict(policy=policy, manager=manager, command=command, cwd=cwd),
                      sort_keys=True, separators=(",", ":")).encode()
    if len(data) > 65536:
        raise ValueError("keyed request arguments too large")
    return hashlib.sha256(data).hexdigest()


def existing(fd, key, digest, *, queued=None):
    """保存済み要求だけを再利用し、中断や旧tokenとの衝突は安全側に拒否する。"""
    if queued is not None and history.read_record(fd, key + ".queue.json") != queued:
        raise ValueError("queued request changed before admission")
    try:
        record = history.request_record(fd, key)
    except FileNotFoundError:
        try:
            os.stat(".pending-" + key + ".request.json", dir_fd=fd, follow_symlinks=False)
        except FileNotFoundError:
            for prefix in ("", ".pending-"):
                try:
                    os.stat(prefix + key + ".queue.json", dir_fd=fd, follow_symlinks=False)
                except FileNotFoundError:
                    continue
                if queued is None or prefix:
                    raise ValueError("queued request requires explicit queue dispatch")
            return None
        raise ValueError("request key has incomplete durable claim")
    if record["schema"] != 2 or record["request_sha256"] != digest:
        raise ValueError("request key conflict")
    return record


def lookup(directory, key, digest, *, queued=None):
    """空きserviceや保持枠の検査より先に、元の要求を見つける。"""
    with history.store_directory(directory) as fd:
        with directory_gate(fd, history.HISTORY_LOCK_SECONDS):
            return existing(fd, key, digest, queued=queued)


def claim(directory, key, digest, manager, service, *, queued=None):
    """異なるpool受付も共有する排他下で、起動前の要求を一度だけ同期する。"""
    with history.store_directory(directory) as fd:
        with directory_gate(fd, history.HISTORY_LOCK_SECONDS):
            record = existing(fd, key, digest, queued=queued)
            if record is not None:
                return False
            check_capacity(fd, key, history.MAX_HISTORY_JOBS, reserved=queued is not None)
            history.write_record(fd, key + ".request.json", dict(
                schema=2, token=key, request_sha256=digest, manager=manager, service=service))
            return True


def replay(directory, key):
    """元tokenの状態をJSONで返し、未確定状態は再起動せず一時拒否する。"""
    result = history.query(directory, key, inspect=history.inspect_service)
    print(json.dumps(dict(result, replayed=True), sort_keys=True), flush=True)
    return {"succeeded": 0, "failed": 1}.get(result["state"], 75)
