"""未追跡slotの残留空cgroupを、受付gate内で起動前に回収する。"""
from contextlib import contextmanager
import errno
import os
from pathlib import Path
import subprocess
import time


CGROUP_ROOT = Path("/sys/fs/cgroup")


@contextmanager
def pin_slice(control_group, slice_name):
    """managerのpool群を各階層nofollowで固定し、親群への逸脱を拒否する。"""
    parts = control_group.split("/")
    if (len(parts) < 2 or parts[0] or parts[-1] != slice_name or
            any(part in ("", ".", "..") for part in parts[1:])):
        raise ValueError("invalid pool control group")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    fd = os.open(CGROUP_ROOT, flags)
    try:
        for part in parts[1:]:
            child = os.open(part, flags, dir_fd=fd)
            os.close(fd)
            fd = child
        yield fd
    finally:
        os.close(fd)


def empty_original(group_fd):
    """元群の空を厳密に確認し、削除済みならNone、生存/不正なら拒否する。"""
    try:
        fd = os.open("cgroup.events", os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=group_fd)
        with os.fdopen(fd, "rb") as events:
            data = events.read(4097)
    except OSError as error:
        if error.errno in (errno.ENOENT, errno.ENODEV):
            return None
        raise
    populated = [line for line in data.splitlines() if line.startswith(b"populated ")]
    if len(data) > 4096 or populated != [b"populated 0"]:
        raise ValueError("untracked slot is populated or has invalid events; no job started")
    return True


def retire_slot(manager, slice_name, unit, inspect, timeout=5):
    """未追跡の同一空群だけを同期削除し、元FDを新jobへ再利用させない。"""
    properties = inspect(manager, slice_name)
    if properties.get("LoadState") == "loaded" and properties.get("ControlGroup") == "":
        # 初回受付ではservice起動前に共有sliceを具現化し、従来の自動起動を維持する。
        subprocess.run(["systemctl", manager, "start", "--", slice_name],
                       capture_output=True, text=True, timeout=5, check=True)
        properties = inspect(manager, slice_name)
    group = properties.get("ControlGroup")
    if properties.get("LoadState") != "loaded" or not isinstance(group, str):
        raise ValueError("cannot locate configured pool control group")
    service = unit + ".service"
    if "/" in service or not unit.startswith("jade-pdf2docx-" + slice_name[:-6] + "-slot-"):
        raise ValueError("slot outside configured pool")
    with pin_slice(group, slice_name) as parent:
        try:
            original = os.open(service, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                               dir_fd=parent)
        except FileNotFoundError:
            return
        try:
            deadline = time.monotonic() + timeout
            while True:
                if empty_original(original) is None:
                    return
                if inspect(manager, unit).get("LoadState") != "not-found":
                    raise ValueError("slot became manager-owned; no removal or launch sent")
                try:
                    current = os.stat(service, dir_fd=parent, follow_symlinks=False)
                    if not os.path.samestat(os.fstat(original), current):
                        raise ValueError("residual group identity changed; no removal or launch sent")
                    # 受付gateで新jobを排除し、kernelも生存群のrmdirを拒否する。
                    os.rmdir(service, dir_fd=parent)
                    return
                except FileNotFoundError:
                    if empty_original(original) is None:
                        return
                    raise ValueError("residual path disappeared without retirement")
                except OSError as error:
                    if error.errno not in (errno.EBUSY, errno.ENOTEMPTY):
                        raise
                if time.monotonic() >= deadline:
                    raise TimeoutError("residual slot retirement timed out; no job started")
                time.sleep(0.02)
        finally:
            os.close(original)
