#!/usr/bin/env python3
"""Adjudicate independent form discovery, null semantics and repeated widgets.

The original mixed-form PDF is SHA-pinned. pypdf's convenience API fails on its
missing radio appearance; raw declarations provide an explicit independent gold.
A placed edit and an unplaced-field variant keep the first saved export unchanged.
Synthetic catalog controls use the pinned public SDK's actual form-type result.
Only the ISO reference is excluded from executable corpus denominators.
"""
import argparse
import ctypes as C
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile

from pypdf import PdfReader, PdfWriter, __version__ as pypdf_version
from pypdf.generic import ArrayObject, NameObject, TextStringObject
from audit_form_fields import declared_fields, value_oracle
from pdf_corpus import ROOT, fetch


def catalog_pdf(mode):
    entry = {
        "absent": "", "null": "/AcroForm null", "null-ref": "/AcroForm 7 0 R",
        "dangling": "/AcroForm 99 0 R", "cycle": "/AcroForm 8 0 R",
        "number": "/AcroForm 42", "name": "/AcroForm /Unknown",
        "string": "/AcroForm (Unknown)", "array": "/AcroForm [6 0 R]",
        "boolean": "/AcroForm true",
    }[mode]
    text = "BT /F1 12 Tf 30 100 Td (Stable page) Tj ET"
    objects = [f"<< /Type /Catalog /Pages 2 0 R {entry} >>",
               "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
               "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
               "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
               f"<< /Length {len(text)} >>\nstream\n{text}\nendstream",
               "<< /FT /Tx /T (Unused) /V (unused) >>", "null", "8 0 R"]
    data, offsets = b"%PDF-1.7\n", []
    for i, obj in enumerate(objects):
        offsets.append(len(data))
        data += f"{i+1} 0 obj\n{obj}\nendobj\n".encode()
    start = len(data)
    data += b"xref\n0 9\n0000000000 65535 f \n"
    data += "".join(f"{x:010} 00000 n \n" for x in offsets).encode()
    data += f"trailer\n<< /Size 9 /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n".encode()
    return data


