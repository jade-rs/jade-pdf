"""永続待機から一度だけ起動する境界と、停止後の再送を検証する。"""
from contextlib import nullcontext, redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
KEY = "d" * 32


class DurableQueueTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("durable_queue", SCRIPTS / "jade_pdf2docx_durable_queue.py")
        self.queue = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(SCRIPTS), *sys.path]):
            spec.loader.exec_module(self.queue)
        self.pool = self.queue.pool
        # kernel群の回収は専用unit/実systemdテストで検証する。
        retirement = patch.object(self.pool, "retire_slot")
        retirement.start()
        self.addCleanup(retirement.stop)
        completion = patch.object(self.pool, "watch_slot", side_effect=lambda *_: nullcontext(Mock()))
        completion.start()
        self.addCleanup(completion.stop)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name).resolve()
        self.store = self.base / "jobs"
        self.store.mkdir(mode=0o700)
        self.policy = dict(schema=1, slice="jade-pdf2docxQueue.slice", slots=1,
                           total_memory_mib=1024, job_memory_mib=512)
        self.command = ["/immutable/jade-pdf2docx", "/data/input.pdf", "/data/output.docx",
                        "--input-sha256", "a" * 64, "--strict"]

    def enqueue(self, key=KEY):
        return self.queue.enqueue(self.store, key, self.policy, "--user", self.command)

    def snapshot(self):
        return {p.name: p.read_bytes() for p in self.store.iterdir()}

    def test_enqueue_is_durable_idempotent_and_never_starts_a_process(self):
        with patch.object(subprocess, "Popen", side_effect=AssertionError("no launch")):
            first = self.enqueue()
            before = self.snapshot()
            second = self.enqueue()
            status = self.queue.status(self.store, KEY)
        self.assertEqual((first["state"], first["replayed"]), ("queued", False))
        self.assertEqual((second["state"], second["replayed"]), ("queued", True))
        self.assertEqual(status["state"], "queued")
        self.assertEqual(self.snapshot(), before)
        record = json.loads(next(self.store.iterdir()).read_text())
        self.assertEqual(record["command"], self.command)
        self.assertEqual(record["cwd"], os.getcwd())

    def test_busy_pool_retains_queued_job_for_later_dispatch(self):
        self.enqueue()
        before = self.snapshot()
        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "loaded"}), \
             patch.object(self.pool.subprocess, "Popen") as launch:
            self.assertEqual(self.queue.dispatch(self.store, KEY, self.base / "gate"), 75)
        launch.assert_not_called()
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(self.queue.status(self.store, KEY)["state"], "queued")

    def test_dispatch_restores_cwd_and_syncs_request_before_launch_then_replays(self):
        original = os.getcwd()
        os.chdir(self.base)
        try:
            self.enqueue()
        finally:
            os.chdir(original)
        child = Mock()
        child.poll.return_value = 0
        child.wait.return_value = 0
        def start(*args, **kwargs):
            self.assertEqual(os.getcwd(), str(self.base))
            self.assertTrue((self.store / (KEY + ".request.json")).exists())
            return child
        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen", side_effect=start) as launch, \
             redirect_stdout(io.StringIO()):
            self.assertEqual(self.queue.dispatch(self.store, KEY, self.base / "gate"), 0)
        launch.assert_called_once()
        self.assertEqual(os.getcwd(), original)
        self.pool.history.finish(self.store, KEY, "jade-pdf2docx-jade-pdf2docxQueue-slot-1.service",
                                 dict(SERVICE_RESULT="success", EXIT_CODE="exited", EXIT_STATUS="0"))
        with patch.object(self.pool.subprocess, "Popen") as launch, redirect_stdout(io.StringIO()):
            self.assertEqual(self.queue.dispatch(self.store, KEY, self.base / "gate"), 0)
        launch.assert_not_called()
        self.assertEqual(self.queue.status(self.store, KEY)["state"], "succeeded")

    def test_synchronous_admission_cannot_bypass_queued_ownership(self):
        self.enqueue()
        with patch.object(self.pool.subprocess, "Popen") as launch, self.assertRaises(ValueError):
            self.pool.run_job(self.policy, "--user", self.base / "gate", self.command, self.store, KEY)
        launch.assert_not_called()
        self.assertEqual(self.queue.status(self.store, KEY)["state"], "queued")

    def test_queue_and_launched_records_share_one_capacity_slot(self):
        self.enqueue()
        with patch.object(self.pool.history, "MAX_HISTORY_JOBS", 1):
            self.assertTrue(self.enqueue()["replayed"])
            with self.assertRaises(self.pool.history.HistoryBusy):
                self.enqueue("e" * 32)
            child = Mock()
            child.poll.return_value = 0
            child.wait.return_value = 0
            with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
                 patch.object(self.pool.subprocess, "Popen", return_value=child), redirect_stdout(io.StringIO()):
                self.assertEqual(self.queue.dispatch(self.store, KEY, self.base / "gate"), 0)

    def test_conflicting_partial_unsafe_and_oversize_queue_never_launch(self):
        self.enqueue()
        before = self.snapshot()
        self.command[2] = "/different.docx"
        with self.assertRaises(ValueError):
            self.enqueue()
        self.assertEqual(self.snapshot(), before)
        path = self.store / (KEY + ".queue.json")
        original = path.read_bytes()
        for data in (b"partial", original.replace(b'"schema": 1', b'"schema": true'),
                     original.replace(b'"cwd":', b'"extra": 1, "cwd":'), b"x" * 4097):
            path.write_bytes(data)
            with patch.object(self.pool.subprocess, "Popen") as launch, self.assertRaises((OSError, ValueError)):
                self.queue.dispatch(self.store, KEY, self.base / "gate")
            launch.assert_not_called()
            self.assertEqual(path.read_bytes(), data)
        path.write_bytes(original)
        path.chmod(0o644)
        with self.assertRaises(ValueError):
            self.queue.status(self.store, KEY)
        path.unlink()
        self.command[-1] = "x" * 5000
        with self.assertRaises(ValueError):
            self.enqueue()
        self.assertEqual(self.snapshot(), {})

    def test_actual_sigkill_after_enqueue_leaves_dispatchable_job(self):
        program = '''import os,sys,json
sys.path.insert(0,sys.argv[1]); import jade_pdf2docx_durable_queue as q
original=q.pool.history.write_record
def write(*args):
 original(*args);os.kill(os.getpid(),9)
q.pool.history.write_record=write
q.enqueue(sys.argv[2],sys.argv[3],json.loads(sys.argv[4]),'--user',json.loads(sys.argv[5]))
'''
        result = subprocess.run([sys.executable, "-c", program, str(SCRIPTS), str(self.store), KEY,
                                 json.dumps(self.policy), json.dumps(self.command)], timeout=10)
        self.assertEqual(result.returncode, -signal.SIGKILL)
        self.assertEqual(self.queue.status(self.store, KEY)["state"], "queued")
        child = Mock()
        child.poll.return_value = 0
        child.wait.return_value = 0
        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen", return_value=child) as launch, redirect_stdout(io.StringIO()):
            self.assertEqual(self.queue.dispatch(self.store, KEY, self.base / "gate"), 0)
        launch.assert_called_once()

    def test_actual_sigkill_after_launch_claim_never_relaunches(self):
        self.enqueue()
        program = '''import os,sys
sys.path.insert(0,sys.argv[1]); import jade_pdf2docx_durable_queue as q
q.pool.unit_properties=lambda *_:{'LoadState':'not-found'}
q.pool.retire_slot=lambda *_:None
from contextlib import nullcontext
from unittest.mock import Mock
q.pool.watch_slot=lambda *_:nullcontext(Mock())
original=q.pool.history.write_record
def write(*args):
 original(*args);os.kill(os.getpid(),9)
q.pool.history.write_record=write
q.dispatch(sys.argv[2],sys.argv[3],sys.argv[4])
'''
        result = subprocess.run([sys.executable, "-c", program, str(SCRIPTS), str(self.store), KEY,
                                 str(self.base / "gate")], timeout=10, capture_output=True)
        self.assertEqual(result.returncode, -signal.SIGKILL)
        with patch.object(self.pool.history, "inspect_service", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen") as launch, redirect_stdout(io.StringIO()):
            self.assertEqual(self.queue.dispatch(self.store, KEY, self.base / "gate"), 75)
            self.assertEqual(self.queue.status(self.store, KEY)["state"], "unknown")
        launch.assert_not_called()

    def test_completed_dispatch_replays_even_when_original_cwd_is_gone(self):
        working = self.base / "working"
        working.mkdir()
        original = os.getcwd()
        os.chdir(working)
        try:
            self.enqueue()
        finally:
            os.chdir(original)
        child = Mock()
        child.poll.return_value = 0
        child.wait.return_value = 0
        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen", return_value=child), redirect_stdout(io.StringIO()):
            self.assertEqual(self.queue.dispatch(self.store, KEY, self.base / "gate"), 0)
        self.pool.history.finish(self.store, KEY, "jade-pdf2docx-jade-pdf2docxQueue-slot-1.service",
                                 dict(SERVICE_RESULT="success", EXIT_CODE="exited", EXIT_STATUS="0"))
        working.rmdir()
        with patch.object(self.pool.subprocess, "Popen") as launch, redirect_stdout(io.StringIO()):
            self.assertEqual(self.queue.dispatch(self.store, KEY, self.base / "gate"), 0)
        launch.assert_not_called()

    def test_system_dispatch_requires_administrator_before_claiming(self):
        self.queue.enqueue(self.store, KEY, self.policy, "--system", self.command)
        before = self.snapshot()
        with patch.object(self.pool.os, "geteuid", side_effect=[os.geteuid(), os.geteuid(), 1001]), \
             patch.object(self.pool.subprocess, "Popen") as launch, \
             self.assertRaisesRegex(ValueError, "administrator"):
            self.queue.dispatch(self.store, KEY, self.base / "gate")
        launch.assert_not_called()
        self.assertEqual(self.snapshot(), before)

    def test_actual_sigkill_before_queue_publication_can_be_recovered_without_launch(self):
        program = """import os,sys,json
sys.path.insert(0,sys.argv[1]); import jade_pdf2docx_durable_queue as q
q.pool.history.os.link=lambda *args,**kwargs:os.kill(os.getpid(),9)
q.enqueue(sys.argv[2],sys.argv[3],json.loads(sys.argv[4]),'--user',json.loads(sys.argv[5]))
"""
        result = subprocess.run([sys.executable, "-c", program, str(SCRIPTS), str(self.store), KEY,
                                 json.dumps(self.policy), json.dumps(self.command)], timeout=10)
        self.assertEqual(result.returncode, -signal.SIGKILL)
        with self.assertRaises((OSError, ValueError)):
            self.enqueue()
        recovered = subprocess.run([sys.executable, str(SCRIPTS / "recover_jade_pdf2docx_history.py"),
                                    str(self.store), KEY], capture_output=True, timeout=10)
        self.assertEqual(recovered.returncode, 0, recovered.stderr)
        self.assertEqual(json.loads(recovered.stdout)["state"], "queued")
        self.assertEqual(json.loads(recovered.stdout)["repaired"], ["queue"])
        self.assertEqual(self.queue.status(self.store, KEY)["state"], "queued")
        self.assertFalse((self.store / (KEY + ".request.json")).exists())

    def test_cli_enqueue_and_status_and_null_cwd_validation(self):
        config = self.base / "policy.json"
        config.write_text(json.dumps(self.policy))
        config.chmod(0o600)
        result = subprocess.run([sys.executable, str(SCRIPTS / "jade_pdf2docx_durable_queue.py"), "enqueue",
                                 "--config", str(config), str(self.store), KEY, *self.command],
                                capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["state"], "queued")
        path = self.store / (KEY + ".queue.json")
        record = json.loads(path.read_text())
        record["cwd"] = None
        path.write_text(json.dumps(record))
        result = subprocess.run([sys.executable, str(SCRIPTS / "jade_pdf2docx_durable_queue.py"), "status",
                                 str(self.store), KEY], capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn(b"durable queue failed:", result.stderr)

    def test_independent_dispatchers_launch_only_once(self):
        self.enqueue()
        log = self.base / "launches"
        program = '''import os,sys,time
sys.path.insert(0,sys.argv[1]); import jade_pdf2docx_durable_queue as q
q.pool.unit_properties=lambda *_:{'LoadState':'not-found'}
q.pool.retire_slot=lambda *_:None
from contextlib import nullcontext
from unittest.mock import Mock
q.pool.watch_slot=lambda *_:nullcontext(Mock())
q.pool.history.inspect_service=lambda *_:{'LoadState':'not-found'}
class Child:
 def __init__(self,*args,**kwargs):
  fd=os.open(sys.argv[5],os.O_WRONLY|os.O_CREAT|os.O_APPEND,0o600)
  os.write(fd,b'launch\\n');os.close(fd);time.sleep(.05)
 def poll(self): return 0
 def wait(self): return 0
q.pool.subprocess.Popen=Child
sys.exit(q.dispatch(sys.argv[2],sys.argv[3],sys.argv[4]))
'''
        children = [subprocess.Popen([sys.executable, "-c", program, str(SCRIPTS), str(self.store), KEY,
                                      str(self.base / "gate"), str(log)], stdout=subprocess.PIPE,
                                      stderr=subprocess.PIPE) for _ in range(8)]
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
        self.assertEqual(outcomes.count(0), 1)
        self.assertEqual(log.read_bytes(), b"launch\n")
