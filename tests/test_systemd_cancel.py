"""再利用されるservice名を、過去のtokenやpathnameで停止しない。"""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/cancel_jade_pdf2docx_pool.py"


class SystemdCancelTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("jade_pdf2docx_cancel", SCRIPT)
        self.cancel = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(SCRIPT.parent), *sys.path]):
            spec.loader.exec_module(self.cancel)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name)
        self.policy = {"schema": 1, "slice": "jade-pdf2docxTest.slice", "slots": 2,
                       "total_memory_mib": 1024, "job_memory_mib": 512}
        self.unit = "jade-pdf2docx-jade-pdf2docxTest-slot-1.service"
        self.token = "a" * 32
        self.group = self.base / "cgroup" / self.policy["slice"] / self.unit
        self.group.mkdir(parents=True)
        self.kill = self.group / "cgroup.kill"
        self.kill.write_bytes(b"")
        self.group.joinpath("cgroup.events").write_text("populated 0\nfrozen 0\n")
        self.properties = {"LoadState": "loaded", "Description": "jade-pdf2docx-job-" + self.token,
                           "ControlGroup": "/jade-pdf2docxTest.slice/" + self.unit}
        self.addCleanup(patch.stopall)
        patch.object(self.cancel, "CGROUP_ROOT", self.base / "cgroup").start()
        self.inspect = patch.object(self.cancel.pool, "unit_properties",
                                    return_value=self.properties).start()

    def run_cancel(self, token=None, unit=None):
        return self.cancel.cancel_job(self.policy, "--system", self.base / "gate",
                                      unit or self.unit, self.token if token is None else token)

    def test_matching_job_is_killed_with_the_shared_gate_held(self):
        original = self.cancel.kill_group
        def retire(group_fd, manager, service, token):
            self.assertEqual((manager, service, token), ("--system", self.unit, self.token))
            self.assertEqual(self.kill.read_bytes(), b"1\n")
            with patch.object(self.cancel.pool, "STARTUP_SECONDS", 0.01):
                with self.assertRaises(self.cancel.pool.PoolBusy):
                    with self.cancel.pool.startup_gate(self.base / "gate", self.policy):
                        self.fail("admission entered before original group retirement")
        def kill(group_fd):
            with patch.object(self.cancel.pool, "STARTUP_SECONDS", 0.01):
                with self.assertRaises(self.cancel.pool.PoolBusy):
                    with self.cancel.pool.startup_gate(self.base / "gate", self.policy):
                        self.fail("admission entered during cancellation")
            return original(group_fd)
        with patch.object(self.cancel, "kill_group", side_effect=kill), \
                patch.object(self.cancel, "retire_group", side_effect=retire) as retirement:
            self.assertEqual(self.run_cancel(), 0)
        retirement.assert_called_once()
        self.assertEqual(self.kill.read_bytes(), b"1\n")
        self.assertEqual(self.inspect.call_count, 2)
        self.inspect.assert_called_with("--system", self.unit[:-8])

    def test_retirement_failure_is_not_reported_as_cancelled(self):
        with patch.object(self.cancel, "retire_group", side_effect=TimeoutError("still retiring")), \
                patch("builtins.print") as output:
            with self.assertRaises(TimeoutError):
                self.run_cancel()
        output.assert_not_called()
        self.assertEqual(self.kill.read_bytes(), b"1\n")
        # 失敗で抜けてもgateを解放し、遅延した停止/削除プロセスを残さない。
        with self.cancel.pool.startup_gate(self.base / "gate", self.policy):
            pass

    def test_old_token_missing_service_and_policy_change_never_kill(self):
        for state, token in (("loaded", "b" * 32), ("not-found", self.token), ("masked", self.token)):
            with self.subTest(state=state, token=token):
                self.inspect.return_value = dict(self.properties, LoadState=state)
                with self.assertRaises(ValueError):
                    self.run_cancel(token)
                self.assertEqual(self.kill.read_bytes(), b"")
        self.inspect.return_value = self.properties
        self.policy["slots"] = 1
        with self.assertRaises(ValueError):
            self.run_cancel()
        self.assertEqual(self.kill.read_bytes(), b"")

    def test_unrelated_unit_and_malformed_token_reject_before_manager_access(self):
        for unit in ("other.service", "jade-pdf2docx-jade-pdf2docxTest-slot-0.service", "jade-pdf2docx-jade-pdf2docxTest-slot-3.service",
                     "jade-pdf2docx-jade-pdf2docxTest-slot-01.service", "../" + self.unit, self.unit + "\n"):
            with self.assertRaises(ValueError):
                self.run_cancel(unit=unit)
        for token in ("", "A" * 32, "a" * 31, "a" * 33, "a" * 32 + "\n"):
            with self.assertRaises(ValueError):
                self.run_cancel(token=token)
        self.inspect.assert_not_called()
        self.assertEqual(self.kill.read_bytes(), b"")

    def test_changed_identity_after_group_open_never_kills(self):
        for changed in (dict(self.properties, Description="jade-pdf2docx-job-" + "b" * 32),
                        dict(self.properties, ControlGroup="/different/" + self.unit),
                        dict(self.properties, LoadState="not-found")):
            self.inspect.side_effect = [self.properties, changed]
            with self.assertRaises(ValueError):
                self.run_cancel()
            self.assertEqual(self.kill.read_bytes(), b"")

    def test_group_must_be_directly_under_the_configured_slice(self):
        for group in ("", "/", "jade-pdf2docxTest.slice/" + self.unit,
                      "/other.slice/" + self.unit, "/jade-pdf2docxTest.slice/../" + self.unit,
                      "/jade-pdf2docxTest.slice//" + self.unit, "/jade-pdf2docxTest.slice/other.service"):
            self.inspect.return_value = dict(self.properties, ControlGroup=group)
            with self.assertRaises(ValueError):
                self.run_cancel()
            self.assertEqual(self.kill.read_bytes(), b"")

    def test_pinned_directory_never_reopens_reused_path(self):
        with self.cancel.pin_group(self.properties["ControlGroup"]) as original:
            retired = self.group.with_name("retired")
            self.group.rename(retired)
            self.group.mkdir()
            self.group.joinpath("cgroup.kill").write_bytes(b"new job")
            self.group.joinpath("cgroup.events").write_text("populated 1\n")
            self.cancel.kill_group(original)
        self.assertEqual(retired.joinpath("cgroup.kill").read_bytes(), b"1\n")
        self.assertEqual(self.kill.read_bytes(), b"new job")

    def test_symlinked_group_or_kill_file_cannot_redirect_stop(self):
        self.kill.unlink()
        target = self.base / "unrelated"
        target.write_bytes(b"keep")
        self.kill.symlink_to(target)
        with self.assertRaises(OSError):
            self.run_cancel()
        self.assertEqual(target.read_bytes(), b"keep")
        self.kill.unlink()
        self.group.rename(self.group.with_name("actual"))
        self.group.symlink_to(self.group.with_name("actual"), target_is_directory=True)
        with self.assertRaises(OSError):
            self.run_cancel()

    def test_live_members_after_deadline_are_not_reported_as_cancelled(self):
        self.group.joinpath("cgroup.events").write_text("populated 1\n")
        with self.cancel.pin_group(self.properties["ControlGroup"]) as pinned:
            with self.assertRaises(TimeoutError):
                self.cancel.kill_group(pinned, timeout=0.01)
        self.assertEqual(self.kill.read_bytes(), b"1\n")
