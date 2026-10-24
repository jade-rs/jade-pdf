#!/usr/bin/env python3
"""Exercise native process supervision on the hash-pinned public corpus.

Checks all-page IR success/explicit failure, measured child resources and
bounded CLI failures followed by recovery. Fault injection and atomic PNG
publication are covered by Rust isolation tests, not by this public-data audit.
Memory sampling can overshoot between polls and is not a security sandbox.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

from pdf_corpus import ROOT, fetch


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,default=ROOT/'target/release/pdf-diff')
    parser.add_argument('--probe',type=Path,default=ROOT/'target/release/examples/audit_isolation')
    parser.add_argument('--report',type=Path,default=ROOT/'corpus/results-process-isolation.json')
    args=parser.parse_args()
    args.report.unlink(missing_ok=True)
    manifest_bytes=(ROOT/'corpus/manifest.json').read_bytes()
    manifest=json.loads(manifest_bytes)
    entries=manifest['documents']+manifest.get('robustness',[])
    sources=[]
    for entry in entries:
        fetch(entry,ROOT/'corpus/cache',offline=True)
        sources.append(ROOT/'corpus/cache'/(entry['id']+'.pdf'))
    with tempfile.TemporaryDirectory(prefix='jade-process-audit-') as temporary:
        directory=Path(temporary)
        binary=directory/'pdf-diff';probe=directory/'audit-isolation'
        shutil.copy2(args.binary,binary);shutil.copy2(args.probe,probe)
        binary_hash=hashlib.sha256(binary.read_bytes()).hexdigest()
        probe_hash=hashlib.sha256(probe.read_bytes()).hexdigest()
        process=subprocess.run([str(probe),str(binary),*map(str,sources)],capture_output=True,text=True,timeout=900)
        assert process.returncode==0,process.stderr[-3000:]
        rows=[json.loads(line) for line in process.stdout.splitlines()]
        assert len(rows)==len(entries),'Audit did not cover every source'
        for row,entry in zip(rows,entries):
            expected=2 if entry.get('outcome') in ['geometry_rejected','password_required'] else 0
            assert row['exit_code']==expected,(entry['id'],row)
            if expected==0:
                assert row['pages']==entry['pages'],(entry['id'],'Wrong page count')
            else:
                assert row['stdout_bytes']==0,(entry['id'],'Partial stdout published')
            assert row['stats']['peak_memory_bytes']>0
            assert row['stats']['peak_memory_bytes']<=2*1024**3
            assert row['stats']['elapsed_ms']<=120000
            assert row['stats']['output_bytes']<=64*1024**2
            row['id']=entry['id'];row['passed']=True
            print('PASS '+entry['id'],flush=True)
        hello=ROOT/'corpus/cache/hello.pdf'
        failures=[]
        for flag in ['--timeout-ms','--max-memory-mb']:
            process=subprocess.run([str(binary),flag,'1',str(hello),str(hello)],capture_output=True,timeout=10)
            assert process.returncode==2 and not process.stdout and process.stderr
            failures.append({'flag':flag,'value':1,'exit_code':2,'stdout_bytes':0,'error':process.stderr.decode(errors='replace').strip()})
        recovery=subprocess.run([str(binary),str(hello),str(hello)],capture_output=True,timeout=30)
        assert recovery.returncode==0 and json.loads(recovery.stdout)['summary']['pages_modified']==0
    report={'version':1,'platform':platform.system()+' '+platform.machine(),'binary_sha256':binary_hash,'probe_sha256':probe_hash,'manifest_sha256':hashlib.sha256(manifest_bytes).hexdigest(),'methodology':__doc__,'limits':{'timeout_ms':120000,'max_memory_bytes':2*1024**3,'max_output_bytes':64*1024**2,'poll_sleep_ms':10},'normal_documents':len(manifest['documents']),'robustness_documents':len(manifest.get('robustness',[])),'cases':len(rows),'passed':len(rows),'maximum_observed_memory_bytes':max(r['stats']['peak_memory_bytes'] for r in rows),'maximum_elapsed_ms':max(r['stats']['elapsed_ms'] for r in rows),'limited_cli_operations':failures,'recovery_passed':True,'results':rows}
    args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(f"{len(rows)}/{len(rows)} supervised operations passed")


if __name__=='__main__':
    main()
