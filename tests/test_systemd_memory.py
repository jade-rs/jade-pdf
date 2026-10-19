"""親CLIを含むsystemdサービスの起動契約を実プロセスで検証する。"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/run_jade_pdf2docx_systemd.sh"


class SystemdMemoryTest(unittest.TestCase):
    def launch(self, args, extra_env=None, failure=0):
        """systemdの代替は引数だけを記録し、制限失敗をそのまま返す。"""
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            log = directory / "arguments.json"
            command = directory / "systemd-run"
            command.write_text(
                "#!/usr/bin/env python3\n"
                "import json, os, sys\n"
                "with open(os.environ['JADE_PDF2DOCX_TEST_SYSTEMD_LOG'], 'w') as output:\n"
                "    json.dump(sys.argv[1:], output)\n"
                "sys.exit(int(os.environ['JADE_PDF2DOCX_TEST_SYSTEMD_EXIT']))\n"
            )
            command.chmod(0o700)
            environment = os.environ.copy()
            for key in ("SUDO_UID", "SUDO_GID", "PDFIUM_LIB_PATH"):
                environment.pop(key, None)
            environment.update(extra_env or {})
            environment.update(
                PATH=str(directory) + os.pathsep + environment["PATH"],
                JADE_PDF2DOCX_TEST_SYSTEMD_LOG=str(log),
                JADE_PDF2DOCX_TEST_SYSTEMD_EXIT=str(failure),
            )
            result = subprocess.run(
                ["sh", str(SCRIPT), *args], env=environment,
                capture_output=True, text=True, timeout=10,
            )
            return result, json.loads(log.read_text()) if log.exists() else None

    def test_whole_cli_service_is_limited_before_execution(self):
        args = ["512", "jade-pdf2docx-test-one", "/tmp/my jade-pdf2docx", "a $HOME.pdf", "out.docx"]
        result, command = self.launch(args)
        self.assertEqual(result.returncode, 0, result.stderr)
        for option in (
            "--user", "--wait", "--pipe", "--collect", "--service-type=exec",
            "--expand-environment=no", "--unit=jade-pdf2docx-test-one",
            "--property=MemoryAccounting=yes", "--property=MemoryMax=512M",
            "--property=MemorySwapMax=0", "--property=OOMPolicy=kill",
            "--property=KillMode=control-group", "--property=TimeoutStopSec=5s",
        ):
            self.assertIn(option, command)
        service = command[command.index("--") + 1:]
        self.assertEqual(service[:2], ["/bin/sh", str(SCRIPT.with_name("exec_in_memory_service.sh"))])
        self.assertEqual(service[2:], ["512", *args[2:]])
        self.assertIn("--working-directory=" + os.getcwd(), command)

    def test_system_service_preserves_the_sudo_callers_identity_and_pdfium(self):
        result, command = self.launch(
            ["--system", "64", "jade-pdf2docx-test-two", "/bin/jade-pdf2docx", "in", "out"],
            {"SUDO_UID": "1001", "SUDO_GID": "1002", "PDFIUM_LIB_PATH": "/a b/lib.so"},
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        for option in ("--system", "--uid=1001", "--gid=1002",
                       "--setenv=PDFIUM_LIB_PATH=/a b/lib.so"):
            self.assertIn(option, command)
        self.assertNotIn("--user", command)

    def test_invalid_capacity_or_unit_never_starts_conversion(self):
        for capacity in ("0", "-1", "1.5", "1M", "8796093022208", "9" * 100):
            with self.subTest(capacity=capacity):
                result, command = self.launch([capacity, "jade-pdf2docx-test", "/bin/jade-pdf2docx", "in", "out"])
                self.assertEqual(result.returncode, 2)
                self.assertIsNone(command)
        for unit in ("other", "jade-pdf2docx-", "jade-pdf2docx-a/b", "jade-pdf2docx-a.service", "jade-pdf2docx-" + "x" * 200):
            with self.subTest(unit=unit):
                result, command = self.launch(["64", unit, "/bin/jade-pdf2docx", "in", "out"])
                self.assertEqual(result.returncode, 2)
                self.assertIsNone(command)

    def test_missing_or_relative_executable_is_rejected(self):
        for args in (["64", "jade-pdf2docx-test"], ["64", "jade-pdf2docx-test", "jade-pdf2docx", "in", "out"]):
            result, command = self.launch(args)
            self.assertEqual(result.returncode, 2)
            self.assertIsNone(command)

    def test_limit_setup_failure_does_not_fall_back_to_unlimited_execution(self):
        result, command = self.launch(["64", "jade-pdf2docx-test", "/bin/jade-pdf2docx", "in", "out"], failure=23)
        self.assertIsNotNone(command)
        self.assertEqual(result.returncode, 23)

    def test_maximum_capacity_and_nonzero_leading_zero_capacity(self):
        for capacity in ("8796093022207", "001"):
            result, _ = self.launch([capacity, "jade-pdf2docx-test", "/bin/jade-pdf2docx", "in", "out"])
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_shared_pool_is_passed_to_manager_and_kernel_verifier(self):
        args = ["--system", "--pool", "jade-pdf2docxProduction.slice", "001024",
                "512", "jade-pdf2docx-test-pool", "/bin/jade-pdf2docx", "in", "out"]
        result, command = self.launch(args)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--slice=jade-pdf2docxProduction.slice", command)
        service = command[command.index("--") + 1:]
        self.assertEqual(service[2:], ["--pool", "jade-pdf2docxProduction.slice", "1024",
                                      "512", "/bin/jade-pdf2docx", "in", "out"])

    def test_invalid_pool_never_starts_conversion(self):
        for name in ("jade-pdf2docx.slice/other", "jade-pdf2docx-other.slice", "other.slice", "jade-pdf2docx.slice.slice",
                     "jade-pdf2docx.slice\n", "jade-pdf2docx" + "x" * 250 + ".slice"):
            with self.subTest(name=name):
                result, command = self.launch(["--pool", name, "1024", "512",
                                               "jade-pdf2docx-test", "/bin/jade-pdf2docx", "in", "out"])
                self.assertEqual(result.returncode, 2)
                self.assertIsNone(command)
        for capacity in ("0", "-1", "1M", "8796093022208", "9" * 100):
            result, command = self.launch(["--pool", "jade-pdf2docx.slice", capacity, "512",
                                           "jade-pdf2docx-test", "/bin/jade-pdf2docx", "in", "out"])
            self.assertEqual(result.returncode, 2)
            self.assertIsNone(command)
        result, command = self.launch(["--pool", "jade-pdf2docx.slice"])
        self.assertEqual(result.returncode, 2)
        self.assertIsNone(command)

    def test_job_registration_token_is_literal_and_never_reaches_cli(self):
        token = "a" * 32
        result, command = self.launch(["--pool", "jade-pdf2docx.slice", "1024", "--job-token", token,
                                       "512", "jade-pdf2docx-test", "/bin/jade-pdf2docx", "in", "out"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--description=jade-pdf2docx-job-" + token, command)
        service = command[command.index("--") + 1:]
        self.assertEqual(service[2:], ["--pool", "jade-pdf2docx.slice", "1024", "512", "/bin/jade-pdf2docx", "in", "out"])
        for invalid in ("", "x" * 32, "a" * 31, "a" * 33, "a" * 31 + "\n"):
            result, command = self.launch(["--job-token", invalid, "512", "jade-pdf2docx-test", "/bin/jade-pdf2docx", "in", "out"])
            self.assertEqual(result.returncode, 2)
            self.assertIsNone(command)

    def test_history_hook_is_a_manager_option_and_never_reaches_cli(self):
        token = "a" * 32
        result, command = self.launch(
            ["--system", "--pool", "jade-pdf2docx.slice", "1024", "--job-token", token,
             "--history-dir", "/absolute/jobs $literal%name", "512", "jade-pdf2docx-test",
             "/bin/jade-pdf2docx", "in", "out"], {"SUDO_UID": "1001", "SUDO_GID": "1002"})
        self.assertEqual(result.returncode, 0, result.stderr)
        hooks = [value for value in command if value.startswith("--property=ExecStopPost=")]
        self.assertEqual(len(hooks), 1)
        self.assertLess(command.index(hooks[0]), command.index("--"))
        self.assertIn('"/absolute/jobs $literal%name"', hooks[0])
        service = command[command.index("--") + 1:]
        self.assertEqual(service[:2], ["/bin/sh", str(SCRIPT.with_name("exec_in_memory_service.sh"))])
        self.assertEqual(service[2:], ["--pool", "jade-pdf2docx.slice", "1024", "512", "/bin/jade-pdf2docx", "in", "out"])

    def test_missing_kernel_limit_never_executes_the_cli(self):
        # 実controllerの未対応・未委譲はsystemdの受付結果だけで判断しない。
        with tempfile.TemporaryDirectory() as temporary:
            marker = Path(temporary) / "executed"
            result = subprocess.run(
                ["sh", str(SCRIPT.with_name("exec_in_memory_service.sh")),
                 "8796093022207", "/usr/bin/touch", str(marker)],
                capture_output=True, text=True, timeout=10,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("memory service limit unavailable", result.stderr)
            self.assertFalse(marker.exists())
