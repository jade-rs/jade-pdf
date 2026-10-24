#!/usr/bin/env python3
"""Exercise pinned SDK cache readers while an installer holds the OS lock.

Uses a private copied cache and owned CLI process groups. Warm readers must
verify complete pinned bytes without waiting for installation. Same-size,
mtime-preserving corruption must join the repair lock, fail offline after it
is released, and recover after atomic publication. This is not a host sandbox
or a benchmark, and POSIX flock coverage does not qualify Windows.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def execute(binary, source, env, deadline=15):
    started = time.monotonic()
    process = subprocess.Popen([str(binary), "--ir", str(source)], env=env,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=True)
    timed_out = False
    try:
        stdout, stderr = process.communicate(timeout=deadline)
    except subprocess.TimeoutExpired:
        timed_out = True
        os.killpg(process.pid, signal.SIGTERM)
        try:
            stdout, stderr = process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate(timeout=5)
    return dict(exit_code=process.returncode, timed_out=timed_out,
                elapsed_seconds=round(time.monotonic() - started, 4),
                stdout=stdout, stderr=stderr)


def main():
    import fcntl
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/pdf-diff")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--library", type=Path)
    parser.add_argument("--report", type=Path, default=ROOT / "corpus/results-sdk-cache-contention.json")
    args = parser.parse_args()
    assert os.name == "posix", "Only POSIX flock is qualified by this audit"
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    pins = json.loads((ROOT / "crates/jade-pdf-diff/pdfium-assets.json").read_bytes())
    manifest_bytes = (ROOT / "corpus/manifest.json").read_bytes()
    hello = next(x for x in json.loads(manifest_bytes)["documents"] if x["id"] == "hello")
    source = ROOT / "corpus/cache/hello.pdf"
    assert sha(source) == hello["sha256"]
    if args.library is None:
        system = {"Darwin": "mac", "Linux": "linux"}[platform.system()]
        machine = {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x64"}[platform.machine()]
        asset = next(a for a in pins["assets"] if a["name"] == f"pdfium-{system}-{machine}.tgz")
        member, metadata = next(iter(asset["libraries"].items()))
        default = Path.home() / "Library/Caches" if system == "mac" else Path(os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache")))
        cache = Path(os.environ.get("JADE_PDFIUM_CACHE_DIR", os.environ.get("PDFIUM_AUTO_CACHE_DIR", str(default / "jade-pdf"))))
        args.library = cache / ("pdfium-" + pins["version"]) / metadata["sha256"] / Path(member).name
    library_hash = sha(args.library)
    choices = [(name, data) for asset in pins["assets"] for name, data in asset["libraries"].items() if data["sha256"] == library_hash]
    assert len(choices) == 1
    member, metadata = choices[0]
    assert args.library.stat().st_size == metadata["bytes"]
    binary = args.binary.resolve()
    results = []
    with tempfile.TemporaryDirectory(prefix="jade-sdk-contention-") as temporary:
        cache = Path(temporary)
        directory = cache / ("pdfium-" + pins["version"]) / library_hash
        directory.mkdir(parents=True)
        target = directory / Path(member).name
        shutil.copy2(args.library, target)
        env = dict(os.environ, JADE_PDFIUM_CACHE_DIR=str(cache), PDFIUM_NO_AUTO_DOWNLOAD="1")
        env.pop("PDFIUM_LIB_PATH", None)
        with (directory / "install.lock").open("a+b") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            with ThreadPoolExecutor(max_workers=8) as pool:
                warm = list(pool.map(lambda _: execute(binary, source, env), range(8)))
            for i, row in enumerate(warm):
                assert not row["timed_out"] and row["exit_code"] == 0, row
                assert len(json.loads(row["stdout"])["pages"]) == 1
                results.append(dict(id=f"verified-warm-{i}", passed=True,
                                    elapsed_seconds=row["elapsed_seconds"]))
            if args.baseline:
                old = execute(args.baseline.resolve(), source, env, 2)
                assert old["timed_out"], "Baseline unexpectedly avoided the installer lock"
                results.append(dict(id="baseline-installer-wait", passed=True,
                                    observed_timeout=True, elapsed_seconds=old["elapsed_seconds"]))
            stat = target.stat()
            data = bytearray(target.read_bytes())
            data[0] ^= 1
            target.write_bytes(data)
            os.utime(target, ns=(stat.st_atime_ns, stat.st_mtime_ns))
            assert target.stat().st_size == stat.st_size and sha(target) != library_hash
            blocked = execute(binary, source, env, 2)
            assert blocked["timed_out"] and not blocked["stdout"], blocked
            results.append(dict(id="corrupt-cache-joins-repair-lock", passed=True,
                                same_size=True, preserved_mtime=True))
            fcntl.flock(lock, fcntl.LOCK_UN)
        rejected = execute(binary, source, env)
        assert not rejected["timed_out"] and rejected["exit_code"] != 0 and not rejected["stdout"]
        assert b"automatic download is disabled" in rejected["stderr"], rejected
        results.append(dict(id="corruption-rejected-offline", passed=True,
                            no_partial_output=True))
        staged = directory / "repaired-library"
        shutil.copy2(args.library, staged)
        os.replace(staged, target)
        recovered = execute(binary, source, env)
        assert recovered["exit_code"] == 0 and not recovered["timed_out"]
        assert len(json.loads(recovered["stdout"])["pages"]) == 1
        assert sha(target) == library_hash
        results.append(dict(id="atomic-repair-recovery", passed=True))
    report = dict(version=1, methodology=__doc__, platform=platform.system()+" "+platform.machine(),
                  manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
                  binary_sha256=sha(binary), sdk_library_sha256=library_hash,
                  tool_sha256=sha(Path(__file__)), cases=len(results),
                  passed=sum(x["passed"] for x in results), results=results)
    if args.baseline:
        report["baseline_binary_sha256"] = sha(args.baseline)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2)+"\n")
    print(f"{report['passed']}/{report['cases']} cache contention/recovery checks passed")


if __name__ == "__main__":
    main()
