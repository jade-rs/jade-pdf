#!/usr/bin/env python3
"""Fetch a hash-pinned public PDF corpus; never silently accept changed sources."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import sys
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
MAX_BYTES = 32 * 1024 * 1024


def fetch(entry, destination, offline=False):
    path = destination / (entry["id"] + ".pdf")

    def valid(data):
        return (
            data.startswith(b"%PDF-")
            and len(data) == entry["bytes"]
            and hashlib.sha256(data).hexdigest() == entry["sha256"]
        )

    if path.exists():
        if valid(path.read_bytes()):
            return entry["id"] + ": verified"
        raise ValueError(f"{path}: checksum mismatch; remove the corrupt cache and retry")
    if offline:
        raise FileNotFoundError(f"{path}: missing in offline mode")
    request = urllib.request.Request(
        entry["url"], headers={"User-Agent": "jade-pdf-corpus/1"}
    )
    for attempt in range(2):
        try:
            with urllib.request.urlopen(request, timeout=45) as response:
                data = response.read(MAX_BYTES + 1)
            break
        except urllib.error.HTTPError as error:
            if attempt == 1 or error.code != 503:
                raise
            error.close()
            time.sleep(1)
        except (TimeoutError, urllib.error.URLError) as error:
            reason = error.reason if isinstance(error, urllib.error.URLError) else error
            if attempt == 1 or not isinstance(reason, TimeoutError):
                raise
    if len(data) > MAX_BYTES or not valid(data):
        raise ValueError(f"{entry['id']}: unexpected PDF size or SHA-256; source may have changed")
    temporary = path.with_suffix(".pdf.part")
    try:
        temporary.write_bytes(data)
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)
    return entry["id"] + ": downloaded and verified"


def sources(manifest):
    """意味精度が未達の資料も取得し、通常の成功件数とは別に保持する。"""
    entries = [entry for group in ("documents", "robustness", "references", "pending_semantic")
               for entry in manifest.get(group, [])]
    identifiers = set()
    for entry in entries:
        if entry["id"] in identifiers:
            raise ValueError(f"Duplicate corpus ID: {entry['id']}")
        identifiers.add(entry["id"])
    return entries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="only verify cached files")
    parser.add_argument("--destination", type=Path, default=ROOT / "corpus/cache")
    args = parser.parse_args()
    manifest = json.loads((ROOT / "corpus/manifest.json").read_text())
    args.destination.mkdir(parents=True, exist_ok=True)
    failures = []
    with ThreadPoolExecutor(max_workers=4) as pool:
        futures = [
            (entry["id"], pool.submit(fetch, entry, args.destination, args.offline))
            for entry in sources(manifest)
        ]
        for identifier, future in futures:
            try:
                print(future.result(), flush=True)
            except Exception as error:
                failures.append(f"{identifier}: {error}")
    for failure in failures:
        print(failure, file=sys.stderr)
    return bool(failures)


if __name__ == "__main__":
    sys.exit(main())