def library_path():
    pins = json.loads((ROOT / "crates/jade-pdf-diff/pdfium-assets.json").read_bytes())
    system = {"Darwin": "mac", "Linux": "linux"}[platform.system()]
    machine = {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x64"}[platform.machine()]
    asset = next(a for a in pins["assets"] if a["name"] == f"pdfium-{system}-{machine}.tgz")
    member, metadata = next(iter(asset["libraries"].items()))
    default = Path.home() / "Library/Caches" if system == "mac" else Path(os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache")))
    root = Path(os.environ.get("JADE_PDFIUM_CACHE_DIR", os.environ.get("PDFIUM_AUTO_CACHE_DIR", str(default / "jade-pdf"))))
    path = root / ("pdfium-" + pins["version"]) / metadata["sha256"] / Path(member).name
    assert path.stat().st_size == metadata["bytes"] and hashlib.sha256(path.read_bytes()).hexdigest() == metadata["sha256"]
    return path, metadata["sha256"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/pdf-diff")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--report", type=Path, default=ROOT / "corpus/results-form-discovery.json")
    args = parser.parse_args()
    manifest_bytes = (ROOT / "corpus/manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    entry = next(x for x in manifest["documents"] if x["id"] == "pdfium-multiple-form-types")
    fetch(entry, ROOT / "corpus/cache", offline=True)
    source = ROOT / "corpus/cache" / (entry["id"] + ".pdf")
    expected, api_error = declared_fields(PdfReader(source))
    gold = entry["canonical_field_gold"]
    assert set(expected) == set(gold)
    for name, values in gold.items():
        for key, value in values.items():
            assert value_oracle(expected[name]["/" + key]) == value
    library, library_hash = library_path()
    sdk = C.CDLL(str(library))
    sdk.FPDF_LoadMemDocument64.restype = C.c_void_p
    sdk.FPDF_LoadMemDocument64.argtypes = [C.c_void_p, C.c_size_t, C.c_char_p]
    sdk.FPDF_GetFormType.restype = C.c_int
    sdk.FPDF_GetFormType.argtypes = [C.c_void_p]
    sdk.FPDF_CloseDocument.argtypes = [C.c_void_p]
    sdk.FPDF_InitLibrary.restype = None
    sdk.FPDF_InitLibrary.argtypes = []
    sdk.FPDF_DestroyLibrary.restype = None
    sdk.FPDF_DestroyLibrary.argtypes = []
    sdk.FPDF_InitLibrary()
    rows = []
    env = dict(os.environ)
    env.pop("PDFIUM_LIB_PATH", None)
    def compare(binary, a, b):
        process = subprocess.run([str(binary.resolve()), str(a), str(b)], env=env,
                                 capture_output=True, text=True, timeout=120)
        assert process.returncode in (0, 1, 3), process.stderr[-2000:]
        return process.returncode, json.loads(process.stdout)
    try:
        with tempfile.TemporaryDirectory(prefix="jade-form-discovery-") as tmp:
            directory = Path(tmp)
            rewrite = directory / "rewrite.pdf"
            rewrite.write_bytes(source.read_bytes() + b"\n% representation-only rewrite\n")
            code, same = compare(args.binary, source, rewrite)
            assert code == 3 and not same["canonical_form_changes"]
            assert same["comparison"]["canonical_fields_examined"] == 10
            assert not same["comparison"]["canonical_form_review_reasons"]
            row = dict(id="original-mixed-form-inventory", passed=True,
                       source_sha256=entry["sha256"], independent_fields=5,
                       pypdf_convenience_api_error=api_error, coverage=same["comparison"])
            if args.baseline:
                _, old = compare(args.baseline, source, rewrite)
                assert old["comparison"]["canonical_fields_examined"] == 0
                assert any("Duplicate" in x for x in old["comparison"]["canonical_form_review_reasons"])
                row["baseline_coverage"] = old["comparison"]
            rows.append(row)
            for unplaced in [False, True]:
                files = []
                for side, last in [("old", "Gamma"), ("new", "Delta")]:
                    writer = PdfWriter(clone_from=source)
                    if unplaced:
                        for page in writer.pages:
                            if "/Annots" in page:
                                del page["/Annots"]
                    field = next(x.get_object() for x in writer.root_object["/AcroForm"]["/Fields"]
                                 if str(x.get_object().get("/T")) == "Listbox_MultiSelectMultipleSelected")
                    field[NameObject("/V")] = ArrayObject([TextStringObject("Epsilon"), TextStringObject(last)])
                    target = directory / (side + ".pdf")
                    writer.write(target)
                    verify, _ = declared_fields(PdfReader(target))
                    assert list(verify["Listbox_MultiSelectMultipleSelected"]["/V"]) == ["Epsilon", last]
                    files.append(target)
                code, diff = compare(args.binary, *files)
                assert code == 3 and diff["summary"]["canonical_form_fields_changed"] == 1
                assert diff["comparison"]["canonical_fields_examined"] == 10
                assert not diff["comparison"]["canonical_form_review_reasons"]
                change = diff["canonical_form_changes"][0]
                assert change["old"]["name"] == ["Listbox_MultiSelectMultipleSelected"]
                assert change["old"]["properties"]["V"] == ["Epsilon", "Gamma"]
                assert change["new"]["properties"]["V"] == ["Epsilon", "Delta"]
                row = dict(id="unplaced-saved-export-edit" if unplaced else "placed-saved-export-edit",
                           passed=True, summary=diff["summary"], coverage=diff["comparison"], change=change)
                if unplaced:
                    assert diff["summary"]["pages_visual_changed"] == diff["summary"]["form_fields_changed"] == 0
                    assert all(not p["form_changes"] for p in diff["pages"])
                if args.baseline:
                    _, old = compare(args.baseline, *files)
                    assert old["summary"]["canonical_form_fields_changed"] == 0
                    if unplaced:
                        assert all(v == 0 for v in old["summary"].values())
                    row["baseline_summary"] = old["summary"]
                rows.append(row)
            absent = directory / "absent.pdf"
            absent.write_bytes(catalog_pdf("absent"))
            for mode in ["absent", "null", "null-ref", "dangling", "number", "name", "string", "array", "boolean", "cycle"]:
                target = directory / ("catalog-" + mode + ".pdf")
                data = catalog_pdf(mode)
                target.write_bytes(data)
                buffer = C.create_string_buffer(data)
                document = sdk.FPDF_LoadMemDocument64(buffer, len(data), None)
                assert document
                kind = sdk.FPDF_GetFormType(document)
                sdk.FPDF_CloseDocument(document)
                assert kind == 0, (mode, kind)
                code, diff = compare(args.binary, absent, target)
                known_absent = mode in ("absent", "null", "null-ref", "dangling")
                assert code == (0 if known_absent else 3), (mode, code)
                assert bool(diff["comparison"]["canonical_form_review_reasons"]) == (not known_absent)
                assert not any(diff["summary"].values())
                row = dict(id="catalog-" + mode, passed=True, sdk_form_type=kind,
                           exit_code=code, diagnostics=diff["diagnostics"], coverage=diff["comparison"])
                if args.baseline:
                    old_code, old = compare(args.baseline, absent, target)
                    assert old_code == 0 and not any(old["summary"].values())
                    row["baseline_exit_code"] = old_code
                rows.append(row)
    finally:
        sdk.FPDF_DestroyLibrary()
    report = dict(version=1, methodology=__doc__, manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
                  binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                  tool_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  pypdf_version=pypdf_version, sdk_library_sha256=library_hash,
                  cases=len(rows), passed=sum(x["passed"] for x in rows), results=rows,
                  reference=next(x for x in manifest["references"] if x["id"] == "adobe-iso32000-1-2008"))
    if args.baseline:
        report["baseline_binary_sha256"] = hashlib.sha256(args.baseline.read_bytes()).hexdigest()
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(f"{report['passed']}/{report['cases']} form discovery/control checks passed")


if __name__ == "__main__":
    main()
