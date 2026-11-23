"""固定ReportLab一次配布物のPDF用glyph名表だけをRustへ生成する。実装は取り込まない。"""
from pathlib import Path
import argparse
import ast
import hashlib
import io
import json
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
META = ROOT / "crates/jade-pdf2docx-parser/resources/pdf-font-encodings.json"
TARGET = ROOT / "crates/jade-pdf2docx-parser/src/type1_outline_pdf_encodings.rs"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-directory", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    metadata = json.loads(META.read_text(encoding="utf-8"))
    archive = None
    if not args.source_directory:
        with urllib.request.urlopen(metadata["source_archive"], timeout=30) as response:
            raw = response.read(8 * 1024 * 1024 + 1)
        if len(raw) > 8 * 1024 * 1024 or hashlib.sha256(raw).hexdigest() != metadata["source_archive_sha256"]:
            raise SystemExit("primary source archive SHA256 mismatch")
        archive = tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz")
    output = (
        "//! 固定ReportLab4.4.9の標準glyph名のみ。tools/generate_pdf_font_encodings.pyで生成。\n"
        "// Copyright (c) 2000-2024, ReportLab Inc.; BSD-3-Clause: ../licenses/reportlab.txt\n"
        "// Provenance: ../resources/pdf-font-encodings.json; ISO32000-1 AnnexD\n\n"
    )
    for table in metadata["tables"]:
        if args.source_directory:
            source = (args.source_directory / table["file"]).read_bytes()
        else:
            member = next(m for m in archive.getmembers() if m.name.endswith("/src/reportlab/pdfbase/" + table["file"]))
            source = archive.extractfile(member).read()
        if hashlib.sha256(source).hexdigest() != table["sha256"]:
            raise SystemExit("primary source table SHA256 mismatch")
        names = next(ast.literal_eval(n.value) for n in ast.parse(source).body
                     if isinstance(n, ast.Assign) and any(isinstance(t, ast.Name) and t.id == table["definition"] for t in n.targets))
        assert len(names) == table["entries"] == 256
        assert all(n is None or (isinstance(n, str) and n.isascii() and 0 < len(n) <= 127) for n in names)
        output += (
            "/// " + table["definition"] + "の256codeを、未定義は.notdefとして保持する。\n"
            "#[rustfmt::skip]\npub(super) const " + table["constant"] + ": [&[u8]; 256] = [\n"
            + "".join("    b" + json.dumps(n or ".notdef") + ",\n" for n in names) + "];\n\n"
        )
    output = output.rstrip() + "\n"
    license_path = META.parent.parent / metadata["license"]
    if hashlib.sha256(license_path.read_bytes()).hexdigest() != metadata["license_sha256"]:
        raise SystemExit("ReportLab license SHA256 mismatch")
    if args.check:
        if TARGET.read_text(encoding="utf-8") != output:
            raise SystemExit("generated PDF encoding tables differ")
    else:
        TARGET.write_text(output, encoding="utf-8")


if __name__ == "__main__":
    main()
