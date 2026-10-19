"""要求キーの再送、未確定な起動、独立受付の競合を検証する。"""
from contextlib import nullcontext, redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/run_jade_pdf2docx_pool.py"
KEY = "a" * 32


class RequestKeyTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("keyed_pool", SCRIPT)
        self.pool = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(SCRIPT.parent), *sys.path]):
            spec.loader.exec_module(self.pool)
        # kernel群の回収は専用unit/実systemdテストで検証する。
        retirement = patch.object(self.pool, "retire_slot")
        retirement.start()
        self.addCleanup(retirement.stop)
        completion = patch.object(self.pool, "watch_slot", side_effect=lambda *_: nullcontext(Mock()))
        completion.start()
        self.addCleanup(completion.stop)
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.store = self.base / "history"
        self.store.mkdir(mode=0o700)
        self.policy = {"schema": 1, "slice": "jade-pdf2docxKey.slice", "slots": 1,
                       "total_memory_mib": 1024, "job_memory_mib": 512}
        self.command = ["/immutable/jade-pdf2docx", "/data/input.pdf", "/data/output.docx",
                        "--input-sha256", "b" * 64, "--strict"]

    def run_job(self, command=None, key=KEY, store=True):
        return self.pool.run_job(self.policy, "--user", self.base / "gate",
                                 self.command if command is None else command,
                                 self.store if store else None, request_key=key)

    def launched(self):
        child = Mock()
        child.poll.return_value = 0
        child.wait.return_value = 0
        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen", return_value=child) as launch, \
             redirect_stdout(io.StringIO()):
            self.assertEqual(self.run_job(), 0)
        launch.assert_called_once()
        return json.loads((self.store / (KEY + ".request.json")).read_text())

    def test_keyed_launch_adds_receipt_without_changing_request_fingerprint(self):
        child = Mock()
        child.poll.return_value = child.wait.return_value = 0
        original = list(self.command)
        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen", return_value=child) as launch, \
             redirect_stdout(io.StringIO()):
            self.assertEqual(self.run_job(), 0)
        digest = self.pool.identity.fingerprint(self.policy, "--user", original)
        self.assertEqual(launch.call_args.args[0][-2:], ["--publication-receipt", KEY + ":" + digest])
        self.assertEqual(self.command, original)
        stored = json.loads((self.store / (KEY + ".request.json")).read_text())
        self.assertEqual(stored["request_sha256"], digest)

    def test_keyed_request_cannot_supply_a_different_publication_receipt(self):
        with self.assertRaises(ValueError), patch.object(self.pool.subprocess, "Popen") as launch:
            self.run_job(self.command + ["--publication-receipt", KEY + ":" + "c"*64])
        launch.assert_not_called()
        self.assertEqual(list(self.store.iterdir()), [])

    def test_replay_after_restart_and_gc_returns_original_completed_result(self):
        request = self.launched()
        self.pool.history.finish(self.store, KEY, request["service"],
                                 {"SERVICE_RESULT": "success", "EXIT_CODE": "exited", "EXIT_STATUS": "0"})
        before = {p.name: p.read_bytes() for p in self.store.iterdir()}
        output = io.StringIO()
        with patch.object(self.pool, "unit_properties", side_effect=AssertionError("no slot lookup")), \
             patch.object(self.pool.subprocess, "Popen") as launch, redirect_stdout(output), \
             patch.object(self.pool.history, "MAX_HISTORY_JOBS", 1):
            self.assertEqual(self.run_job(), 0)
        launch.assert_not_called()
        replay = json.loads(output.getvalue())
        self.assertEqual((replay["token"], replay["state"], replay["replayed"]), (KEY, "succeeded", True))
        self.assertEqual({p.name: p.read_bytes() for p in self.store.iterdir()}, before)

    def test_unknown_and_failed_requests_are_never_restarted(self):
        request = self.launched()
        output = io.StringIO()
        with patch.object(self.pool.subprocess, "Popen") as launch, \
             patch.object(self.pool.history, "inspect_service", return_value={"LoadState": "not-found"}), \
             redirect_stdout(output):
            self.assertEqual(self.run_job(), 75)
        launch.assert_not_called()
        self.assertEqual(json.loads(output.getvalue())["state"], "unknown")
        self.pool.history.finish(self.store, KEY, request["service"],
                                 {"SERVICE_RESULT": "exit-code", "EXIT_CODE": "exited", "EXIT_STATUS": "7"})
        with patch.object(self.pool.subprocess, "Popen") as launch, redirect_stdout(io.StringIO()):
            self.assertEqual(self.run_job(), 1)
        launch.assert_not_called()

    def test_changed_request_is_conflict_before_manager_access(self):
        self.launched()
        changes = [self.command[:2] + ["/other.docx"] + self.command[3:],
                   self.command[:4] + ["c" * 64] + self.command[5:],
                   self.command[:-1], ["/new/jade-pdf2docx", *self.command[1:]]]
        with patch.object(self.pool, "unit_properties", side_effect=AssertionError("no slot lookup")), \
             patch.object(self.pool.subprocess, "Popen") as launch:
            for command in changes:
                with self.subTest(command=command), self.assertRaisesRegex(ValueError, "conflict"):
                    self.run_job(command)
            self.policy["job_memory_mib"] = 256
            with self.assertRaisesRegex(ValueError, "conflict"):
                self.run_job()
        launch.assert_not_called()

    def test_invalid_or_unbound_requests_have_no_side_effects(self):
        invalid = [self.command[:3], [self.command[0], "relative.pdf", *self.command[2:]],
                   self.command[:4] + ["B" * 64], self.command + ["--input-sha256", "b" * 64]]
        with patch.object(self.pool, "unit_properties") as inspect, \
             patch.object(self.pool.subprocess, "Popen") as launch:
            for key in ("", "../key", "A" * 32, "a" * 31, "a" * 33, "a" * 32 + "\n"):
                with self.subTest(key=key), self.assertRaises(ValueError):
                    self.run_job(key=key)
            for command in invalid:
                with self.subTest(command=command), self.assertRaises(ValueError):
                    self.run_job(command)
            with self.assertRaises(ValueError):
                self.run_job(store=False)
        inspect.assert_not_called()
        launch.assert_not_called()
        self.assertEqual(list(self.store.iterdir()), [])

    def test_launch_failure_claim_is_preserved_and_replay_does_not_retry(self):
        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen", side_effect=OSError("exec failed")), \
             redirect_stdout(io.StringIO()), self.assertRaises(OSError):
            self.run_job()
        self.assertTrue((self.store / (KEY + ".request.json")).exists())
        with patch.object(self.pool.history, "inspect_service", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen") as launch, redirect_stdout(io.StringIO()):
            self.assertEqual(self.run_job(), 75)
        launch.assert_not_called()

    def test_interrupted_pending_record_and_legacy_token_never_admit(self):
        pending = self.store / (".pending-" + KEY + ".request.json")
        pending.write_text("partial")
        pending.chmod(0o600)
        with patch.object(self.pool.subprocess, "Popen") as launch, \
             patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}):
            with self.assertRaises((OSError, ValueError)):
                self.run_job()
        launch.assert_not_called()
        self.assertEqual(pending.read_text(), "partial")
        pending.unlink()
        self.pool.history.prepare(self.store, KEY, "--user", "jade-pdf2docx-legacy.service")
        with patch.object(self.pool.subprocess, "Popen") as launch, self.assertRaisesRegex(ValueError, "conflict"):
            self.run_job()
        launch.assert_not_called()

    def test_independent_dispatchers_launch_same_key_only_once(self):
        program = '''import json,os,pathlib,sys,time
sys.path.insert(0,sys.argv[1]); import run_jade_pdf2docx_pool as pool
pool.history.inspect_service=lambda *_: {"LoadState":"not-found"}
pool.unit_properties=lambda *_: {"LoadState":"not-found"}
pool.retire_slot=lambda *_: None
from contextlib import nullcontext
from unittest.mock import Mock
pool.watch_slot=lambda *_: nullcontext(Mock())
class Child:
 def __init__(self,*args,**kwargs):
  fd=os.open(sys.argv[6],os.O_WRONLY|os.O_CREAT|os.O_APPEND,0o600)
  os.write(fd,b"launch\\n"); os.close(fd); time.sleep(.05)
 def poll(self): return 0
 def wait(self): return 0
pool.subprocess.Popen=Child
sys.exit(pool.run_job(json.loads(sys.argv[2]),"--user",pathlib.Path(sys.argv[3]),json.loads(sys.argv[4]),pathlib.Path(sys.argv[5]),request_key=sys.argv[7]))
'''
        log = self.base / "launches"
        processes = [subprocess.Popen([sys.executable, "-c", program, str(SCRIPT.parent),
                                      json.dumps(self.policy), str(self.base / f"gate-{i}"),
                                      json.dumps(self.command), str(self.store), str(log), KEY],
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE) for i in range(8)]
        results = []
        try:
            for process in processes:
                stdout, stderr = process.communicate(timeout=15)
                self.assertIn(process.returncode, (0, 75), (stdout, stderr))
                results.append(process.returncode)
            self.assertEqual(results.count(0), 1)
            self.assertEqual(log.read_text().splitlines(), ["launch"])
            self.assertEqual(len(list(self.store.iterdir())), 1)
        finally:
            for process in processes:
                if process.poll() is None:
                    process.kill()
                process.communicate(timeout=5)

    def test_sigkill_after_durable_claim_never_causes_automatic_relaunch(self):
        program = '''import json,os,pathlib,signal,sys
sys.path.insert(0,sys.argv[1]); import run_jade_pdf2docx_pool as pool
pool.unit_properties=lambda *_: {"LoadState":"not-found"}
pool.retire_slot=lambda *_: None
from contextlib import nullcontext
from unittest.mock import Mock
pool.watch_slot=lambda *_: nullcontext(Mock())
pool.subprocess.Popen=lambda *args,**kwargs: os.kill(os.getpid(),signal.SIGKILL)
pool.run_job(json.loads(sys.argv[2]),"--user",pathlib.Path(sys.argv[3]),json.loads(sys.argv[4]),pathlib.Path(sys.argv[5]),request_key=sys.argv[6])
'''
        result = subprocess.run([sys.executable, "-c", program, str(SCRIPT.parent), json.dumps(self.policy),
                                 str(self.base / "gate"), json.dumps(self.command), str(self.store), KEY],
                                capture_output=True, timeout=10)
        self.assertEqual(result.returncode, -9)
        self.assertTrue((self.store / (KEY + ".request.json")).exists())
        with patch.object(self.pool.history, "inspect_service", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen") as launch, redirect_stdout(io.StringIO()):
            self.assertEqual(self.run_job(), 75)
        launch.assert_not_called()

    def test_durable_write_failure_never_launches_and_preserves_uncertain_claim(self):
        original = self.pool.history.write_record

        def uncertain(fd, name, record):
            original(fd, name, record)
            raise OSError("directory synchronization failed")

        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.history, "write_record", side_effect=uncertain), \
             patch.object(self.pool.subprocess, "Popen") as launch, self.assertRaises(OSError):
            self.run_job()
        launch.assert_not_called()
        with patch.object(self.pool.history, "inspect_service", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen") as launch, redirect_stdout(io.StringIO()):
            self.assertEqual(self.run_job(), 75)
        launch.assert_not_called()

    def test_corrupt_keyed_identity_is_never_reused_or_completed(self):
        request = self.launched()
        path = self.store / (KEY + ".request.json")
        invalid = [dict(request, schema=True), dict(request, schema=3),
                   dict(request, request_sha256="B" * 64), dict(request, request_sha256=None),
                   dict(request, extra=1), {k: v for k, v in request.items() if k != "request_sha256"}]
        for record in invalid:
            with self.subTest(record=record):
                path.write_text(json.dumps(record))
                with patch.object(self.pool.subprocess, "Popen") as launch, self.assertRaises(ValueError):
                    self.run_job()
                launch.assert_not_called()
                with self.assertRaises(ValueError):
                    self.pool.history.finish(self.store, KEY, request["service"], {})
        self.assertFalse((self.store / (KEY + ".result.json")).exists())

    def test_cli_passes_request_key_outside_conversion_arguments(self):
        arguments = [str(SCRIPT), "--config", "/policy.json", "--history-dir", str(self.store),
                     "--request-key", KEY, *self.command]
        with patch.object(sys, "argv", arguments), patch.object(sys, "platform", "linux"), \
             patch.object(self.pool, "load_policy", return_value=self.policy), \
             patch.object(self.pool, "run_job", return_value=75) as run:
            self.assertEqual(self.pool.main(), 75)
        self.assertEqual(run.call_args.args[3:], (self.command, self.store, KEY))
