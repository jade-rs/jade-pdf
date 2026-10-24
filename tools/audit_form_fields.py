#!/usr/bin/env python3
"""Investigate AcroForm records on the pinned public corpus using pypdf.

EOF-comment rewrites must not invent changes. A separate controlled removal of
the catalog form and page annotations exposes source records as removals for
name/value inventory checks. This is not signature, action or XFA validation.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from pypdf import PdfReader, PdfWriter, __version__ as pypdf_version
from pypdf.generic import ArrayObject, BooleanObject, DictionaryObject, IndirectObject, NameObject, NullObject, NumberObject, TextStringObject

from pdf_corpus import ROOT, fetch


def value_oracle(value):
    if isinstance(value, NameObject) and str(value).isascii():
        return {'name_hex':str(value)[1:].encode('ascii').hex()}
    if isinstance(value, TextStringObject):
        return str(value)
    if isinstance(value, NullObject):
        return None
    if isinstance(value, BooleanObject):
        return value.value
    if isinstance(value, NumberObject):
        return int(value)
    if isinstance(value, ArrayObject):
        return [value_oracle(item) for item in value]
    raise ValueError('Value is outside this independent scalar/array oracle')


def catalog_fields(reader):
    """Prefer pypdf's convenience API; retain errors and use raw declarations
    for a source whose missing widget AP prevents that API from enumerating.
    The fallback reads fields, not radio appearance-state dictionaries.
    """
    try:
        return reader.get_fields() or {}, None
    except Exception as error:
        api_error = type(error).__name__ + ": " + str(error)
    form = reader.root_object.get("/AcroForm")
    if form is None or isinstance(form.get_object(), NullObject):
        return {}, api_error
    form = form.get_object()
    assert isinstance(form, DictionaryObject)
    inherited_keys = ("/FT", "/Ff", "/V", "/DV", "/DA", "/Q", "/MaxLen")
    property_keys = inherited_keys + ("/Opt", "/I", "/TU", "/TM", "/A", "/AA")
    records, seen = {}, set()
    steps = 0
    def resolve(value):
        nonlocal steps
        chain = set()
        while isinstance(value, IndirectObject):
            steps += 1
            key = (value.idnum, value.generation)
            assert len(chain) < 64 and key not in chain and steps <= 200000
            chain.add(key)
            value = value.get_object()
        return value
    def entry(dictionary, key):
        if key not in dictionary:
            return None
        value = resolve(dictionary.raw_get(key))
        return None if isinstance(value, NullObject) else value
    def walk(reference, parents, inherited, depth):
        nonlocal steps
        steps += 1
        assert steps <= 200000 and depth < 64
        if isinstance(reference, IndirectObject):
            key = (reference.idnum, reference.generation)
            assert key not in seen
            seen.add(key)
        field = resolve(reference)
        assert isinstance(field, DictionaryObject)
        name = entry(field, "/T")
        widget = entry(field, "/Subtype") == "/Widget"
        if widget and name is None and depth:
            for key in property_keys:
                child = entry(field, key)
                if child is None:
                    continue
                assert key in inherited_keys, "Unidentified unnamed widget property"
                parent = inherited.get(key, NumberObject(0) if key in ("/Ff","/Q") else None)
                assert parent is not None and value_oracle(child) == value_oracle(parent), "Conflicting unnamed widget"
            return
        names = parents + ([str(name)] if name is not None else [])
        effective = dict(inherited)
        for key in inherited_keys:
            value = entry(field, key)
            if value is not None:
                effective[key] = value
        if name is not None or effective:
            qualified = ".".join(names)
            assert qualified not in records, "Ambiguous qualified field name"
            record = dict(field)
            record.update(effective)
            records[qualified] = record
        kids = entry(field, "/Kids")
        if kids is not None:
            assert isinstance(kids, ArrayObject) and len(kids) <= 100000
            for kid in kids:
                walk(kid, names, effective, depth + 1)
    roots = entry(form, "/Fields")
    if roots is None:
        return {}, api_error
    assert isinstance(roots, ArrayObject) and len(roots) <= 100000
    inherited = {key: entry(form,key) for key in ("/DA","/Q") if entry(form,key) is not None}
    for root in roots:
        walk(root, [], inherited, 0)
    return records, api_error



def declared_fields(reader):
    """Keep the primary catalog oracle and supplement page Widget declarations.
    The catalog count/error stay observable; raw page records are not a proof
    that an incomplete inventory contains every logical field.
    """
    records, api_error = catalog_fields(reader)
    records = dict(records)
    inherited_keys = ("/FT", "/Ff", "/V", "/DV", "/DA", "/Q", "/MaxLen")
    steps = 0
    seen_widgets, recovered_names = set(), {}
    def resolve(value):
        nonlocal steps
        seen = set()
        while isinstance(value, IndirectObject):
            steps += 1
            identity = (value.idnum, value.generation)
            assert steps <= 200000 and len(seen) < 64 and identity not in seen
            seen.add(identity)
            value = value.get_object()
        return value
    def entry(d, key):
        value = resolve(d.raw_get(key)) if key in d else None
        return None if isinstance(value, NullObject) else value
    form = entry(reader.root_object, "/AcroForm")
    defaults = {key: entry(form, key) for key in ("/DA", "/Q") if form is not None and entry(form, key) is not None}
    for page in reader.pages:
        annotations = entry(page, "/Annots")
        if annotations is None:
            continue
        assert isinstance(annotations, ArrayObject) and len(annotations) <= 100000
        for reference in annotations:
            steps += 1
            assert steps <= 200000
            field = resolve(reference)
            assert isinstance(field, DictionaryObject)
            if entry(field, "/Subtype") != "/Widget" or id(field) in seen_widgets:
                continue
            seen_widgets.add(id(field))
            chain, seen = [], set()
            while field is not None:
                steps += 1
                assert steps <= 200000 and len(chain) < 64 and id(field) not in seen
                seen.add(id(field));chain.append(field)
                field = entry(field, "/Parent")
                assert field is None or isinstance(field, DictionaryObject)
            names, inherited = [], dict(defaults)
            for depth, field in enumerate(reversed(chain)):
                name = entry(field, "/T")
                if name is not None:
                    names.append(str(name))
                effective = dict(inherited)
                for key in inherited_keys:
                    value = entry(field, key)
                    if value is not None:
                        effective[key] = value
                control = entry(field, "/Subtype") == "/Widget" and name is None and (depth or "/FT" not in effective)
                if not control and (name is not None or effective):
                    qualified = ".".join(names)
                    assert qualified not in recovered_names or recovered_names[qualified] == id(field), "Ambiguous page-side field name"
                    if qualified not in records:
                        recovered_names[qualified] = id(field)
                        record = dict(field);record.update(effective);records[qualified] = record
                inherited = effective
    return records, api_error


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,default=ROOT/'target/release/pdf-diff')
    parser.add_argument('--report',type=Path,default=ROOT/'corpus/results-form-fields.json')
    args=parser.parse_args()
    manifest_bytes=(ROOT/'corpus/manifest.json').read_bytes()
    manifest=json.loads(manifest_bytes)
    results=[]
    with tempfile.TemporaryDirectory(prefix='jade-form-inventory-') as temporary:
        directory=Path(temporary)
        binary=directory/'pdf-diff'
        shutil.copy2(args.binary,binary)
        binary_hash=hashlib.sha256(binary.read_bytes()).hexdigest()
        def compare(old,new):
            process=subprocess.run([str(binary),str(old),str(new)],capture_output=True,text=True,timeout=60)
            if process.returncode not in [0,1,3]:
                raise RuntimeError(process.stderr[:2000])
            return json.loads(process.stdout)
        for entry in manifest['documents']:
            fetch(entry,ROOT/'corpus/cache',offline=True)
            source=ROOT/'corpus/cache'/(entry['id']+'.pdf')
            reader=PdfReader(source)
            expected, api_error=declared_fields(reader)
            if not expected:
                continue
            row={'id':entry['id'],'passed':False,'pypdf_field_records':len(expected),'has_xfa':bool(reader.root_object.get('/AcroForm', {}).get('/XFA')), 'pypdf_convenience_api_error':api_error, 'catalog_field_records':len(catalog_fields(reader)[0]),'reference_method':'pypdf catalog plus bounded raw page/Parent declarations'}
            try:
                gold=entry.get('canonical_field_gold')
                if gold is not None:
                    assert set(gold)==set(expected), 'Declared field gold names differ'
                    for name, properties in gold.items():
                        for key, value in properties.items():
                            assert value_oracle(expected[name]['/'+key])==value, f'Independent field gold differs: {name}/{key}'
                rewrite=directory/'rewrite.pdf'
                rewrite.write_bytes(source.read_bytes()+b'\n% representation-only field inventory\n')
                equal=compare(source,rewrite)
                assert not equal['canonical_form_changes'],'Phantom canonical changes'
                writer=PdfWriter(clone_from=source)
                writer.root_object.pop('/AcroForm', None)
                for page in writer.pages:
                    if '/Annots' in page:
                        del page['/Annots']
                removed=directory/'removed.pdf'
                writer.write(removed)
                probe=compare(source,removed)
                records={'.'.join(change['old']['name']):change['old'] for change in probe['canonical_form_changes']}
                assert all(change['status']=='removed' and change['new'] is None for change in probe['canonical_form_changes'])
                assert set(records)==set(expected),'Canonical/pypdf field names differ'
                checked=0
                for name,field in expected.items():
                    if '/V' not in field or 'V' in records[name]['unavailable_properties']:
                        continue
                    try:
                        value=value_oracle(field['/V'])
                    except ValueError:
                        continue
                    assert records[name]['properties'].get('V')==value,f'Saved value differs: {name}'
                    checked+=1
                coverage=equal['comparison']
                row.update(passed=True,canonical_records=len(records),declared_values_checked=checked,
                           unavailable_properties=coverage['canonical_properties_unavailable']//2,
                           review_reasons=coverage['canonical_form_review_reasons'])
            except Exception as error:
                row['error']=str(error)
            results.append(row)
            print(('PASS' if row['passed'] else 'FAIL')+' '+entry['id'],flush=True)
    report={'version':4,'binary_sha256':binary_hash,'manifest_sha256':hashlib.sha256(manifest_bytes).hexdigest(),
            'independent_parser':'pypdf '+pypdf_version,'methodology':__doc__,'documents':len(results),
            'matching_inventories':sum(r['passed'] for r in results),
            'declared_values_checked':sum(r.get('declared_values_checked',0) for r in results),'results':results}
    args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(f"{report['matching_inventories']}/{report['documents']} matched")
    return report['matching_inventories']!=report['documents']


if __name__=='__main__':
    raise SystemExit(main())
