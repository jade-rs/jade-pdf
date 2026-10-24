#!/usr/bin/env python3
"""Compare isolated byte operations with direct APIs on SHA-pinned public data.

The Rust probe rechecks bytes before using the host-side reference parser.
This differential gate covers transport/option/coordinate/PNG preservation,
not independent PDF semantic correctness or a portable hard memory ceiling.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from pdf_corpus import ROOT, fetch


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,default=ROOT/'target/release/pdf-diff')
    parser.add_argument('--probe',type=Path,default=ROOT/'target/release/examples/audit_isolated_bytes')
    parser.add_argument('--report',type=Path,default=ROOT/'corpus/results-isolated-bytes.json')
    args=parser.parse_args()
    args.report.unlink(missing_ok=True)
    manifest_bytes=(ROOT/'corpus/manifest.json').read_bytes();manifest=json.loads(manifest_bytes)
    for entry in manifest['documents']+manifest.get('robustness',[]):fetch(entry,ROOT/'corpus/cache',offline=True)
    with tempfile.TemporaryDirectory(prefix='jade-byte-audit-') as temporary:
        directory=Path(temporary);binary=directory/'pdf-diff';probe=directory/'audit-isolated-bytes'
        shutil.copy2(args.binary,binary);shutil.copy2(args.probe,probe)
        binary_hash=hashlib.sha256(binary.read_bytes()).hexdigest();probe_hash=hashlib.sha256(probe.read_bytes()).hexdigest()
        private=directory/'private';private.mkdir()
        process=subprocess.run([str(probe),str(binary),str(ROOT/'corpus/manifest.json'),str(ROOT/'corpus/cache'),str(private)],capture_output=True,text=True,timeout=1800)
        assert process.returncode==0,process.stderr[-6000:]
        rows=[json.loads(line) for line in process.stdout.splitlines()]
        assert len(rows)==len(manifest['documents'])+len(manifest.get('robustness',[]))
        assert all(row['passed'] and row['private_directory_empty'] for row in rows)
        assert not list(private.iterdir()),'Retained private input data'
    operations=[operation for row in rows for operation in row['operations']]
    matched=[op for op in operations if op['status']=='matched']
    report={'version':1,'binary_sha256':binary_hash,'probe_sha256':probe_hash,'manifest_sha256':hashlib.sha256(manifest_bytes).hexdigest(),'methodology':__doc__,'documents':len(rows),'passed_documents':len(rows),'operation_checks':len(operations),'matching_successful_operations':len(matched),'matching_rejections':len(operations)-len(matched),'text_layer_checks':sum(op['operation'].startswith('text_layer_') for op in operations),'maximum_observed_child_memory_bytes':max(op['execution']['peak_memory_bytes'] for op in matched),'maximum_child_elapsed_ms':max(op['execution']['elapsed_ms'] for op in matched),'results':rows}
    args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(f"{len(rows)}/{len(rows)} documents, {len(operations)} operation checks matched")


if __name__=='__main__':
    main()
