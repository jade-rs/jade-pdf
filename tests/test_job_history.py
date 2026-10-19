"""待機側の寿命と独立した記録を再読込し、不明を成功に変えない。"""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/jade_pdf2docx_job_history.py"


class JobHistoryTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("jade_pdf2docx_history", SCRIPT)
        self.history = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(SCRIPT.parent), *sys.path]):
            spec.loader.exec_module(self.history)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve() / "jobs $literal%name"
        self.root.mkdir(mode=0o700)
        self.token = "a" * 32
        self.unit = "jade-pdf2docx-jade-pdf2docxTest-slot-1.service"

    def prepare(self):
        self.history.prepare(self.root, self.token, "--system", self.unit)

    def finish(self, result="success", code="exited", status="0"):
        self.history.finish(self.root, self.token, self.unit,
                            {"SERVICE_RESULT": result, "EXIT_CODE": code, "EXIT_STATUS": status})

    def query(self, properties=None):
        return self.history.query(self.root, self.token,
                                  lambda manager, unit: properties or {"LoadState": "not-found"})

    def test_success_survives_reader_restart_without_manager_or_output_file(self):
        self.prepare()
        self.finish()
        result = subprocess.run([sys.executable, str(SCRIPT), "status", str(self.root), self.token],
                                capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        record = json.loads(result.stdout)
        self.assertEqual(record["state"], "succeeded")
        self.assertEqual(record["output"], "reported_published")
        self.assertEqual(record["service"], self.unit)

    def test_failure_signal_oom_and_missing_completion_never_claim_publication(self):
        self.prepare()
        self.assertEqual(self.query()["state"], "unknown")
        for result, code, status, expected in (
                ("exit-code", "exited", "1", "failed"), ("signal", "killed", "KILL", "failed"),
                ("oom-kill", "killed", "KILL", "failed"), ("success", "killed", "TERM", "failed"),
                ("", "", "", "unknown"), ("success", "", "", "unknown")):
            with self.subTest(result=result):
                self.finish(result, code, status)
                record = self.query()
                self.assertEqual(record["state"], expected)
                self.assertEqual(record["output"], "unknown")
                (self.root / (self.token + ".result.json")).unlink()

    def test_live_same_token_is_observed_but_reused_service_stays_unknown(self):
        self.prepare()
        properties = {"LoadState": "loaded", "Description": "jade-pdf2docx-job-" + self.token,
                      "ActiveState": "active"}
        self.assertEqual(self.query(properties)["state"], "running")
        properties["Description"] = "jade-pdf2docx-job-" + "b" * 32
        self.assertEqual(self.query(properties)["state"], "unknown")
        def unavailable(*args):
            raise OSError("manager unavailable")
        self.assertEqual(self.history.query(self.root, self.token, unavailable)["state"], "unknown")

    def test_duplicate_token_terminal_record_and_identity_change_are_rejected(self):
        self.prepare()
        with self.assertRaises(FileExistsError):
            self.prepare()
        with self.assertRaises(ValueError):
            self.history.finish(self.root, self.token, "other.service", {})
        self.finish()
        before = (self.root / (self.token + ".result.json")).read_bytes()
        with self.assertRaises(FileExistsError):
            self.finish("exit-code", "exited", "1")
        self.assertEqual((self.root / (self.token + ".result.json")).read_bytes(), before)

    def test_unsafe_store_links_and_corrupt_records_fail_closed(self):
        self.root.chmod(0o755)
        with self.assertRaises(ValueError):
            self.prepare()
        self.root.chmod(0o700)
        link = self.root.parent / "link"
        link.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(OSError):
            self.history.prepare(link, self.token, "--system", self.unit)
        self.prepare()
        path = self.root / (self.token + ".request.json")
        original = path.read_bytes()
        for content in (b"{", b"x" * 4097, b'{"schema":1,"schema":1}', b'[]'):
            path.write_bytes(content)
            with self.assertRaises(ValueError):
                self.query()
        path.write_bytes(original)
        result = self.root / (self.token + ".result.json")
        result.symlink_to(path)
        with self.assertRaises(OSError):
            self.query()

    def test_invalid_tokens_reject_before_file_access(self):
        for token in ("", "../file", "A" * 32, "a" * 31, "a" * 33, "a" * 32 + "\n"):
            with self.assertRaises(ValueError):
                self.history.prepare(self.root / "absent", token, "--system", self.unit)

    def test_partial_write_is_not_visible_as_a_complete_record(self):
        self.prepare()
        with patch.object(self.history.os, "fsync", side_effect=OSError("disk failure")):
            with self.assertRaises(OSError):
                self.finish()
        self.assertEqual(self.query()["state"], "unknown")
        self.assertFalse((self.root / (self.token + ".result.json")).exists())

    def test_terminal_identity_types_and_inconsistent_result_are_rejected(self):
        self.prepare()
        self.finish()
        path = self.root / (self.token + ".result.json")
        original = json.loads(path.read_bytes())
        for changes in ({"schema": True}, {"token": "b" * 32}, {"state": "failed"},
                        {"output": "unknown"}, {"exit_status": "1"}, {"extra": 0}):
            with self.subTest(changes=changes):
                path.write_text(json.dumps(dict(original, **changes)))
                with self.assertRaises(ValueError):
                    self.query()

    def test_stop_hook_quotes_paths_without_environment_or_specifier_expansion(self):
        value = self.history.stop_property(self.root, self.token, self.unit)
        self.assertTrue(value.startswith("--property=ExecStopPost=+:/usr/bin/python3 "))
        self.assertIn("%name", value)
        self.assertNotIn("%%name", value)
        self.assertIn("$literal", value)
        self.assertIn('"' + str(SCRIPT) + '"', value)
        self.assertIn('"finish"', value)
