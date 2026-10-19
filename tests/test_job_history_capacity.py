"""履歴満杯時も既存jobを保ち、独立受付の競合と中断を検証する。"""
import importlib.util
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/jade_pdf2docx_job_history.py"


class HistoryCapacityTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("history_capacity", SCRIPT)
        self.history = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(SCRIPT.parent), *sys.path]):
            spec.loader.exec_module(self.history)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve() / "history"
        self.root.mkdir(mode=0o700)
        self.unit = "jade-pdf2docx-test-slot-1.service"

    def prepare(self, number):
        self.history.prepare(self.root, f"{number:032x}", "--system", self.unit)

    def finish(self, number):
        self.history.finish(self.root, f"{number:032x}", self.unit,
                            {"SERVICE_RESULT": "success", "EXIT_CODE": "exited", "EXIT_STATUS": "0"})

    def test_full_store_preserves_records_and_reserves_terminal_writes(self):
        with patch.object(self.history, "MAX_HISTORY_JOBS", 2):
            self.prepare(1)
            self.finish(1)
            self.prepare(2)
            before = {path.name: path.read_bytes() for path in self.root.iterdir()}
            with self.assertRaises(self.history.HistoryBusy):
                self.prepare(3)
            self.assertEqual(before, {path.name: path.read_bytes() for path in self.root.iterdir()})
            self.finish(2)
            for number in (1, 2):
                self.assertEqual(self.history.query(self.root, f"{number:032x}")["state"], "succeeded")

    def test_directory_opened_before_publication_sees_current_capacity(self):
        from jade_pdf2docx_history_capacity import check_capacity, directory_gate
        # Linux overlayfsではopen時点のdirectory cacheが残る場合がある。
        with self.history.store_directory(self.root) as fd:
            self.prepare(1)
            with directory_gate(fd, 1):
                with self.assertRaises(self.history.HistoryBusy):
                    check_capacity(fd, f"{2:032x}", 1)

    def test_orphan_pending_corrupt_and_foreign_entries_consume_capacity(self):
        for name in (".pending-" + "1" * 32 + ".request.json", "foreign", "2" * 32 + ".result.json"):
            with self.subTest(name=name), patch.object(self.history, "MAX_HISTORY_JOBS", 1):
                path = self.root / name
                path.write_bytes(b"corrupt")
                with self.assertRaises(self.history.HistoryBusy):
                    self.prepare(3)
                self.assertEqual(list(self.root.iterdir()), [path])
                path.unlink()

    def test_completion_crash_does_not_accumulate_temporary_files_on_retry(self):
        self.prepare(1)
        program = """
import os, sys
sys.path.insert(0, sys.argv[1])
import jade_pdf2docx_job_history as history
history.os.fsync = lambda fd: os.kill(os.getpid(), 9)
history.finish(sys.argv[2], '00000000000000000000000000000001', 'jade-pdf2docx-test-slot-1.service', {})
"""
        result = subprocess.run([sys.executable, "-c", program, str(SCRIPT.parent), str(self.root)], timeout=5)
        self.assertEqual(result.returncode, -signal.SIGKILL)
        before = {path.name: path.read_bytes() for path in self.root.iterdir()}
        for _ in range(3):
            with self.assertRaises(FileExistsError):
                self.finish(1)
            self.assertEqual(before, {path.name: path.read_bytes() for path in self.root.iterdir()})
        self.assertEqual(self.history.query(self.root, f"{1:032x}", lambda *args: {})["state"], "unknown")

    def test_independent_processes_cannot_overbook_same_store(self):
        program = """
import sys, time
sys.path.insert(0, sys.argv[1])
import jade_pdf2docx_job_history as history
history.MAX_HISTORY_JOBS = 2
original = history.write_record
def delayed(*args):
    time.sleep(0.05)
    return original(*args)
history.write_record = delayed
try:
    history.prepare(sys.argv[2], format(int(sys.argv[3]), '032x'), '--system', 'jade-pdf2docx-test-slot-1.service')
except history.HistoryBusy:
    sys.exit(75)
"""
        children = [subprocess.Popen([sys.executable, "-c", program, str(SCRIPT.parent), str(self.root), str(i)],
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE) for i in range(1, 9)]
        def cleanup():
            for child in children:
                if child.poll() is None:
                    child.kill()
                child.wait()
        self.addCleanup(cleanup)
        outcomes = []
        for child in children:
            _, error = child.communicate(timeout=10)
            self.assertIn(child.returncode, (0, 75), error.decode())
            outcomes.append(child.returncode)
        self.assertEqual(outcomes.count(0), 2)
        self.assertEqual(len(list(self.root.glob("*.request.json"))), 2)

    def test_lock_wait_is_bounded_and_dead_holder_releases_lock(self):
        program = """
import fcntl, os, sys, time
fd = os.open(sys.argv[1], os.O_RDONLY | os.O_DIRECTORY)
fcntl.flock(fd, fcntl.LOCK_EX)
print('locked', flush=True)
time.sleep(30)
"""
        child = subprocess.Popen([sys.executable, "-c", program, str(self.root)], stdout=subprocess.PIPE)
        def cleanup():
            if child.poll() is None:
                child.kill()
            child.wait()
            child.stdout.close()
        self.addCleanup(cleanup)
        self.assertEqual(child.stdout.readline(), b"locked\n")
        with patch.object(self.history, "HISTORY_LOCK_SECONDS", 0.05):
            with self.assertRaises(self.history.HistoryBusy):
                self.prepare(1)
        self.assertEqual(list(self.root.iterdir()), [])
        child.kill()
        child.wait(timeout=5)
        self.prepare(1)

    def test_oversize_generated_records_reject_before_creating_files(self):
        with self.assertRaises(ValueError):
            self.history.prepare(self.root, f"{1:032x}", "--system", "jade-pdf2docx-" + "x" * 5000 + ".service")
        self.assertEqual(list(self.root.iterdir()), [])

    def test_terminal_publication_uses_same_directory_lock_as_admission(self):
        import fcntl
        self.prepare(1)
        original = self.history.write_record
        def locked_write(*args):
            fd = os.open(self.root, os.O_RDONLY | os.O_DIRECTORY)
            try:
                with self.assertRaises(BlockingIOError):
                    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            finally:
                os.close(fd)
            return original(*args)
        with patch.object(self.history, "write_record", side_effect=locked_write):
            self.finish(1)
