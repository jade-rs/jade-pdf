"""公開PDFの出典・固定ハッシュと、旧データの混入防止を検査する。"""
import hashlib
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class FixtureHygieneTest(unittest.TestCase):
    def test_legacy_directories_are_absent(self):
        for name in (
            "tests/fixtures", "scripts/fixtures",
            "crates/jade-pdf-diff/tests/fixtures", "docs/benchmarks",
            "ml-ocr", "pdf-parser",
        ):
            with self.subTest(directory=name):
                self.assertFalse((ROOT / name).exists())

    def test_only_attributed_pinned_public_pdfs_are_stored(self):
        root = ROOT / "fixtures"
        manifest = json.loads((root / "manifest.json").read_text())
        self.assertEqual(manifest["license"], "CC-BY-SA-4.0")
        self.assertTrue((root / "LICENSE-CC-BY-SA-4.0.txt").is_file())
        self.assertEqual(len(manifest["cases"]), 3)
        declared = set()
        for case in manifest["cases"]:
            with self.subTest(case=case["name"]):
                for key in ("input", "expected"):
                    path = Path(case[key])
                    self.assertFalse(path.is_absolute())
                    self.assertNotIn("..", path.parts)
                    declared.add(path.as_posix())
                    self.assertFalse((root / path).is_symlink())
                self.assertTrue(case["attribution"])
                self.assertEqual(case["modifications"], "none")
                self.assertRegex(case["revision"], r"^[0-9a-f]{40}$")
                self.assertTrue(case["source"].startswith(
                    "https://raw.githubusercontent.com/py-pdf/sample-files/" + case["revision"] + "/"))
                pdf = (root / case["input"]).read_bytes()
                self.assertTrue(pdf.startswith(b"%PDF-"))
                self.assertEqual(hashlib.sha256(pdf).hexdigest(), case["sha256"])
                expected = (root / case["expected"]).read_bytes()
                self.assertEqual(hashlib.sha256(expected).hexdigest(), case["expected_sha256"])
                self.assertEqual(len([p for p in expected.split(b"\x0c") if p.strip()]), case["pages"])
        stored = {p.relative_to(root).as_posix() for directory in ("input", "expected")
                  for p in (root / directory).rglob("*") if p.is_file()}
        self.assertEqual(stored, declared)
        self.assertEqual({p.name for p in root.iterdir()}, {
            "README.md", "manifest.json", "LICENSE-CC-BY-SA-4.0.txt", "input", "expected"})


if __name__ == "__main__":
    unittest.main()
