"""同時実行枠・policy固定・受付失敗時の副作用を検証する。"""
import importlib.util
from contextlib import nullcontext, redirect_stderr
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


class SystemdAdmissionTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("jade_pdf2docx_pool", SCRIPT)
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
        self.base = Path(self.temporary.name)
        self.policy = {"schema": 1, "slice": "jade-pdf2docxTest.slice", "slots": 2,
                       "total_memory_mib": 1024, "job_memory_mib": 512}
        self.config = self.base / "pool.json"
        self.config.write_text(json.dumps(self.policy))
        self.command = ["/absolute/my jade-pdf2docx", "a $HOME.pdf", "b.docx", "--strict"]

    def test_history_is_durable_before_launch_and_passed_to_stop_hook(self):
        history = self.base.resolve() / "history"
        history.mkdir(mode=0o700)
        child = Mock()
        child.poll.return_value = 0
        child.wait.return_value = 0
        def launch(arguments, **kwargs):
            requests = list(history.glob("*.request.json"))
            self.assertEqual(len(requests), 1)
            record = json.loads(requests[0].read_text())
            self.assertEqual(record["service"], "jade-pdf2docx-jade-pdf2docxTest-slot-1.service")
            self.assertIn("--history-dir", arguments)
            self.assertIn(str(history), arguments)
            return child
        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen", side_effect=launch) as process:
            self.assertEqual(self.pool.run_job(self.policy, "--system", self.base / "gate",
                                               self.command, history), 0)
            self.assertEqual(process.call_count, 1)

    def test_residual_retirement_holds_gate_and_precedes_claim_or_launch(self):
        store = self.base.resolve() / "history"
        store.mkdir(mode=0o700)
        child = Mock()
        child.poll.return_value = 0
        child.wait.return_value = 0
        def retire(manager, slice_name, unit, inspect):
            self.assertEqual((manager, slice_name, unit),
                             ("--system", self.policy["slice"], "jade-pdf2docx-jade-pdf2docxTest-slot-1"))
            self.assertFalse(list(store.iterdir()))
            with patch.object(self.pool, "STARTUP_SECONDS", 0.01):
                with self.assertRaises(self.pool.PoolBusy):
                    with self.pool.startup_gate(self.base / "gate", self.policy):
                        self.fail("admission gate released during retirement")
            raise TimeoutError("residual remains")
        with patch.object(self.pool, "retire_slot", side_effect=retire) as retirement, \
                patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
                patch.object(self.pool.subprocess, "Popen", return_value=child) as launch:
            for key in (None, "a" * 32):
                with self.subTest(request_key=key), self.assertRaises(TimeoutError):
                    command = ["/immutable/jade-pdf2docx", "/data/input.pdf", "/data/output.docx",
                               "--input-sha256", "b" * 64, "--strict"]
                    self.pool.run_job(self.policy, "--system", self.base / "gate", command, store,
                                      request_key=key)
        self.assertEqual(retirement.call_count, 2)
        launch.assert_not_called()
        self.assertFalse(list(store.iterdir()))

    def test_manager_queries_keep_slice_suffix_and_qualify_slot_service(self):
        result = subprocess.CompletedProcess([], 0, "LoadState=loaded\nControlGroup=/jade-pdf2docxTest.slice\n", "")
        with patch.object(self.pool.subprocess, "run", return_value=result) as query:
            for name, expected in (("jade-pdf2docxTest.slice", "jade-pdf2docxTest.slice"),
                                   ("jade-pdf2docx-jade-pdf2docxTest-slot-1", "jade-pdf2docx-jade-pdf2docxTest-slot-1.service")):
                self.pool.unit_properties("--system", name)
                self.assertEqual(query.call_args.args[0][3], expected)

    def test_history_sync_failure_never_starts_or_retries_conversion(self):
        history = self.base.resolve() / "history"
        history.mkdir(mode=0o700)
        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.history, "prepare", side_effect=OSError("history disk full")), \
             patch.object(self.pool.subprocess, "Popen") as process:
            with self.assertRaises(OSError):
                self.pool.run_job(self.policy, "--system", self.base / "gate", self.command, history)
            process.assert_not_called()

    def test_history_full_returns_75_without_launch_or_issuing_new_token(self):
        history = self.base.resolve() / "history"
        history.mkdir(mode=0o700)
        self.pool.history.prepare(history, "a" * 32, "--user", "jade-pdf2docx-jade-pdf2docxTest-slot-1.service")
        gate = self.pool.startup_gate
        diagnostic = io.StringIO()
        with patch.object(self.pool.history, "MAX_HISTORY_JOBS", 1), \
             patch.object(self.pool, "startup_gate", side_effect=lambda *_: gate(self.base / "gate", self.policy)), \
             patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen") as process, \
             patch.object(sys, "platform", "linux"), \
             patch.object(sys, "argv", [str(SCRIPT), "--config", str(self.config),
                                        "--history-dir", str(history), *self.command]), \
             redirect_stderr(diagnostic):
            self.assertEqual(self.pool.main(), 75)
        process.assert_not_called()
        self.assertIn("history full", diagnostic.getvalue())
        self.assertNotIn("token:", diagnostic.getvalue())
        self.assertEqual(len(list(history.iterdir())), 1)

    def test_policy_is_bounded_and_rejects_ambiguous_or_unsafe_input(self):
        self.assertEqual(self.pool.load_policy(self.config), self.policy)
        invalid = [dict(self.policy, slots=value) for value in (0, 65, True, "2")]
        invalid += [dict(self.policy, slice=name) for name in
                    ("other.slice", "jade-pdf2docx-b.slice", "jade-pdf2docx.slice\n", "jade-pdf2docx" + "a" * 120 + ".slice")]
        for key in ("total_memory_mib", "job_memory_mib"):
            invalid += [dict(self.policy, **{key: value}) for value in
                        (0, -1, True, 8796093022208, 1.5)]
        invalid += [dict(self.policy, schema=True), dict(self.policy, unknown=1), {}]
        for value in invalid:
            with self.subTest(value=value):
                self.config.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    self.pool.load_policy(self.config)
        for raw in ('{"schema":1,"schema":1}', " " * 4097, "[]"):
            self.config.write_text(raw)
            with self.assertRaises(ValueError):
                self.pool.load_policy(self.config)
        self.config.write_text(json.dumps(self.policy))
        self.config.chmod(0o666)
        with self.assertRaises(ValueError):
            self.pool.load_policy(self.config)
        self.config.chmod(0o600)
        link = self.base / "link.json"
        link.symlink_to(self.config)
        with self.assertRaises((ValueError, OSError)):
            self.pool.load_policy(link)

    def test_loaded_services_including_inactive_or_deactivating_fill_slots(self):
        for state in ("loaded", "error", "masked"):
            with self.subTest(state=state), \
                 patch.object(self.pool, "unit_properties", return_value={"LoadState": state}), \
                 patch.object(self.pool.subprocess, "Popen") as launch:
                self.assertEqual(self.pool.run_job(self.policy, "--user", self.base, self.command), 75)
                launch.assert_not_called()

    def test_free_second_slot_preserves_arguments_and_failure_without_retry(self):
        child = Mock()
        child.poll.return_value = 23  # 極短時間で終了したjobも再実行しない。
        child.wait.return_value = 23
        with patch.object(self.pool, "unit_properties", side_effect=[
                {"LoadState": "loaded"}, {"LoadState": "not-found"}]), \
             patch.object(self.pool.subprocess, "Popen", return_value=child) as launch:
            self.assertEqual(self.pool.run_job(self.policy, "--system", self.base, self.command), 23)
        launch.assert_called_once()
        self.assertTrue(launch.call_args.kwargs["close_fds"])
        args = launch.call_args.args[0]
        self.assertEqual(args[:7], ["/bin/sh", str(SCRIPT.with_name("run_jade_pdf2docx_systemd.sh")),
                                   "--system", "--pool", "jade-pdf2docxTest.slice", "1024", "--job-token"])
        self.assertRegex(args[7], r"^[a-f0-9]{32}$")
        self.assertEqual(args[8:], ["512", "jade-pdf2docx-jade-pdf2docxTest-slot-2", *self.command])

    def test_registration_releases_startup_gate_while_service_keeps_running(self):
        child = Mock()
        child.poll.return_value = None
        def wait():
            # 変換の待機中は別の受付が同じgateを取得できる。
            with self.pool.startup_gate(self.base, self.policy):
                return 0
        child.wait.side_effect = wait
        with patch.object(self.pool, "unit_properties", side_effect=[
                {"LoadState": "not-found"},
                {"LoadState": "loaded", "Description": "jade-pdf2docx-job-" + "a" * 32}]), \
             patch.object(self.pool.uuid, "uuid4", return_value=Mock(hex="a" * 32)), \
             patch.object(self.pool.subprocess, "Popen", return_value=child):
            self.assertEqual(self.pool.run_job(self.policy, "--user", self.base, self.command), 0)

    def test_live_or_stale_policy_conflict_and_corrupt_gate_fail_closed(self):
        with self.pool.startup_gate(self.base, self.policy):
            pass
        changed = dict(self.policy, slots=3)
        with self.assertRaises(ValueError):
            with self.pool.startup_gate(self.base, changed):
                self.fail("inconsistent capacity was admitted")
        gate = next(self.base.glob("*.lock"))
        gate.write_text("partial")
        with self.assertRaises(ValueError):
            with self.pool.startup_gate(self.base, self.policy):
                self.fail("corrupt gate was admitted")

    def test_gate_rejects_symlink_hardlink_and_permissive_directory(self):
        with self.pool.startup_gate(self.base, self.policy):
            pass
        gate = next(self.base.glob("*.lock"))
        target = self.base / "target"
        gate.rename(target)
        gate.symlink_to(target)
        with self.assertRaises((ValueError, OSError)):
            with self.pool.startup_gate(self.base, self.policy):
                self.fail("symlink was admitted")
        gate.unlink()
        os.link(target, gate)
        with self.assertRaises(ValueError):
            with self.pool.startup_gate(self.base, self.policy):
                self.fail("hardlink was admitted")
        self.base.chmod(0o777)
        with self.assertRaises(ValueError):
            with self.pool.startup_gate(self.base, self.policy):
                self.fail("writable directory was admitted")
        self.base.chmod(0o700)

    def test_manager_failure_never_executes_or_retries_job(self):
        failure = subprocess.TimeoutExpired("systemctl", 2)
        with patch.object(self.pool, "unit_properties", side_effect=failure), \
             patch.object(self.pool.subprocess, "Popen") as launch:
            with self.assertRaises(subprocess.TimeoutExpired):
                self.pool.run_job(self.policy, "--user", self.base, self.command)
            launch.assert_not_called()

    def test_startup_gate_is_bounded_and_released_after_holder_sigkill(self):
        program = (
            "import importlib.util,json,pathlib,sys,time\n"
            "sys.path.insert(0,str(pathlib.Path(sys.argv[1]).parent))\n"
            "spec=importlib.util.spec_from_file_location('pool',sys.argv[1])\n"
            "pool=importlib.util.module_from_spec(spec);spec.loader.exec_module(pool)\n"
            "with pool.startup_gate(pathlib.Path(sys.argv[2]),json.loads(sys.argv[3])):\n"
            " print('locked',flush=True)\n"
            " time.sleep(30)\n"
        )
        holder = subprocess.Popen([sys.executable, "-c", program, str(SCRIPT),
                                   str(self.base), json.dumps(self.policy)],
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            self.assertEqual(holder.stdout.readline(), "locked\n")
            with patch.object(self.pool, "STARTUP_SECONDS", 0.05):
                with self.assertRaises(self.pool.PoolBusy):
                    with self.pool.startup_gate(self.base, self.policy):
                        self.fail("two startup gate holders")
            holder.kill()
            holder.communicate(timeout=5)
            with self.pool.startup_gate(self.base, self.policy):
                pass
        finally:
            if holder.poll() is None:
                holder.kill()
            holder.communicate(timeout=5)

    def test_unavailable_manager_is_not_mistaken_for_a_free_slot(self):
        results = [subprocess.CompletedProcess([], 1, "", "bus unavailable"),
                   subprocess.CompletedProcess([], 1, "LoadState=loaded\n", "error")]
        for result in results:
            with patch.object(self.pool.subprocess, "run", return_value=result):
                with self.assertRaises(ValueError):
                    self.pool.unit_properties("--system", "jade-pdf2docx-test")
        absent = subprocess.CompletedProcess([], 4, "LoadState=not-found\n", "")
        with patch.object(self.pool.subprocess, "run", return_value=absent):
            self.assertEqual(self.pool.unit_properties("--user", "jade-pdf2docx-test")["LoadState"], "not-found")

    def test_registration_observation_failure_waits_original_child_without_retry(self):
        child = Mock()
        child.poll.return_value = None
        child.wait.return_value = 17
        with patch.object(self.pool, "unit_properties", side_effect=[
                {"LoadState": "not-found"}, subprocess.TimeoutExpired("systemctl", 2)]), \
             patch.object(self.pool.subprocess, "Popen", return_value=child) as launch:
            self.assertEqual(self.pool.run_job(self.policy, "--user", self.base, self.command), 17)
        launch.assert_called_once()
        child.wait.assert_called_once()

    def test_signaled_waiter_returns_conventional_signal_exit_status(self):
        child = Mock()
        child.poll.return_value = -9
        child.wait.return_value = -9
        with patch.object(self.pool, "unit_properties", return_value={"LoadState": "not-found"}), \
             patch.object(self.pool.subprocess, "Popen", return_value=child):
            self.assertEqual(self.pool.run_job(self.policy, "--user", self.base, self.command), 137)
