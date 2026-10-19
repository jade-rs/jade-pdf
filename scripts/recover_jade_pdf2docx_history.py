#!/usr/bin/env python3
"""中断した履歴fileの公開だけを完了し、変換や終了結果の推測をしない。"""
import argparse
import json
import os
from pathlib import Path
import sys

import jade_pdf2docx_job_history as history
from jade_pdf2docx_durable_queue import validate_queue


def record_pair(fd, name):
    """二つの名前が同じinodeだけを指すことを確認し、記録を厳密に読む。"""
    pending = ".pending-" + name
    entries = []
    for candidate in (name, pending):
        try:
            entries.append(os.stat(candidate, dir_fd=fd, follow_symlinks=False))
        except FileNotFoundError:
            entries.append(None)
    published, temporary = entries
    if published is None and temporary is None:
        return None
    if published is not None and temporary is not None:
        if (published.st_dev, published.st_ino) != (temporary.st_dev, temporary.st_ino):
            raise ValueError("conflicting published and pending history records")
    source = name if published is not None else pending
    record = history.read_record(fd, source, expected_links=sum(x is not None for x in entries))
    return dict(name=name, pending=pending, published=published is not None,
                temporary=temporary is not None, record=record)


def publish_pair(fd, pair):
    """内容と公開名を同期してから一時名を外す。中断しても同じ手順を再実行できる。"""
    if not pair["temporary"]:
        return False
    source = os.open(pair["pending"], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                     dir_fd=fd)
    try:
        os.fsync(source)
    finally:
        os.close(source)
    if not pair["published"]:
        os.link(pair["pending"], pair["name"], src_dir_fd=fd, dst_dir_fd=fd, follow_symlinks=False)
    os.fsync(fd)
    os.unlink(pair["pending"], dir_fd=fd)
    os.fsync(fd)
    return True


def recover(directory, token):
    """共有排他下で全記録を先に検証し、存在する要求/終了記録の公開を完了する。"""
    history.validate_token(token)
    with history.store_directory(directory) as fd:
        with history.directory_gate(fd, history.HISTORY_LOCK_SECONDS):
            queue = record_pair(fd, token + ".queue.json")
            if queue is not None:
                validate_queue(queue["record"], token)
            request = record_pair(fd, token + ".request.json")
            if request is None and queue is None:
                raise ValueError("no recoverable job request")
            if request is not None:
                history.validate_request(request["record"], token)
                if queue is not None and (request["record"]["schema"] != 2
                        or any(request["record"][key] != queue["record"][key]
                               for key in ("token", "manager", "request_sha256"))):
                    raise ValueError("queued request and launch identity mismatch")
            result = record_pair(fd, token + ".result.json")
            if result is not None:
                if request is None:
                    raise ValueError("completion without launch request")
                history.validate_result(result["record"], request["record"])
            repaired = []
            for kind, pair in (("queue", queue), ("request", request), ("result", result)):
                if pair is not None and publish_pair(fd, pair):
                    repaired.append(kind)
            os.fsync(fd)
            if result is not None:
                record = result["record"]
            elif request is not None:
                record = dict(request["record"], state="unknown", output="unknown")
            else:
                record = dict(token=token, manager=queue["record"]["manager"],
                              request_sha256=queue["record"]["request_sha256"], state="queued", output="unknown")
            return dict(record, repaired=repaired)


def main():
    """一件を明示的に復旧し、状態JSONを返す。破損記録は変更せず拒否する。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("token")
    args = parser.parse_args()
    try:
        print(json.dumps(recover(args.directory, args.token), sort_keys=True))
        return 0
    except (OSError, ValueError, history.HistoryBusy) as error:
        print("history recovery failed: " + str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
