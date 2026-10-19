#!/usr/bin/env python3
"""poolのjob tokenを照合し、固定した元cgroupだけを強制停止する。"""
import argparse
from contextlib import contextmanager
import errno
import os
from pathlib import Path
import re
import subprocess
import sys
import time

import run_jade_pdf2docx_pool as pool
from jade_pdf2docx_group_retirement import retire_original


CGROUP_ROOT = Path("/sys/fs/cgroup")


@contextmanager
def pin_group(control_group):
    """各directoryをnofollowで開き、元kernel cgroupのFDを停止まで保持する。"""
    parts = control_group.split("/")
    if len(parts) < 2 or parts[0] or any(part in ("", ".", "..") for part in parts[1:]):
        raise ValueError("invalid job control group")
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


def kill_group(group_fd, timeout=5):
    """元FDから一度だけ群停止し、元群の空/削除を期限付きで確認する。"""
    kill_fd = os.open("cgroup.kill", os.O_WRONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=group_fd)
    try:
        if os.write(kill_fd, b"1\n") != 2:
            raise OSError("incomplete group kill write")
    finally:
        os.close(kill_fd)
    deadline = time.monotonic() + timeout
    while True:
        try:
            fd = os.open("cgroup.events", os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=group_fd)
            with os.fdopen(fd, "rb") as events:
                data = events.read(4097)
            if b"populated 0" in data.splitlines():
                return
        except OSError as error:
            if error.errno in (errno.ENOENT, errno.ENODEV):
                return  # 元directoryが削除された。現在のpathnameは開き直さない。
            raise
        if time.monotonic() >= deadline:
            raise TimeoutError("cancel requested but original group still populated; inspect job before retry")
        time.sleep(0.02)


def retire_group(group_fd, manager, service, token, timeout=5):
    """取消で固定した元群の回収を、共通の所有権検査へ渡す。"""
    retire_original(group_fd, manager, service, token, pool.unit_properties, timeout=timeout)


def cancel_job(policy, manager, directory, service, token):
    """受付と同じgate内でtokenを照合し、再利用前の群へだけ停止を送る。"""
    units = {f"jade-pdf2docx-{policy['slice'][:-6]}-slot-{slot}.service"
             for slot in range(1, policy["slots"] + 1)}
    if service not in units or not re.fullmatch(r"[a-f0-9]{32}", token):
        raise ValueError("invalid pool service or job token")
    with pool.startup_gate(directory, policy):
        properties = pool.unit_properties(manager, service[:-8])
        if properties.get("LoadState") != "loaded" or properties.get("Description") != "jade-pdf2docx-job-" + token:
            raise ValueError("job identity mismatch; no cancellation sent")
        group = properties.get("ControlGroup", "")
        parts = group.split("/")
        if parts[-2:] != [policy["slice"], service]:
            raise ValueError("job control group outside configured pool")
        with pin_group(group) as original:
            # GC/外部操作で観測中に置換された場合も、停止前に安全側へ拒否する。
            if pool.unit_properties(manager, service[:-8]) != properties:
                raise ValueError("job identity mismatch after group pin; no cancellation sent")
            kill_group(original)
            retire_group(original, manager, service, token)
    print("cancelled original job: " + service + "; token: " + token, file=sys.stderr)
    return 0


def main():
    """既定はuser manager、system managerは同じ管理者policyを必須とする。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--system", action="store_true")
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("service")
    parser.add_argument("token")
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("pool cancellation requires Linux cgroup v2")
    if args.system and os.geteuid() != 0:
        parser.error("--system requires administrator (sudo)")
    manager = "--system" if args.system else "--user"
    directory = Path("/run/jade-pdf2docx-pool") if args.system else Path(f"/run/user/{os.geteuid()}/jade-pdf2docx-pool")
    try:
        return cancel_job(pool.load_policy(args.config), manager, directory, args.service, args.token)
    except pool.PoolBusy as error:
        print(error, file=sys.stderr)
        return 75
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print("pool cancellation failed: " + str(error), file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        print("cancel waiter interrupted; inspect original job before retry", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
