#!/usr/bin/env python3
"""systemd終了フックがtoken別の結果を永続化し、欠損は不明として照会する。"""
import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
from jade_pdf2docx_history_capacity import HistoryBusy, capacity_gate, directory_gate


MAX_HISTORY_JOBS = 4096
HISTORY_LOCK_SECONDS = 1


def validate_token(token):
    """tokenを固定長の小文字hexに限定して任意pathを拒否する。"""
    if not isinstance(token, str) or not re.fullmatch(r"[a-f0-9]{32}", token):
        raise ValueError("invalid job token")


@contextmanager
def store_directory(directory):
    """絶対pathの各階層をnofollowで開き、所有者専用の保存先FDを保持する。"""
    directory = Path(directory)
    if not directory.is_absolute() or ".." in directory.parts:
        raise ValueError("history directory must be absolute without parent traversal")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    fd = os.open(directory.anchor, flags)
    try:
        for part in directory.parts[1:]:
            child = os.open(part, flags, dir_fd=fd)
            os.close(fd)
            fd = child
        info = os.fstat(fd)
        if info.st_uid != os.geteuid() or info.st_mode & 0o077:
            raise ValueError("unsafe history directory owner or permissions")
        yield fd
    finally:
        os.close(fd)


def read_record(fd, name, *, expected_links=1):
    """小さな所有者専用の通常JSONだけを読み、破損や重複keyを拒否する。"""
    source = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=fd)
    with os.fdopen(source, "rb") as stream:
        info = os.fstat(stream.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid()
                or info.st_mode & 0o077 or info.st_nlink != expected_links or info.st_size > 4096):
            raise ValueError("unsafe job history record")
        data = stream.read(4097)
    def unique(pairs):
        """重複keyでidentityや結果の解釈を変えない。"""
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate job history key")
            result[key] = value
        return result
    try:
        record = json.loads(data, object_pairs_hook=unique)
    except (UnicodeError, RecursionError) as error:
        raise ValueError("invalid job history JSON") from error
    if len(data) > 4096 or not isinstance(record, dict):
        raise ValueError("invalid job history record")
    return record


def write_record(fd, name, record):
    """同期済み一時fileを排他的に公開し、途中書込や既存結果の上書きを防ぐ。"""
    data = (json.dumps(record, sort_keys=True) + "\n").encode()
    if len(data) > 4096:
        raise ValueError("job history record too large")
    # 中断後も同じ一時名を使い、再試行で容量が増えることを防ぐ。
    temporary = ".pending-" + name
    target = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600, dir_fd=fd)
    try:
        with os.fdopen(target, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        # linkは既存tokenを置換せず、同期済みinodeだけを公開する。
        os.link(temporary, name, src_dir_fd=fd, dst_dir_fd=fd, follow_symlinks=False)
        os.unlink(temporary, dir_fd=fd)
        os.fsync(fd)
    finally:
        try:
            os.unlink(temporary, dir_fd=fd)
        except FileNotFoundError:
            pass


def request_record(fd, token):
    """要求recordのschemaとmanager/service/tokenを厳密に照合する。"""
    record = read_record(fd, token + ".request.json")
    return validate_request(record, token)


def validate_request(record, token):
    """通常readerと中断復旧で同じ要求schemaを検証する。"""
    fields = {"schema", "token", "manager", "service"}
    schema = record.get("schema")
    if schema == 2:
        fields.add("request_sha256")
    if (set(record) != fields
            or type(schema) is not int or schema not in (1, 2)
            or (schema == 2 and (not isinstance(record["request_sha256"], str)
                                or not re.fullmatch(r"[a-f0-9]{64}", record["request_sha256"])))
            or record["token"] != token or record["manager"] not in ("--system", "--user")
            or not isinstance(record["service"], str)
            or not re.fullmatch(r"jade-pdf2docx-[a-zA-Z0-9-]+\.service", record["service"])):
        raise ValueError("invalid job request identity")
    return record


def validate_result(record, request):
    """終了recordのidentity・型・終了状態と公開診断の一致を検証する。"""
    fields = {"state", "output", "service_result", "exit_code", "exit_status"}
    if (set(record) != set(request) | fields
            or any(type(record.get(key)) is not type(value) or record.get(key) != value
                   for key, value in request.items())
            or any(not isinstance(record[key], str) or len(record[key]) > 64 for key in fields)
            or record["state"] != outcome(record["service_result"], record["exit_code"], record["exit_status"])
            or record["output"] != ("reported_published" if record["state"] == "succeeded" else "unknown")):
        raise ValueError("invalid job completion record")
    return record


def prepare(directory, token, manager, service):
    """service起動より先に要求を同期保存し、同じtokenを二度発行しない。"""
    validate_token(token)
    if manager not in ("--system", "--user") or not re.fullmatch(r"jade-pdf2docx-[a-zA-Z0-9-]+\.service", service):
        raise ValueError("invalid job request identity")
    with store_directory(directory) as fd:
        with capacity_gate(fd, token, MAX_HISTORY_JOBS, HISTORY_LOCK_SECONDS):
            write_record(fd, token + ".request.json",
                         {"schema": 1, "token": token, "manager": manager, "service": service})


def outcome(result, code, status):
    """managerのsuccessだけでは成功とせず、通常CLIのexit 0を必須とする。"""
    if (result, code, status) == ("success", "exited", "0"):
        return "succeeded"
    if result in {"exit-code", "signal", "core-dump", "oom-kill", "timeout", "watchdog",
                  "resources", "protocol", "start-limit-hit", "exec-condition"}:
        return "failed"
    if result == "success" and code in ("killed", "dumped"):
        return "failed"
    return "unknown"


def finish(directory, token, service, environment):
    """受付と独立したExecStopPostから、元要求と一致する終了結果を一度だけ記録する。"""
    validate_token(token)
    with store_directory(directory) as fd:
        request = request_record(fd, token)
        if request["service"] != service:
            raise ValueError("job completion identity mismatch")
        result, code, status = [environment.get(key, "") for key in ("SERVICE_RESULT", "EXIT_CODE", "EXIT_STATUS")]
        if any(not isinstance(value, str) or len(value) > 64 for value in (result, code, status)):
            raise ValueError("invalid job completion result")
        state = outcome(result, code, status)
        with directory_gate(fd, HISTORY_LOCK_SECONDS):
            write_record(fd, token + ".result.json", dict(
                request, state=state, output="reported_published" if state == "succeeded" else "unknown",
                service_result=result, exit_code=code, exit_status=status))


def inspect_service(manager, service):
    """現在のmanager観測を取得し、通信失敗を未実行へ変えない。"""
    result = subprocess.run(["systemctl", manager, "show", service, "-p", "LoadState",
                             "-p", "Description", "-p", "ActiveState"],
                            capture_output=True, text=True, timeout=2)
    if result.returncode:
        raise OSError("cannot inspect job service")
    return dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)


