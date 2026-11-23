"""固定fontToolsのOpenType Unicode範囲だけを、数値のRust定数へ変換する。"""
from pathlib import Path
import argparse
import ast
import hashlib
import json
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
META = ROOT / "crates/jade-pdf2docx-parser/resources/opentype-unicode-ranges.json"
TARGET = ROOT / "crates/jade-pdf2docx-parser/src/type1_outline_unicode_ranges.rs"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path)
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
    ranges = next(ast.literal_eval(node.value) for node in ast.parse(source).body
                  if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == "OS2_UNICODE_RANGES" for t in node.targets))
    assert len(ranges) == 123
    entries = [(bit, start, end) for bit, blocks in enumerate(ranges) for _, (start, end) in blocks]
    assert all(0 <= start <= end <= 0x10FFFF for _, start, end in entries)
    output = ("//! 固定fontTools4.60.1からtools/generate_opentype_unicode_ranges.pyで生成。\n"
              "// Copyright (c) 2017 Just van Rossum; MIT: ../licenses/fonttools.txt\n"
              "// Provenance: ../resources/opentype-unicode-ranges.json\n\n"
              "/// OpenType OS/2のcoverage bitと対応するUnicode範囲。bit57は別途全補助平面にも適用する。\n"
              "#[rustfmt::skip]\n"
              "pub(super) const RANGES: &[(u8, u32, u32)] = &[\n"
              + "".join(f"    ({bit}, 0x{start:X}, 0x{end:X}),\n" for bit, start, end in entries) + "];\n")
    if args.check:
        if TARGET.read_text(encoding="utf-8") != output:
            raise SystemExit("generated Unicode ranges differ")
    else:
        TARGET.write_text(output, encoding="utf-8")


if __name__ == "__main__":
    main()
