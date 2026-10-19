import re
import unittest
from pathlib import Path


STYLE = Path(__file__).resolve().parents[1] / "playground" / "src" / "style.css"
MAIN = Path(__file__).resolve().parents[1] / "playground" / "src" / "main.js"


class PlaygroundLayoutTest(unittest.TestCase):
    def test_inspect_output_wraps_long_pdf_inspection_json(self):
        css = STYLE.read_text()
        rule = css_rule(css, r"#inspect-output")

        self.assertIn("max-width: 100%", rule)
        self.assertIn("white-space: pre-wrap", rule)
        self.assertIn("overflow-wrap: anywhere", rule)
        self.assertIn("box-sizing: border-box", rule)

    def test_playground_uses_structured_conversion_for_empty_cell_tables(self):
        js = MAIN.read_text()
        condition = re.search(
            r"const hasStructuredContent\s*=\s*(?P<body>.*?);",
            js,
            re.DOTALL,
        )

        self.assertIsNotNone(condition)
        self.assertIn("inferred_table_count", condition.group("body"))
        self.assertIn("convertPdfToDocx(selectedBytes)", js)


def css_rule(css: str, selector: str) -> str:
    match = re.search(selector + r"\s*\{(?P<body>[^}]*)\}", css, re.MULTILINE)
    if match is None:
        return ""
    return match.group("body")


if __name__ == "__main__":
    unittest.main()
