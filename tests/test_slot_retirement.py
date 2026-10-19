"""受付は未追跡の空群だけを回収し、旧FDを新jobへ引き継がない。"""
import errno
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/jade_pdf2docx_slot_retirement.py"


class SlotRetirementTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("slot_retirement", SCRIPT)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name)
        self.parent = self.base / "user.slice" / "jade-pdf2docxTest.slice"
        self.parent.mkdir(parents=True)
        self.unit = "jade-pdf2docx-jade-pdf2docxTest-slot-1"
        self.group = self.parent / (self.unit + ".service")
        self.group.mkdir()
        self.events = self.group / "cgroup.events"
        self.events.write_text("populated 0\nfrozen 0\n")
        self.slice_properties = {"LoadState": "loaded", "ControlGroup": "/user.slice/jade-pdf2docxTest.slice"}
        self.inspect = Mock(side_effect=lambda manager, unit: self.slice_properties
                            if unit.endswith(".slice") else {"LoadState": "not-found"})
        patcher = patch.object(self.module, "CGROUP_ROOT", self.base)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.rmdir = os.rmdir

    def retire(self, timeout=0):
        self.module.retire_slot("--user", "jade-pdf2docxTest.slice", self.unit, self.inspect, timeout=timeout)

    def remove_virtual_group(self, name, *, dir_fd):
        self.events.unlink()
        self.rmdir(name, dir_fd=dir_fd)

    def test_empty_residual_is_removed_and_parent_path_can_be_nested(self):
        with patch.object(self.module.os, "rmdir", side_effect=self.remove_virtual_group):
            self.retire()
        self.assertFalse(self.group.exists())
        self.inspect.assert_any_call("--user", self.unit)

    def test_absent_group_needs_no_removal_or_slot_recheck(self):
        self.events.unlink()
        self.rmdir(self.group)
        with patch.object(self.module.os, "rmdir") as remove:
            self.retire()
        remove.assert_not_called()
        self.assertEqual(self.inspect.call_count, 1)

    def test_inactive_configured_slice_is_started_before_residual_inspection(self):
        self.inspect.side_effect = [
            {"LoadState": "loaded", "ControlGroup": ""}, self.slice_properties,
            {"LoadState": "not-found"}]
        with patch.object(self.module, "subprocess") as process, \
                patch.object(self.module.os, "rmdir", side_effect=self.remove_virtual_group):
            self.retire()
        process.run.assert_called_once_with(
            ["systemctl", "--user", "start", "--", "jade-pdf2docxTest.slice"],
            capture_output=True, text=True, timeout=5, check=True)
        self.assertFalse(self.group.exists())

    def test_failed_slice_start_or_missing_post_start_group_denies_admission(self):
        self.inspect.side_effect = None
        self.inspect.return_value = {"LoadState": "loaded", "ControlGroup": ""}
        for error in (TimeoutError("slice start timed out"), None):
            with self.subTest(error=error), patch.object(self.module.subprocess, "run", side_effect=error), \
                    patch.object(self.module.os, "rmdir") as remove:
                with self.assertRaises((TimeoutError, ValueError)):
                    self.retire()
                remove.assert_not_called()

    def test_populated_or_malformed_events_never_remove_or_kill(self):
        for events in ("populated 1\n", "", "populated 00\n", "populated 0\n" + "x" * 4096):
            with self.subTest(events=events[:30]):
                self.events.write_text(events)
                with patch.object(self.module.os, "rmdir") as remove:
                    with self.assertRaises(ValueError):
                        self.retire()
                remove.assert_not_called()
                self.assertTrue(self.group.exists())

    def test_loaded_slot_or_missing_slice_does_not_remove(self):
        for properties in ({"LoadState": "loaded"}, {"LoadState": "masked"}):
            with self.subTest(properties=properties):
                self.inspect.side_effect = lambda manager, unit: self.slice_properties if unit.endswith(".slice") else properties
                with patch.object(self.module.os, "rmdir") as remove:
                    with self.assertRaises(ValueError):
                        self.retire()
                remove.assert_not_called()
        self.slice_properties = {"LoadState": "not-found", "ControlGroup": ""}
        with self.assertRaises(ValueError):
            self.retire()

    def test_invalid_parent_paths_and_symlink_are_rejected(self):
        for path in ("/", "/other.slice", "/user.slice/../jade-pdf2docxTest.slice", "//jade-pdf2docxTest.slice"):
            with self.subTest(path=path):
                self.slice_properties["ControlGroup"] = path
                with self.assertRaises(ValueError):
                    self.retire()
        self.slice_properties["ControlGroup"] = "/user.slice/jade-pdf2docxTest.slice"
        self.parent.rename(self.parent.with_name("target"))
        self.parent.symlink_to("target", target_is_directory=True)
        with self.assertRaises(OSError):
            self.retire()

    def test_replaced_group_is_not_removed(self):
        def replace(manager, unit):
            if unit.endswith(".slice"):
                return self.slice_properties
            self.group.rename(self.parent / "original")
            self.group.mkdir()
            return {"LoadState": "not-found"}
        self.inspect.side_effect = replace
        with patch.object(self.module.os, "rmdir") as remove:
            with self.assertRaises(ValueError):
                self.retire()
        remove.assert_not_called()
        self.assertTrue(self.group.exists())

    def test_busy_and_permission_failures_are_not_success(self):
        for error in (OSError(errno.EBUSY, "busy"), OSError(errno.ENOTEMPTY, "children"),
                      PermissionError(errno.EACCES, "denied")):
            with self.subTest(error=error), patch.object(self.module.os, "rmdir", side_effect=error):
                with self.assertRaises(OSError):
                    self.retire()
                self.assertTrue(self.group.exists())

    def test_transient_busy_is_rechecked_and_removed_without_kill(self):
        attempts = []
        def remove(name, *, dir_fd):
            attempts.append(name)
            if len(attempts) == 1:
                raise OSError(errno.EBUSY, "retiring")
            self.remove_virtual_group(name, dir_fd=dir_fd)
        with patch.object(self.module.os, "rmdir", side_effect=remove):
            self.retire(timeout=0.1)
        self.assertEqual(len(attempts), 2)

    def test_manager_failure_is_not_treated_as_empty(self):
        self.inspect.side_effect = TimeoutError("manager unavailable")
        with patch.object(self.module.os, "rmdir") as remove:
            with self.assertRaises(TimeoutError):
                self.retire()
        remove.assert_not_called()
