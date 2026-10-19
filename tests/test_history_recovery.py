"""履歴公開中の停止から復旧し、変換の再起動や結果の推測をしない。"""
import importlib.util
from contextlib import redirect_stdout
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
TOKEN = "a" * 32


class HistoryRecoveryTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("history_recovery", SCRIPTS / "recover_jade_pdf2docx_history.py")
        self.recovery = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
            spec.loader.exec_module(self.recovery)
        self.history = self.recovery.history
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.request = dict(schema=2, token=TOKEN, manager="--user",
                            service="jade-pdf2docx-recovery-slot-1.service", request_sha256="b" * 64)
        self.result = dict(self.request, state="succeeded", output="reported_published",
                           service_result="success", exit_code="exited", exit_status="0")

    def write(self, kind, record, pending=False):
        path = self.root / ((".pending-" if pending else "") + TOKEN + "." + kind + ".json")
        path.write_text(json.dumps(record))
        path.chmod(0o600)
        return path

    def snapshot(self):
        return {p.name: p.read_bytes() for p in self.root.iterdir() if p.is_file()}

    def recover(self):
        return self.recovery.recover(self.root, TOKEN)

    def test_complete_pending_claim_is_preserved_as_unknown_without_launch(self):
        self.write("request", self.request, pending=True)
        with patch.object(subprocess, "Popen", side_effect=AssertionError("no process launch")):
            result = self.recover()
        self.assertEqual(result["state"], "unknown")
        self.assertEqual(result["repaired"], ["request"])
        self.assertEqual(self.history.query(self.root, TOKEN, lambda *a: {})["state"], "unknown")
        before = self.snapshot()
        self.assertEqual(self.recover()["repaired"], [])
        self.assertEqual(self.snapshot(), before)

    def test_same_inode_publication_interruptions_are_recovered_for_both_records(self):
        for kind, record in (("request", self.request), ("result", self.result)):
            published = self.write(kind, record)
            os.link(published, self.root / (".pending-" + published.name))
        with self.assertRaises(ValueError):
            self.history.query(self.root, TOKEN)
        self.assertEqual(self.recover()["repaired"], ["request", "result"])
        result = self.history.query(self.root, TOKEN, lambda *a: self.fail("no manager query"))
        self.assertEqual(result["state"], "succeeded")
        self.assertEqual(result["output"], "reported_published")
        self.assertEqual(len(list(self.root.iterdir())), 2)

    def test_pending_completion_is_validated_and_published_without_changing_output(self):
        self.write("request", self.request)
        self.write("result", self.result, pending=True)
        output = self.root / "existing.docx"
        output.write_bytes(b"preserve current output")
        self.assertEqual(self.recover()["state"], "succeeded")
        self.assertEqual(output.read_bytes(), b"preserve current output")

    def test_conflicting_inodes_and_partial_json_are_rejected_without_mutation(self):
        self.write("request", self.request)
        pending = self.write("request", self.request, pending=True)
        for content in (json.dumps(self.request), '{"schema":'):
            pending.write_text(content)
            before = self.snapshot()
            with self.assertRaises(ValueError):
                self.recover()
            self.assertEqual(self.snapshot(), before)

    def test_invalid_completion_prevents_even_valid_claim_repair(self):
        self.write("request", self.request, pending=True)
        self.write("result", dict(self.result, token="c" * 32), pending=True)
        before = self.snapshot()
        with self.assertRaises(ValueError):
            self.recover()
        self.assertEqual(self.snapshot(), before)

    def test_extra_hardlinks_and_symlinks_are_rejected(self):
        pending = self.write("request", self.request, pending=True)
        os.link(pending, self.root / (TOKEN + ".request.json"))
        extra = self.root / "unrelated-link"
        os.link(pending, extra)
        with self.assertRaises(ValueError):
            self.recover()
        extra.unlink()
        pending.unlink()
        pending.symlink_to(self.root / (TOKEN + ".request.json"))
        with self.assertRaises((ValueError, OSError)):
            self.recover()

    def test_recovery_itself_can_be_interrupted_after_link_and_retried(self):
        self.write("request", self.request, pending=True)
        with patch.object(self.recovery.os, "unlink", side_effect=OSError("interrupted")):
            with self.assertRaises(OSError):
                self.recover()
        self.assertEqual(self.recover()["state"], "unknown")
        self.assertEqual(self.recover()["repaired"], [])

    def test_shared_directory_gate_prevents_repair_during_writer(self):
        self.write("request", self.request, pending=True)
        before = self.snapshot()
        with self.history.store_directory(self.root) as fd:
            with self.history.directory_gate(fd, 1):
                with patch.object(self.history, "HISTORY_LOCK_SECONDS", 0):
                    with self.assertRaises(self.history.HistoryBusy):
                        self.recover()
        self.assertEqual(self.snapshot(), before)

    def test_cli_reports_restored_failed_result_and_invalid_token(self):
        self.write("request", self.request)
        self.write("result", dict(self.result, state="failed", output="unknown",
                                  service_result="exit-code", exit_status="7"), pending=True)
        command = [sys.executable, str(SCRIPTS / "recover_jade_pdf2docx_history.py"), str(self.root), TOKEN]
        result = subprocess.run(command, capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["state"], "failed")
        result = subprocess.run(command[:-1] + ["../bad"], capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 2)
        self.assertIn(b"history recovery failed:", result.stderr)

    def test_real_sigkill_after_publication_can_be_recovered_without_reexecuting_claim(self):
        spec = importlib.util.spec_from_file_location("recovery_pool", SCRIPTS / "run_jade_pdf2docx_pool.py")
        pool = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
            spec.loader.exec_module(pool)
        policy = dict(schema=1, slice="jade-pdf2docxRecovery.slice", slots=1,
                      total_memory_mib=1024, job_memory_mib=512)
        command = ["/opt/jade-pdf2docx", "/input.pdf", "/output.docx", "--input-sha256", "c" * 64]
        digest = pool.identity.fingerprint(policy, "--user", command)
        child = '''import os, signal, sys
sys.path.insert(0, sys.argv[1])
import jade_pdf2docx_job_history as history
import jade_pdf2docx_request_identity as identity
def stop(name, **kwargs):
    os.kill(os.getpid(), signal.SIGKILL)
history.os.unlink = stop
identity.claim(sys.argv[2], sys.argv[3], sys.argv[4], "--user", "jade-pdf2docx-recovery-slot-1.service")
'''
        process = subprocess.run([sys.executable, "-c", child, str(SCRIPTS), str(self.root), TOKEN, digest],
                                 capture_output=True, timeout=5)
        self.assertEqual(process.returncode, -9, process.stderr)
        self.assertEqual(self.recover()["state"], "unknown")
        output = io.StringIO()
        with patch.object(pool.subprocess, "Popen", side_effect=AssertionError("no launch")), \
             patch.object(pool, "unit_properties", side_effect=AssertionError("no slot lookup")), \
             patch.object(pool.history, "inspect_service", return_value={"LoadState": "not-found"}), \
             redirect_stdout(output):
            self.assertEqual(pool.run_job(policy, "--user", self.root / "unused-gate", command,
                                          self.root, TOKEN), 75)
        self.assertTrue(json.loads(output.getvalue())["replayed"])
