"""終了時は最初に固定した元群だけを回収し、後続jobを保護する。"""
from contextlib import contextmanager
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/run_jade_pdf2docx_pool.py"


class JobCompletionTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("completion_pool", SCRIPT)
        self.pool = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(SCRIPT.parent), *sys.path]):
            spec.loader.exec_module(self.pool)
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.base = Path(temp.name)
        self.parent = self.base / "jade-pdf2docxTest.slice"
        self.parent.mkdir()
        self.group = self.parent / "jade-pdf2docx-jade-pdf2docxTest-slot-1.service"
        self.policy = dict(schema=1, slice="jade-pdf2docxTest.slice", slots=1,
                           total_memory_mib=1024, job_memory_mib=512)
        self.token = "a" * 32
        self.live = {"LoadState": "loaded", "Description": "jade-pdf2docx-job-" + self.token,
                     "ControlGroup": "/jade-pdf2docxTest.slice/" + self.group.name}
        self.state = {"LoadState": "not-found"}
        self.inspect = Mock(side_effect=self.properties)
        self.child = Mock()
        self.child.poll.return_value = None
        self.child.wait.side_effect = self.finish
        self.rmdir = os.rmdir
        self.removals = []
        self.gate_depth = 0
        self.gate = self.pool.startup_gate
        # common module自体は既存のもの。新しい終了監視の存在をmockしない。
        import jade_pdf2docx_slot_retirement
        for patcher in (patch.object(jade_pdf2docx_slot_retirement, "CGROUP_ROOT", self.base),
                        patch.object(self.pool, "unit_properties", self.inspect),
                        patch.object(self.pool.uuid, "uuid4", return_value=Mock(hex=self.token)),
                        patch.object(self.pool.subprocess, "Popen", side_effect=self.launch),
                        patch.object(self.pool, "startup_gate", side_effect=self.tracked_gate),
                        patch.object(os, "rmdir", side_effect=self.remove)):
            patcher.start()
            self.addCleanup(patcher.stop)

    @contextmanager
    def tracked_gate(self, *args):
        with self.gate(*args):
            self.gate_depth += 1
            try:
                yield
            finally:
                self.gate_depth -= 1

    def properties(self, manager, unit):
        if unit.endswith(".slice"):
            return {"LoadState": "loaded", "ControlGroup": "/jade-pdf2docxTest.slice"}
        return self.state

    def launch(self, *args, **kwargs):
        self.assertEqual(self.gate_depth, 1)
        self.group.mkdir()
        (self.group / "cgroup.events").write_text("populated 1\n")
        self.state = self.live
        return self.child

    def finish(self):
        self.assertEqual(self.gate_depth, 0, "conversion must not hold the admission gate")
        (self.group / "cgroup.events").write_text("populated 0\n")
        self.state = {"LoadState": "not-found"}
        return 0

    def remove(self, name, *, dir_fd=None):
        # TemporaryDirectory cleanupも標準rmdirへ渡す。
        if name == self.group.name and dir_fd is not None:
            self.assertEqual(self.gate_depth, 1)
            self.removals.append(name)
            group = os.open(name, os.O_RDONLY, dir_fd=dir_fd)
            try:
                os.unlink("cgroup.events", dir_fd=group)
            finally:
                os.close(group)
        return self.rmdir(name, dir_fd=dir_fd)

    def run_job(self):
        return self.pool.run_job(self.policy, "--user", self.base / "gate",
                                 ["/bin/true", "input.pdf", "output.docx"])

    def test_completed_job_returns_only_after_own_group_is_removed(self):
        self.assertEqual(self.run_job(), 0)
        self.assertFalse(self.group.exists(), "successful waiter left its original cgroup")
        self.assertEqual(self.removals, [self.group.name])
        self.child.wait.assert_called_once()

    def test_failed_conversion_keeps_its_original_exit_code_after_retirement(self):
        def failed():
            self.finish()
            return 23
        self.child.wait.side_effect = failed
        self.assertEqual(self.run_job(), 23)
        self.assertFalse(self.group.exists())

    def test_removed_original_does_not_inspect_or_remove_reused_name(self):
        def next_job():
            self.finish()
            (self.group / "cgroup.events").unlink()
            self.rmdir(self.group)
            self.group.mkdir()
            (self.group / "cgroup.events").write_text("populated 1\n")
            self.state = dict(self.live, Description="jade-pdf2docx-job-" + "b" * 32)
            self.inspect.reset_mock()
            return 0
        self.child.wait.side_effect = next_job
        self.assertEqual(self.run_job(), 0)
        self.inspect.assert_not_called()
        self.assertEqual(self.removals, [])
        self.assertEqual((self.group / "cgroup.events").read_text(), "populated 1\n")

    def test_permission_failure_does_not_report_success_or_retry_conversion(self):
        def denied(name, *, dir_fd=None):
            if name == self.group.name:
                raise PermissionError("no retirement permission")
            return self.rmdir(name, dir_fd=dir_fd)
        with patch.object(os, "rmdir", side_effect=denied):
            with self.assertRaises(PermissionError):
                self.run_job()
        self.child.wait.assert_called_once()
        self.assertTrue(self.group.exists())

    def test_fast_job_removed_during_observation_keeps_original_result(self):
        calls = 0
        def observed(manager, unit):
            nonlocal calls
            if not unit.endswith(".slice") and self.group.exists():
                calls += 1
                if calls == 2:
                    self.state = {"LoadState": "not-found"}
                    (self.group / "cgroup.events").unlink()
                    self.rmdir(self.group)
                    self.child.poll.return_value = 23
            return self.properties(manager, unit)
        self.inspect.side_effect = observed
        self.child.wait.side_effect = None
        self.child.wait.return_value = 23
        self.assertEqual(self.run_job(), 23)
        self.child.wait.assert_called_once()
        self.assertFalse(self.group.exists())

    def test_capture_identity_failure_still_waits_once_and_preserves_group(self):
        calls = 0
        def changed(manager, unit):
            nonlocal calls
            if not unit.endswith(".slice") and self.group.exists():
                calls += 1
                if calls == 2:
                    return dict(self.live, Description="jade-pdf2docx-job-" + "b" * 32)
            return self.properties(manager, unit)
        self.inspect.side_effect = changed
        with self.assertRaisesRegex(ValueError, "identity"):
            self.run_job()
        self.child.wait.assert_called_once()
        self.assertEqual(self.removals, [])
        self.assertTrue(self.group.exists())

    def test_live_or_invalid_group_is_not_removed_after_wait(self):
        for events in ("populated 1\n", "invalid\n"):
            with self.subTest(events=events):
                def finished():
                    self.assertEqual(self.gate_depth, 0)
                    (self.group / "cgroup.events").write_text(events)
                    self.state = {"LoadState": "not-found"}
                    return 0
                self.child.wait.side_effect = finished
                with self.assertRaisesRegex(ValueError, "populated|invalid"):
                    self.run_job()
                self.assertEqual(self.removals, [])
                (self.group / "cgroup.events").unlink()
                self.rmdir(self.group)

    def test_replaced_path_with_original_still_present_refuses_removal(self):
        moved = self.parent / "original"
        def replaced():
            self.finish()
            self.group.rename(moved)
            self.group.mkdir()
            (self.group / "cgroup.events").write_text("populated 1\n")
            return 0
        self.child.wait.side_effect = replaced
        with self.assertRaisesRegex(ValueError, "identity"):
            self.run_job()
        self.assertEqual(self.removals, [])
        self.assertEqual((moved / "cgroup.events").read_text(), "populated 0\n")
        self.assertEqual((self.group / "cgroup.events").read_text(), "populated 1\n")
