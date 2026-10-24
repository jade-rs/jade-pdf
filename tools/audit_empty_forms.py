#!/usr/bin/env python3
"""Verify empty AcroForm shell comparisons against independent public-source facts.

Every SHA-pinned normal input is inspected. Empty root arrays stay separate from
unplaced/orphan field objects, annotations and XFA. The two W3C table shells have
no fields/widgets/scripts by independent pypdf inspection. They must lose only
interactive review, retaining reading-order/source-text uncertainty, raster/Unicode results
and all diff summaries. This is not general action/signature/XFA certification.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

from pypdf import PdfReader, __version__ as pypdf_version
from pypdf.generic import ArrayObject, DictionaryObject, IndirectObject
from pdf_corpus import ROOT, fetch
from audit_form_fields import declared_fields


def facts(source):
    reader = PdfReader(source)
    catalog = reader.root_object
    form = catalog.get("/AcroForm")
    if form is None:
        return None
    form = form.get_object()
    if not isinstance(form, DictionaryObject):
        return dict(malformed_form=True)
    fields = form.get("/Fields")
    if fields is not None:
        fields = fields.get_object()
    canonical, api_error = declared_fields(reader)
    out = dict(pypdf_convenience_api_error=api_error, form_keys=sorted(form), fields_is_array=isinstance(fields, ArrayObject),
               root_fields=len(fields) if isinstance(fields, ArrayObject) else None,
               canonical_fields=len(canonical),
               page_annotations=sum((len(p["/Annots"]) if "/Annots" in p else 0) for p in reader.pages),
               has_xfa="/XFA" in form, field_type_dictionaries=0,
               widget_dictionaries=0, javascript_dictionaries=0)
    ids = {(obj, generation) for generation, values in reader.xref.items()
           for obj in values if obj > 0} | {(obj, 0) for obj in reader.xref_objStm}
    for obj, generation in sorted(ids):
        stack = [(reader.get_object(IndirectObject(obj, generation, reader)), 0)]
        while stack:
            value, depth = stack.pop()
            assert depth < 64, "Independent source object nesting exceeds 64"
            if isinstance(value, IndirectObject):
                continue  # all indirect objects are enumerated above
            if isinstance(value, DictionaryObject):
                out["field_type_dictionaries"] += int("/FT" in value)
                out["widget_dictionaries"] += int(value.get("/Subtype") == "/Widget")
                out["javascript_dictionaries"] += int("/JS" in value)
                stack.extend((item, depth + 1) for item in value.values())
            elif isinstance(value, ArrayObject):
                stack.extend((item, depth + 1) for item in value)
    out["indirect_objects_inspected"] = len(ids)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/pdf-diff")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--report", type=Path, default=ROOT / "corpus/results-empty-forms.json")
    args = parser.parse_args()
    manifest_bytes = (ROOT / "corpus/manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    results, inventory = [], []
    with tempfile.TemporaryDirectory(prefix="jade-empty-form-") as temp:
        directory = Path(temp)
        binary = directory / "pdf-diff"
        binary.write_bytes(args.binary.read_bytes())
        binary.chmod(0o700)
        binary_hash = sha(binary)
        def compare(executable, source, rewrite):
            process = subprocess.run([str(executable), str(source), str(rewrite)],
                                     capture_output=True, text=True, timeout=120)
            assert process.returncode in (0, 1, 3), process.stderr[:2000]
            return process.returncode, json.loads(process.stdout)
        for entry in manifest["documents"]:
            fetch(entry, ROOT / "corpus/cache", offline=True)
            source = ROOT / "corpus/cache" / (entry["id"] + ".pdf")
            independent = facts(source)
            if independent is not None:
                inventory.append(dict(id=entry["id"], **independent))
            if entry["id"] not in ("w3c-complex-table", "w3c-rowspan-table"):
                continue
            row = dict(id=entry["id"], source_sha256=entry["sha256"], passed=False,
                       independent_source=independent)
            try:
                assert independent["root_fields"] == independent["canonical_fields"] == 0
                assert independent["field_type_dictionaries"] == independent["widget_dictionaries"] == independent["javascript_dictionaries"] == independent["page_annotations"] == 0
                assert not independent["has_xfa"]
                rewrite = directory / (entry["id"] + ".pdf")
                rewrite.write_bytes(source.read_bytes() + b"\n% Empty-shell representation-only rewrite\n")
                exit_code, diff = compare(binary, source, rewrite)
                coverage = diff["comparison"]
                assert coverage["verified_empty_acroform_shells"] == 2
                assert coverage["interactive_pages_requiring_review"] == coverage["canonical_fields_examined"] == coverage["widget_fields_examined"] == 0
                assert not coverage["canonical_form_review_reasons"]
                assert all(value == 0 for value in diff["summary"].values())
                assert all(p["visual"]["changed_pixels"] == 0 for p in diff["pages"])
                reading_review = entry["id"] == "w3c-rowspan-table"
                assert diff["diagnostics"].get("old_pages_with_uncertain_reading_order", 0) == int(reading_review)
                # Both original W3C sources contain independently enumerated SDK
                # objects with no character records (36/28). Shell removal must
                # preserve that separate uncertainty.
                expected_unobserved = 36 if entry["id"]=="w3c-complex-table" else 28
                ir_process=subprocess.run([str(binary),"--ir",str(source)],capture_output=True,check=True,timeout=120)
                source_ir=json.loads(ir_process.stdout)
                assert source_ir["pages"][0]["sdk_text_objects_without_characters"]==expected_unobserved
                assert diff["diagnostics"]["old_pages_with_unobserved_text_objects"]==1
                assert diff["diagnostics"]["new_pages_with_unobserved_text_objects"]==1
                assert exit_code == 3
                row.update(passed=True, exit_code=exit_code, summary=diff["summary"],
                           coverage=coverage, diagnostics=diff["diagnostics"],
                           inferred_reading_order_review_retained=reading_review)
                if args.baseline:
                    previous_exit, previous = compare(args.baseline.resolve(), source, rewrite)
                    assert previous_exit == 3
                    assert previous["summary"] == diff["summary"]
                    stripped={k:v for k,v in diff["diagnostics"].items() if not k.endswith("_with_unobserved_text_objects")}
                    assert previous["diagnostics"] == stripped
                    assert previous["comparison"]["interactive_pages_requiring_review"] == 1
                    row.update(baseline_exit_code=previous_exit,
                               baseline_coverage=previous["comparison"],
                               prior_summary_and_existing_diagnostics_unchanged=True,
                               source_text_uncertainty_added=True)
            except Exception as error:
                row["error"] = str(error)
            results.append(row)
            print(("PASS " if row["passed"] else "FAIL ") + entry["id"], flush=True)
    assert len(results) == 2
    report = dict(version=1, methodology=__doc__, manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
                  binary_sha256=binary_hash, tool_sha256=sha(Path(__file__)),
                  pypdf_version=pypdf_version, documents=2,
                  passed=sum(row["passed"] for row in results), results=results,
                  inspected_documents=len(manifest["documents"]), form_inventory=inventory)
    if args.baseline:
        report["baseline_binary_sha256"] = sha(args.baseline)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    return report["passed"] != 2


if __name__ == "__main__":
    raise SystemExit(main())
