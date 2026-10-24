#!/usr/bin/env python3
"""Adjudicate orphan page Widget values on a pinned Apache regression PDF.

The original has an empty catalog Fields array and 220 unique page Widgets.
Raw pypdf declarations validate names/types/values independently of the catalog.
Controlled hidden-value/default-value edits keep uncertainty and do not generate
page bounds for document-level records. Full JavaScript/appearance semantics are
not verified. Source bytes are retained unchanged.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
from collections import Counter
from pypdf import PdfReader, PdfWriter, __version__ as pypdf_version
from pypdf.generic import ArrayObject, NameObject, NullObject, NumberObject, TextStringObject
from pdf_corpus import ROOT, fetch


def widgets(reader):
    return [x.get_object() for page in reader.pages for x in page.get("/Annots", [])
            if x.get_object().get("/Subtype") == "/Widget"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/pdf-diff")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--report", type=Path, default=ROOT / "corpus/results-orphan-forms.json")
    args = parser.parse_args()
    manifest_bytes = (ROOT / "corpus/manifest.json").read_bytes()
    entry = next(x for x in json.loads(manifest_bytes)["documents"] if x["id"] == "pdfbox-orphan-widgets")
    fetch(entry, ROOT / "corpus/cache", offline=True)
    source = ROOT / "corpus/cache" / (entry["id"] + ".pdf")
    reader = PdfReader(source)
    raw = widgets(reader)
    gold = entry["page_widget_gold"]
    assert len(raw) == gold["widgets"] == len(set(str(x["/T"]) for x in raw)) == 220
    assert dict(Counter(str(x["/FT"]) for x in raw)) == gold["field_types"]
    assert not reader.get_fields() and len(reader.root_object["/AcroForm"]["/Fields"]) == 0
    assert all("/Parent" not in x for x in raw)
    named = {str(x["/T"]): x for x in raw}
    for name, value in gold["saved_values"].items():
        assert named[name]["/V"] == value
    def compare(binary, old, new):
        run = subprocess.run([str(binary.resolve()), str(old), str(new)], capture_output=True, text=True, timeout=120)
        assert run.returncode in (0, 1, 3), run.stderr[-2000:]
        return run.returncode, json.loads(run.stdout)
    rows = []
    with tempfile.TemporaryDirectory(prefix="jade-orphan-audit-") as temporary:
        directory = Path(temporary)
        rewritten = directory / "rewrite.pdf"
        rewritten.write_bytes(source.read_bytes() + b"\n% orphan inventory rewrite\n")
        code, diff = compare(args.binary, source, rewritten)
        assert code == 3 and not any(diff["summary"].values())
        assert diff["comparison"]["canonical_fields_examined"] == 440
        assert diff["comparison"]["canonical_form_review_reasons"]
        row = dict(id="original-page-widget-inventory", passed=True, page_widgets=220,
                   catalog_fields=0, independently_checked_saved_values=gold["saved_values"],
                   summary=diff["summary"], coverage=diff["comparison"])
        if args.baseline:
            _, old = compare(args.baseline, source, rewritten)
            assert old["comparison"]["canonical_fields_examined"] == 0
            row["baseline_coverage"] = old["comparison"]
        rows.append(row)
        for mode in ["original-catalog", "no-catalog", "null-catalog", "missing-fields", "partial-fields"]:
            for property_name in ["/V", "/DV"]:
                paths = []
                for side, value in [("old", ""), ("new", "Changed saved alert")]:
                    writer = PdfWriter(clone_from=source)
                    fields = widgets(writer)
                    field = next(x for x in fields if x.get("/T") == "AlertField0")
                    field[NameObject("/F")] = NumberObject(2)
                    field[NameObject(property_name)] = TextStringObject(value)
                    if mode == "no-catalog":
                        del writer.root_object["/AcroForm"]
                    elif mode == "null-catalog":
                        writer.root_object[NameObject("/AcroForm")] = NullObject()
                    elif mode == "missing-fields":
                        del writer.root_object["/AcroForm"]["/Fields"]
                    elif mode == "partial-fields":
                        other = next(x for x in fields if x.get("/T") == "PortfolioTable0:0")
                        writer.root_object["/AcroForm"][NameObject("/Fields")] = ArrayObject([other.indirect_reference])
                    path = directory / (side + ".pdf")
                    writer.write(path)
                    verify = next(x for x in widgets(PdfReader(path)) if x.get("/T") == "AlertField0")
                    assert verify["/F"] == 2 and verify[property_name] == value
                    paths.append(path)
                code, diff = compare(args.binary, *paths)
                assert code == 3 and diff["summary"]["canonical_form_fields_changed"] == 1
                assert diff["summary"]["pages_visual_changed"] == diff["summary"]["form_fields_changed"] == 0
                assert diff["comparison"]["canonical_fields_examined"] == 440
                assert diff["comparison"]["canonical_form_review_reasons"]
                change = diff["canonical_form_changes"][0]
                assert change["status"] == "modified" and change["old"]["name"] == change["new"]["name"] == ["AlertField0"]
                assert change["old"]["properties"][property_name[1:]] == ""
                assert change["new"]["properties"][property_name[1:]] == "Changed saved alert"
                assert all(not p["form_changes"] for p in diff["pages"])
                row = dict(id=mode + "-" + property_name[1:], passed=True, change=change,
                           summary=diff["summary"], coverage=diff["comparison"])
                if args.baseline:
                    code0, old = compare(args.baseline, *paths)
                    assert code0 == 3 and not any(old["summary"].values())
                    row.update(baseline_summary=old["summary"], baseline_coverage=old["comparison"])
                rows.append(row)
    report = dict(version=1, methodology=__doc__, binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                  manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(), source_sha256=entry["sha256"],
                  tool_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), parser="pypdf " + pypdf_version,
                  cases=len(rows), passed=sum(x["passed"] for x in rows), results=rows)
    if args.baseline:
        report["baseline_binary_sha256"] = hashlib.sha256(args.baseline.read_bytes()).hexdigest()
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(f"{report['passed']}/{report['cases']} orphan source controls passed")


if __name__ == "__main__":
    main()
