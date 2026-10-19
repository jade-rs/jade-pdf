"""CI とローカルで共用する feature 検証の実行契約。"""

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "check_rust_features.sh"


class RustFeatureChecksTest(unittest.TestCase):
    def run_checks(self, mode, fail_command=None):
        """Cargo を代替し、引数と失敗後の実行を実プロセスで観測する。"""
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            log = directory / "commands.jsonl"
            cargo = directory / "cargo"
            cargo.write_text(
                "#!/usr/bin/env python3\n"
                "import json, os, sys\n"
                "with open(os.environ['JADE_PDF2DOCX_TEST_CARGO_LOG'], 'a') as log:\n"
                "    log.write(json.dumps({'args': sys.argv[1:], 'environment': {key: os.environ.get(key) for key in ('CARGO_PROFILE_DEV_DEBUG', 'CARGO_PROFILE_TEST_DEBUG', 'CARGO_INCREMENTAL')}}) + '\\n')\n"
                "sys.exit(23 if sys.argv[1] == os.environ.get('JADE_PDF2DOCX_TEST_CARGO_FAIL') else 0)\n"
            )
            cargo.chmod(0o700)
            environment = os.environ.copy()
            environment["PATH"] = str(directory) + os.pathsep + environment["PATH"]
            environment["JADE_PDF2DOCX_TEST_CARGO_LOG"] = str(log)
            environment["JADE_PDF2DOCX_TEST_CARGO_FAIL"] = fail_command or ""
            result = subprocess.run(
                ["sh", str(SCRIPT), mode],
                env=environment,
                capture_output=True,
                text=True,
                timeout=10,
            )
            commands = (
                [json.loads(line) for line in log.read_text().splitlines()]
                if log.exists()
                else []
            )
            return result, commands

    def test_checks_all_targets_and_tests_each_feature_with_locked_resolution(self):
        for mode in ("default", "render", "ocr", "ocr-onnx"):
            with self.subTest(mode=mode):
                result, commands = self.run_checks(mode)
                self.assertEqual(result.returncode, 0, result.stderr)
                features = [] if mode == "default" else ["--features", mode]
                self.assertEqual(
                    [command['args'] for command in commands],
                    [
                        ["clippy", "--workspace", "--exclude", "jade-pdf-diff", "--all-targets", "--locked", *features],
                        ["test", "--workspace", "--exclude", "jade-pdf-diff", "--locked", *features],
                    ],
                )

    def test_builds_and_nested_cargo_keep_debug_and_incremental_artifacts_disabled(self):
        for mode in ("default", "render", "ocr", "ocr-onnx"):
            with self.subTest(mode=mode):
                result, commands = self.run_checks(mode)
                self.assertEqual(result.returncode, 0, result.stderr)
                for command in commands:
                    self.assertEqual(
                        command["environment"],
                        {
                            "CARGO_PROFILE_DEV_DEBUG": "0",
                            "CARGO_PROFILE_TEST_DEBUG": "0",
                            "CARGO_INCREMENTAL": "0",
                        },
                    )

    def test_failure_stops_the_pipeline_and_preserves_exit_code(self):
        for command, expected_count in (("clippy", 1), ("test", 2)):
            with self.subTest(command=command):
                result, commands = self.run_checks("ocr", command)
                self.assertEqual(result.returncode, 23, result.stderr)
                self.assertEqual(len(commands), expected_count)

    def test_unknown_feature_is_rejected_before_cargo(self):
        result, commands = self.run_checks("invalid")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(commands, [])
