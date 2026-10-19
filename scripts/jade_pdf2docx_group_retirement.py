"""受付gate内で固定済みの元空cgroupだけを同期回収する。"""
import errno
import os
import time

from jade_pdf2docx_slot_retirement import empty_original


def group_removed(group_fd):
    """kernfsのリンク数に依存せず、元FDのevents消失で削除済みと判断する。"""
    try:
        fd = os.open("cgroup.events", os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=group_fd)
    except OSError as error:
        if error.errno in (errno.ENOENT, errno.ENODEV):
            return True
        raise
    os.close(fd)
    return False


def retire_original(group_fd, manager, service, token, inspect, timeout=5):
    """受付gate内でGCを待ち、同じinodeの元空群だけを削除して再利用を許可する。"""
    if group_removed(group_fd):
        return
    # 元群から親を固定する。元pathnameや新しい同名群を開き直さない。
    try:
        parent = os.open("..", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                         dir_fd=group_fd)
    except FileNotFoundError:
        if group_removed(group_fd):
            return
        raise
    deadline = time.monotonic() + timeout
    try:
        while True:
            if group_removed(group_fd):
                return
            original = os.fstat(group_fd)
            properties = inspect(manager, service[:-8])
            if properties.get("LoadState") == "not-found":
                try:
                    current = os.stat(service, dir_fd=parent, follow_symlinks=False)
                    if not os.path.samestat(original, current):
                        raise ValueError("original group identity changed; no removal sent")
                    if empty_original(group_fd) is None:
                        return
                    # 同じgateを使う受付はまだ再利用できない。kernelも生存群のrmdirを拒否する。
                    os.rmdir(service, dir_fd=parent)
                    return
                except FileNotFoundError:
                    if group_removed(group_fd):
                        return
                    raise ValueError("original group path disappeared without retirement")
                except OSError as error:
                    if error.errno not in (errno.EBUSY, errno.ENOTEMPTY):
                        raise
            elif (properties.get("LoadState") != "loaded" or
                  properties.get("Description") != "jade-pdf2docx-job-" + token):
                raise ValueError("job identity changed during retirement; no removal sent")
            if time.monotonic() >= deadline:
                raise TimeoutError("original group retirement timed out; inspect job before retry")
            time.sleep(0.02)
    finally:
        os.close(parent)

