"""履歴保存先ごとの排他と、破損/中断を含めた保守的な保持枠を管理する。"""
from contextlib import contextmanager
import fcntl
import os
import re
import time


class HistoryBusy(Exception):
    """保持上限または受付排他の期限に達し、新jobを起動せず拒否する。"""


@contextmanager
def directory_gate(fd, seconds):
    """保存先inodeを期限付きで排他し、lockを子へ継承しない。"""
    deadline = time.monotonic() + seconds
    while True:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            break
        except BlockingIOError:
            if time.monotonic() >= deadline:
                raise HistoryBusy("history admission lock timed out")
            time.sleep(0.01)
    try:
        yield
    finally:
        fcntl.flock(fd, fcntl.LOCK_UN)


@contextmanager
def capacity_gate(fd, token, maximum, seconds):
    """未完了・破損・中断領域も保持枠へ数え、受付から公開まで排他する。"""
    with directory_gate(fd, seconds):
        check_capacity(fd, token, maximum)
        yield


def check_capacity(fd, token, maximum, *, reserved=False):
    """呼出側が保存先排他を保持した状態で、保持枠とtoken重複を調べる。"""
    # flock待機前に開いたFDのdirectory cacheを破棄し、公開済みentryを数える。
    os.lseek(fd, 0, os.SEEK_SET)
    slots = set()
    with os.scandir(fd) as entries:
        for index, entry in enumerate(entries):
            # 既知tokenはキュー/要求/終了/各一時fileの最大6名を一枠へ集約する。
            # 旧版の一時fileや任意entryも一枠を消費し、無制限scanを避ける。
            match = re.fullmatch(r"(?:\.pending-)?([a-f0-9]{32})\.(?:queue|request|result)\.json", entry.name)
            if reserved and match and match[1] == token:
                continue
            slots.add(("token", match[1]) if match else ("entry", entry.name))
            if len(slots) >= maximum or index >= 6 * maximum:
                raise HistoryBusy("job history full; preserve records and rotate store offline")
    if ("token", token) in slots:
        raise FileExistsError("job history token already reserved")
