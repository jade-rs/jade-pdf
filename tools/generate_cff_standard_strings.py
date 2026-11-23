"""固定したfontTools一次資料の標準名391件だけをRustの定数へ変換する。"""
from pathlib import Path
import argparse
import ast
import hashlib
import json
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
META = ROOT / "crates/jade-pdf2docx-parser/resources/cff-standard-strings.json"
TARGET = ROOT / "crates/jade-pdf2docx-parser/src/type1_outline_cff_strings.rs"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, help="固定SHA256と照合する既存の一次資料")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    metadata = json.loads(META.read_text(encoding="utf-8"))
    if args.source:
        source = args.source.read_bytes()
    else:
        with urllib.request.urlopen(metadata["source"], timeout=30) as response:
            source = response.read(1024 * 1024 + 1)
    if len(source) > 1024 * 1024 or hashlib.sha256(source).hexdigest() != metadata["source_sha256"]:
        raise SystemExit("primary source SHA256 mismatch")
    strings = next(
        ast.literal_eval(node.value)
        for node in ast.parse(source).body
        if isinstance(node, ast.Assign)
        and any(isinstance(target, ast.Name) and target.id == "cffStandardStrings" for target in node.targets)
    )
    assert len(strings) == len(set(strings)) == metadata["string_count"] == 391
    assert all(isinstance(s, str) and s.isascii() for s in strings)
    output = (
        "//! tools/generate_cff_standard_strings.pyで固定fontTools4.60.1から生成。手動編集しない。\n"
        "// Copyright (c) 2017 Just van Rossum; MIT notice: ../licenses/fonttools.txt\n"
        "// Provenance: ../resources/cff-standard-strings.json; Adobe CFF Appendix A\n\n"
        "/// Adobe CFFの標準文字列をSID順に保存する。runtime索引の確保は不要。\n"
        "#[rustfmt::skip]\n"
        "pub(super) const STANDARD: [&[u8]; 391] = [\n"
        + "".join("    b" + json.dumps(s) + ",\n" for s in strings)
        + "];\n"
    )
    if args.check:
        if TARGET.read_text(encoding="utf-8") != output:
            raise SystemExit("generated CFF string table differs")
    else:
        TARGET.write_text(output, encoding="utf-8")


if __name__ == "__main__":
    main()