def query(directory, token, inspect=inspect_service):
    """確定記録を優先し、未確定要求は同じtokenの生存観測以外をunknownとする。"""
    validate_token(token)
    with store_directory(directory) as fd:
        request = request_record(fd, token)
        try:
            record = read_record(fd, token + ".result.json")
        except FileNotFoundError:
            record = None
        if record is not None:
            return validate_result(record, request)
    state = "unknown"
    try:
        properties = inspect(request["manager"], request["service"])
        if (properties.get("LoadState") == "loaded" and properties.get("Description") == "jade-pdf2docx-job-" + token
                and properties.get("ActiveState") == "active"):
            state = "running"
    except (OSError, ValueError, subprocess.SubprocessError):
        pass
    return dict(request, state=state, output="unknown")


def stop_property(directory, token, service):
    """transient property用に引用し、環境変数を展開しない終了フックを返す。"""
    validate_token(token)
    arguments = [str(Path(__file__).resolve()), "finish", str(directory), token, service]
    def quote(value):
        """制御文字を拒否して引用する。D-Bus propertyのpercentはliteralで渡す。"""
        if any(ord(char) < 32 or ord(char) == 127 for char in value):
            raise ValueError("control character in history hook argument")
        return '"' + value.replace('\\', '\\\\').replace('"', '\\"') + '"'
    return "--property=ExecStopPost=+:/usr/bin/python3 " + " ".join(map(quote, arguments))


def main():
    """終了フックと独立status照会を、変換を再送せず実行する。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("finish", "status", "stop-property"))
    parser.add_argument("directory", type=Path)
    parser.add_argument("token")
    parser.add_argument("service", nargs="?")
    args = parser.parse_args()
    try:
        if args.operation == "status":
            print(json.dumps(query(args.directory, args.token), sort_keys=True))
        elif args.service is None:
            parser.error("service required for completion hook")
        elif args.operation == "finish":
            finish(args.directory, args.token, args.service, os.environ)
        else:
            print(stop_property(args.directory, args.token, args.service))
        return 0
    except (OSError, ValueError, HistoryBusy, subprocess.SubprocessError) as error:
        print("job history failed: " + str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
