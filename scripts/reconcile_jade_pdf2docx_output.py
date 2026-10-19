#!/usr/bin/env python3
"""終了履歴を変えず、現在のDOCXに含まれる要求照合情報を明示的に読む。"""
import argparse
import json
import os
from pathlib import Path
import stat
import struct
import sys

import jade_pdf2docx_job_history as history

TAIL_LIMIT = 65535 + 22


def open_output(path):
    """絶対pathを各階層で再解決し、linkを辿らず非blockingで開く。"""
    path = Path(path)
    if not path.is_absolute() or '..' in path.parts:
        raise ValueError('output path must be absolute without parent traversal')
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC
    directory = os.open(path.anchor, flags | os.O_DIRECTORY)
    try:
        for part in path.parts[1:-1]:
            child = os.open(part, flags | os.O_DIRECTORY, dir_fd=directory)
            os.close(directory)
            directory = child
        source = os.open(path.name, flags | os.O_NONBLOCK, dir_fd=directory)
    finally:
        os.close(directory)
    return source


def artifact_comment(path):
    """各階層のlinkと特殊fileを拒否し、固定上限のZIP末尾だけを読み取る。"""
    source = open_output(path)
    try:
        before = os.fstat(source)
        if not stat.S_ISREG(before.st_mode):
            raise ValueError('output must be a regular file')
    except BaseException:
        os.close(source)
        raise
    with os.fdopen(source, 'rb') as stream:
        start = max(0, before.st_size - TAIL_LIMIT)
        stream.seek(start)
        tail = stream.read(TAIL_LIMIT)
        after = os.fstat(stream.fileno())
        def identity(info):
            """読取中の書換を検出するため、inode・長さ・変更時刻を比較する。"""
            return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns
        if identity(before) != identity(after):
            raise ValueError('output changed while reading')
        current = open_output(path)
        try:
            if identity(after) != identity(os.fstat(current)):
                raise ValueError('output changed while reading')
        finally:
            os.close(current)
    for offset in range(len(tail) - 22, -1, -1):
        if tail[offset:offset+4] != b'PK\x05\x06':
            continue
        disk, central_disk, entries_disk, entries, size, position, comment_size = struct.unpack_from(
            '<4H2IH', tail, offset + 4)
        if (disk == central_disk == 0 and entries_disk == entries and entries > 0
                and position + size == start + offset and offset + 22 + comment_size == len(tail)):
            return tail[offset+22:]
    raise ValueError('invalid or unsupported DOCX ZIP footer')


def reconcile(directory, token, output):
    """元の終了状態と、現在の成果物の照合結果を独立に返す。再送や記録変更はしない。"""
    record = history.query(directory, token, inspect=lambda manager, service: {})
    if record['schema'] != 2:
        raise ValueError('output reconciliation requires schema 2 request')
    expected = ('JADE-PDF2DOCX-PUBLICATION-1\n' + token + ':' + record['request_sha256']).encode()
    try:
        comment = artifact_comment(output)
    except FileNotFoundError:
        publication = 'absent'
    else:
        publication = 'matched' if comment == expected else 'unmatched'
    return dict(record, publication=publication, inspected_output=str(output))


def main():
    """一要求の現在出力だけを照合し、観測JSONまたは明示的な失敗を返す。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('token')
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(reconcile(args.directory, args.token, args.output), sort_keys=True))
        return 0
    except (OSError, ValueError) as error:
        print('output reconciliation failed: ' + str(error), file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
