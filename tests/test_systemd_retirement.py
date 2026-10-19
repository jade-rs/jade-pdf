"""managerが解放した元cgroupだけを、slot再利用前に回収する。"""
import errno
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import subprocess
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/cancel_jade_pdf2docx_pool.py"


class SystemdRetirementTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("jade_pdf2docx_retirement", SCRIPT)
        self.cancel = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(SCRIPT.parent), *sys.path]):
            spec.loader.exec_module(self.cancel)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name)
        self.service = "jade-pdf2docx-jade-pdf2docxTest-slot-1.service"
        self.token = "a" * 32
        self.group = self.base / self.service
        self.group.mkdir()
        self.group.joinpath("cgroup.events").write_text("populated 0\nfrozen 0\n")
        self.original = os.open(self.group, os.O_RDONLY | os.O_DIRECTORY)
        self.addCleanup(os.close, self.original)
        self.loaded = {"LoadState": "loaded", "Description": "jade-pdf2docx-job-" + self.token}
        self.missing = {"LoadState": "not-found"}
        self.inspect = patch.object(self.cancel.pool, "unit_properties", return_value=self.missing).start()
        self.addCleanup(patch.stopall)
        # 通常FS上のfixtureでも、cgroupfs同様に仮想eventsをrmdir時に消す。
        self.remove_directory = os.rmdir
        self.remove = patch.object(self.cancel.os, "rmdir", side_effect=self.remove_group).start()

    def remove_group(self, name, *, dir_fd):
        os.unlink("cgroup.events", dir_fd=self.original)
        self.remove_directory(name, dir_fd=dir_fd)

    def retire(self, timeout=0.1):
        self.cancel.retire_group(self.original, "--system", self.service, self.token, timeout=timeout)

    def test_empty_original_is_removed_only_after_manager_gc(self):
        self.inspect.side_effect = [self.loaded, self.missing]
        self.retire()
        self.assertFalse(self.group.exists())
        self.assertEqual(self.inspect.call_count, 2)

    def test_kernel_busy_removal_is_retried_without_reopening_group(self):
        attempts = []
        def busy_once(name, *, dir_fd):
            attempts.append(name)
            if len(attempts) == 1:
                raise OSError(errno.EBUSY, "kernel task still retiring")
            self.remove_group(name, dir_fd=dir_fd)
        with patch.object(self.cancel.os, "rmdir", side_effect=busy_once):
            self.retire()
        self.assertEqual(attempts, [self.service, self.service])
        self.assertFalse(self.group.exists())

    def test_busy_deadline_is_failure_and_leaves_original(self):
        with patch.object(self.cancel.os, "rmdir", side_effect=OSError(errno.EBUSY, "busy")):
            with self.assertRaises(TimeoutError):
                self.retire(timeout=0)
        self.assertTrue(self.group.exists())

    def test_manager_owned_or_replaced_job_is_never_removed(self):
        for properties in (self.loaded, dict(self.loaded, Description="jade-pdf2docx-job-" + "b" * 32),
                           {"LoadState": "masked"}):
            with self.subTest(properties=properties):
                self.inspect.return_value = properties
                with patch.object(self.cancel.os, "rmdir") as remove:
                    with self.assertRaises((TimeoutError, ValueError)):
                        self.retire(timeout=0)
                remove.assert_not_called()
                self.assertTrue(self.group.exists())

    def test_replaced_path_or_symlink_is_never_removed(self):
        retired = self.base / "retired"
        self.group.rename(retired)
        self.group.mkdir()
        with self.assertRaises(ValueError):
            self.retire()
        self.assertTrue(self.group.exists())
        self.remove_directory(self.group)
        self.group.symlink_to(retired, target_is_directory=True)
        with self.assertRaises(ValueError):
            self.retire()
        self.assertTrue(self.group.is_symlink())
        self.assertTrue(retired.is_dir())

    def test_already_removed_original_does_not_touch_same_name_new_group(self):
        self.group.joinpath("cgroup.events").unlink()
        self.remove_directory(self.group)
        self.group.mkdir()
        self.group.joinpath("cgroup.events").write_text("populated 1\n")
        self.retire()
        self.assertTrue(self.group.exists())
        self.inspect.assert_not_called()

    def test_manager_deletion_between_stat_and_rmdir_is_success(self):
        def manager_removes_first(name, *, dir_fd):
            self.remove_group(name, dir_fd=dir_fd)
            raise FileNotFoundError(errno.ENOENT, "manager already removed it")
        with patch.object(self.cancel.os, "rmdir", side_effect=manager_removes_first):
            self.retire()
        self.assertFalse(self.group.exists())

    def test_permission_failure_is_not_retried_or_reported_as_success(self):
        with patch.object(self.cancel.os, "rmdir", side_effect=PermissionError(errno.EACCES, "denied")) as remove:
            with self.assertRaises(PermissionError):
                self.retire()
        self.assertEqual(remove.call_count, 1)
        self.assertTrue(self.group.exists())

    def test_manager_timeout_does_not_remove_original(self):
        self.inspect.side_effect = subprocess.TimeoutExpired("systemctl", 2)
        with self.assertRaises(subprocess.TimeoutExpired):
            self.retire()
        self.remove.assert_not_called()
        self.assertTrue(self.group.exists())

    def test_missing_path_with_existing_original_is_not_retirement(self):
        retired = self.base / "retired"
        self.group.rename(retired)
        with self.assertRaises(ValueError):
            self.retire()
        self.remove.assert_not_called()
        self.assertTrue(retired.is_dir())
