#!/usr/bin/env python3
"""Evaluate real PDFs and controlled PDF edits. Requires pypdf==6.10.0.

Run pdf_corpus.py first. Results measure these controlled cases, not accuracy on
arbitrary revisions. Every PDF passed to the library goes through Pdfium.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import shutil
import tempfile
import time

from pypdf import PdfReader, PdfWriter
from pypdf.generic import ArrayObject, DecodedStreamObject, DictionaryObject, NameObject, RectangleObject, TextStringObject, NumberObject

from pdf_corpus import ROOT, fetch


def save(reader, path, transform=None):
    writer = PdfWriter()
    writer.clone_document_from_reader(reader)
    if transform:
        transform(writer)
    writer.write(path)
    return path


def overlay(writer, text, size=12, x=100, y=16):
    """Append a standard PDF text object without touching the source streams."""
    page = writer.pages[0]
    # These fixture dimensions are physical points, even on nondefault UserUnit pages.
    unit = float(page.get('/UserUnit', 1))
    size, x, y = size / unit, x / unit, y / unit
    resources = copy.copy(page.get('/Resources', DictionaryObject()).get_object())
    fonts = copy.copy(resources.get('/Font', DictionaryObject()).get_object())
    # Reserve a name that cannot overwrite an existing source font resource.
    name = '/JadeCorpusFont'
    while name in fonts:
        name += 'X'
    fonts[NameObject(name)] = writer._add_object(DictionaryObject({
        NameObject('/Type'): NameObject('/Font'),
        NameObject('/Subtype'): NameObject('/Type1'),
        NameObject('/BaseFont'): NameObject('/Helvetica'),
    }))
    resources[NameObject('/Font')] = fonts
    page[NameObject('/Resources')] = resources
    stream = DecodedStreamObject()
    stream.set_data(f'q BT {name} {size} Tf {x} {y} Td ({text}) Tj ET Q\n'.encode('ascii'))
    original = page.get('/Contents')
    contents = ArrayObject()
    if original is not None:
        if isinstance(original.get_object(), ArrayObject):
            contents.extend(original.get_object())
        else:
            contents.append(original)
    contents.append(writer._add_object(stream))
    page[NameObject('/Contents')] = contents


def insert_page(writer):
    # A blank insertion has a known ground truth even in image-only documents.
    page = writer.add_blank_page(width=612, height=792)
    insertion = len(writer.pages) // 2
    writer.insert_page(page, insertion)
    writer.remove_page(len(writer.pages) - 1)


def reverse_pages(writer):
    pages = list(writer.pages)
    for _ in pages:
        writer.remove_page(0)
    for page in reversed(pages):
        writer.add_page(page)


def crop_page(writer, offset):
    page = writer.pages[0]
    media, crop = page.mediabox, page.cropbox
    x0 = max(min(float(media.left), float(media.right)), min(float(crop.left), float(crop.right)))
    y0 = max(min(float(media.bottom), float(media.top)), min(float(crop.bottom), float(crop.top)))
    x1 = min(max(float(media.left), float(media.right)), max(float(crop.left), float(crop.right)))
    y1 = min(max(float(media.bottom), float(media.top)), max(float(crop.bottom), float(crop.top)))
    width, height = x1-x0, y1-y0
    assert width > 0 and height > 0
    page.cropbox = RectangleObject([x0+width*offset, y0+height*offset,
                                   x0+width*(offset+.7), y0+height*(offset+.7)])


def set_form_value(writer, name, value):
    def visit(reference, parent=''):
        field=reference.get_object()
        own=str(field.get('/T',''))
        qualified='.'.join(part for part in [parent,own] if part)
        if qualified==name and own:
            field[NameObject('/V')]=TextStringObject(value)
            if field.get('/FT')=='/Ch':
                options=[str(o[0] if isinstance(o,ArrayObject) else o) for o in field['/Opt']]
                field[NameObject('/I')]=ArrayObject([NumberObject(options.index(value))])
            return 1
        return sum(visit(child,qualified) for child in field.get('/Kids',[]))
    assert sum(visit(field) for field in writer.root_object['/AcroForm']['/Fields'])==1
    # Do not regenerate /AP: canonical values must be compared even when the
    # appearance is absent or stale.


def verify_form_value(path, name, expected):
    reader=PdfReader(path)
    assert reader.get_fields()[name]['/V']==expected
    found=False
    for page in reader.pages:
        for ref in page.get('/Annots',[]):
            field=ref.get_object(); parts=[]; value=None
            while field:
                if value is None and '/V' in field: value=field['/V']
                if '/T' in field: parts.insert(0,str(field['/T']))
                field=field['/Parent'].get_object() if '/Parent' in field else None
            if '.'.join(parts)==name:
                assert value==expected
                found=True
    assert found, 'Canonical field has no matching widget'


def field_dictionary(writer, name):
    found=[]
    def visit(reference, parent=''):
        field=reference.get_object()
        own=str(field.get('/T',''))
        qualified='.'.join(part for part in [parent,own] if part)
        if qualified==name and own:
            found.append(field)
        for child in field.get('/Kids',[]):
            visit(child,qualified)
    for field in writer.root_object['/AcroForm']['/Fields']:
        visit(field)
    assert len(found)==1
    return found[0]


def unplace_field(writer, name, value):
    field_dictionary(writer,name)[NameObject('/V')]=TextStringObject(value)
    for page in writer.pages:
        if '/Annots' in page:
            del page['/Annots']


def change_second_export(writer, revised):
    field=field_dictionary(writer,'Listbox_MultiSelectMultipleValues')
    assert list(field['/V'])==['Epsilon','Gamma']
    export='Gamma-export-change' if revised else 'Gamma'
    options=ArrayObject()
    for option in field['/Opt']:
        if str(option)=='Gamma':
            options.append(ArrayObject([TextStringObject(export),TextStringObject('Gamma')]))
        else:
            options.append(option)
    field[NameObject('/Opt')]=options
    field[NameObject('/V')]=ArrayObject([TextStringObject('Epsilon'),TextStringObject(export)])


def set_form_flags(writer, name, flags):
    def visit(reference, parent=''):
        field = reference.get_object()
        own = str(field.get('/T', ''))
        qualified = '.'.join(part for part in [parent, own] if part)
        if qualified == name and own:
            field[NameObject('/Ff')] = NumberObject(flags)
            return 1
        return sum(visit(child, qualified) for child in field.get('/Kids', []))
    assert sum(visit(field) for field in writer.root_object['/AcroForm']['/Fields']) == 1


def effective_widget_flags(path, name):
    reader = PdfReader(path)
    canonical = int(reader.get_fields()[name].get('/Ff', 0))
    found = []
    for page in reader.pages:
        for reference in page.get('/Annots', []):
            field = reference.get_object()
            parts, flags, seen = [], None, set()
            while field:
                identity = id(field)
                assert identity not in seen, 'Cyclic widget parent tree'
                seen.add(identity)
                if flags is None and '/Ff' in field:
                    flags = int(field['/Ff'])
                if '/T' in field:
                    parts.insert(0, str(field['/T']))
                field = field['/Parent'].get_object() if '/Parent' in field else None
            if '.'.join(parts) == name:
                found.append(flags if flags is not None else 0)
    assert found and all(flags == canonical for flags in found)
    return canonical


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/pdf-diff')
    parser.add_argument('--report', type=Path, default=ROOT / 'corpus/results.json')
    parser.add_argument('--variants-dir', type=Path, default=ROOT / 'corpus/variants')
    parser.add_argument('--only', nargs='*', help='evaluate selected corpus IDs')
    parser.add_argument('--crop-only', action='store_true', help='run only the shifted CropBox regression')
    args = parser.parse_args()
    # Cargo may rebuild while an evaluation is running. Execute one immutable
    # snapshot for every case, and record the hash of exactly that executable.
    snapshot_directory = tempfile.TemporaryDirectory(prefix='jade-pdf-evaluation-')
    binary = Path(snapshot_directory.name) / 'pdf-diff'
    shutil.copy2(args.binary, binary)
    manifest_path = ROOT / 'corpus/manifest.json'
    manifest = json.loads(manifest_path.read_text())
    if args.only is not None:
        unknown = set(args.only) - {entry['id'] for entry in manifest['documents']}
        if not args.only or unknown:
            parser.error('--only must name at least one known corpus ID; unknown: ' + ', '.join(sorted(unknown)))
    directory = args.variants_dir
    directory.mkdir(parents=True, exist_ok=True)
    results = []
    documents = []

    def check(name, old, new, predicate):
        if args.crop_only and not name.endswith('/crop-origin'):
            return
        start = time.monotonic()
        try:
            completed = subprocess.run(
                [str(binary), str(old), str(new)],
                capture_output=True, text=True, timeout=60,
            )
            if completed.returncode not in (0, 1, 3):
                raise RuntimeError(completed.stderr[:2000])
            result = json.loads(completed.stdout)
            passed = bool(predicate(result))
            results.append(dict(
                case=name, passed=passed, seconds=round(time.monotonic()-start, 4),
                exit_code=completed.returncode, summary=result['summary'],
                diagnostics=result['diagnostics'],
            ))
        except Exception as error:
            results.append(dict(case=name, passed=False, error=str(error)))
        print(('PASS' if results[-1]['passed'] else 'FAIL') + ' ' + name, flush=True)

    def unchanged(result):
        return all(p['status'] == 'unchanged' for p in result['pages']) and all(
            value == 0 for value in result['summary'].values()
        )

    def check_crop(doc_id, reader):
        before = save(reader, directory / (doc_id + '-crop-old.pdf'), lambda w: crop_page(w, .1))
        after = save(reader, directory / (doc_id + '-crop-new.pdf'), lambda w: crop_page(w, .15))
        check(doc_id + '/crop-origin', before, after, lambda r:
              r['summary']['pages_geometry_changed'] == 1
              and r['summary']['pages_modified'] == 1
              and r['summary']['pages_added'] == 0 and r['summary']['pages_removed'] == 0
              and r['summary']['blocks_changed'] == 0
              and r['summary']['blocks_format_changed'] == 0)

    for entry in manifest['documents']:
        doc_id = entry['id']
        if args.only is not None and doc_id not in args.only:
            continue
        # Do not evaluate tampered or stale caches as the pinned corpus.
        fetch(entry, ROOT / 'corpus/cache', offline=True)
        original = ROOT / 'corpus/cache' / (doc_id + '.pdf')
        reader = PdfReader(original)
        assert len(reader.pages) == entry['pages'], doc_id
        documents.append(dict(id=doc_id, pages=len(reader.pages)))
        if args.crop_only:
            check_crop(doc_id, reader)
            continue
        check(doc_id + '/identity', original, original, unchanged)
        form_cases={
            'pdfium-combobox-form':('Combo1','Apple','Banana'),
            'pdfium-listbox-form':('Listbox_SingleSelect','foo','bar'),
            'pdfium-text-form-multiline':('Text Box','Before value','After value'),
        }
        if doc_id in form_cases:
            name,old_value,new_value=form_cases[doc_id]
            before=save(reader,directory/(doc_id+'-field-old.pdf'),lambda w:set_form_value(w,name,old_value))
            after=save(reader,directory/(doc_id+'-field-new.pdf'),lambda w:set_form_value(w,name,new_value))
            verify_form_value(before,name,old_value);verify_form_value(after,name,new_value)
            check(doc_id+'/form-value',before,after,lambda r:
                  r['summary']['form_fields_changed']==1 and any(
                      f.get('old') and f.get('new') and f['old']['name']==name
                      and f['old']['value']==old_value and f['new']['value']==new_value
                      for p in r['pages'] for f in p['form_changes']))
            # Toggle only the read-only bit, preserving other field flags.
            original_flags = effective_widget_flags(before, name)
            flag_before = save(PdfReader(before), directory/(doc_id+'-flags-old.pdf'),
                               lambda w: set_form_flags(w, name, original_flags & ~1))
            flag_after = save(PdfReader(before), directory/(doc_id+'-flags-new.pdf'),
                              lambda w: set_form_flags(w, name, original_flags | 1))
            assert effective_widget_flags(flag_before, name) == original_flags & ~1
            assert effective_widget_flags(flag_after, name) == original_flags | 1
            check(doc_id+'/form-flags', flag_before, flag_after, lambda r:
                  r['summary']['form_fields_changed'] == 1 and any(
                      f['old'] and f['new'] and f['new']['name'] == name
                      and f['old']['value'] == f['new']['value']
                      and f['old']['flags'] ^ f['new']['flags'] == 1
                      for p in r['pages'] for f in p['form_changes']))
        if doc_id=='pdfjs-user-unit':
            changed=save(reader,directory/(doc_id+'-unit-change.pdf'),
                         lambda w:w.pages[0].__setitem__(NameObject('/UserUnit'),NumberObject(36)))
            assert float(PdfReader(original).pages[0]['/UserUnit'])==72
            assert float(PdfReader(changed).pages[0]['/UserUnit'])==36
            check(doc_id+'/physical-unit',original,changed,lambda r:
                  r['summary']['pages_geometry_changed']==1 and r['summary']['pages_visual_changed']==1
                  and r['summary']['blocks_changed']==0 and r['summary']['blocks_format_changed']==0
                  and r['pages'][0]['old_size']['user_unit']==72 and r['pages'][0]['new_size']['user_unit']==36
                  and r['pages'][0]['visual']['old_viewport']['width']==1224
                  and r['pages'][0]['visual']['new_viewport']['width']==612)
        if doc_id=='pdfium-text-form-multiline':
            name='Text Box'
            before=save(reader,directory/(doc_id+'-unplaced-old.pdf'),lambda w:unplace_field(w,name,'Original invisible field'))
            after=save(reader,directory/(doc_id+'-unplaced-new.pdf'),lambda w:unplace_field(w,name,'Changed invisible field'))
            for path,value in [(before,'Original invisible field'),(after,'Changed invisible field')]:
                oracle=PdfReader(path)
                assert oracle.get_fields()[name]['/V']==value
                assert all(not p.get('/Annots') for p in oracle.pages)
            check(doc_id+'/unplaced-field',before,after,lambda r:
                  r['summary']['canonical_form_fields_changed']==1 and r['summary']['form_fields_changed']==0
                  and r['summary']['pages_visual_changed']==0 and any(
                      f['old'] and f['new'] and f['old']['properties']['V']=='Original invisible field'
                      and f['new']['properties']['V']=='Changed invisible field'
                      for f in r['canonical_form_changes']))
        if doc_id=='pdfjs-annotation-text-widget':
            name='formulier1[0].#subform[0].Tekstveld1[0]'
            before=save(reader,directory/(doc_id+'-hybrid-old.pdf'),lambda w:unplace_field(w,name,'Old hybrid value'))
            after=save(reader,directory/(doc_id+'-hybrid-new.pdf'),lambda w:unplace_field(w,name,'New hybrid value'))
            for path,value in [(before,'Old hybrid value'),(after,'New hybrid value')]:
                oracle=PdfReader(path)
                assert oracle.root_object['/AcroForm'].get('/XFA') is not None
                assert oracle.get_fields()[name]['/V']==value
                assert all(not p.get('/Annots') for p in oracle.pages)
            check(doc_id+'/hybrid-field',before,after,lambda r:
                  r['summary']['canonical_form_fields_changed']==1 and r['summary']['form_fields_changed']==0
                  and r['summary']['pages_visual_changed']==0
                  and any('XFA semantics' in reason for reason in r['comparison']['canonical_form_review_reasons'])
                  and any(f['old'] and f['new'] and f['old']['properties']['V']=='Old hybrid value'
                          and f['new']['properties']['V']=='New hybrid value' for f in r['canonical_form_changes']))
        if doc_id=='pdfium-listbox-form':
            before=save(reader,directory/(doc_id+'-multi-export-old.pdf'),lambda w:change_second_export(w,False))
            after=save(reader,directory/(doc_id+'-multi-export-new.pdf'),lambda w:change_second_export(w,True))
            for path,values in [(before,['Epsilon','Gamma']),(after,['Epsilon','Gamma-export-change'])]:
                oracle=PdfReader(path).get_fields()['Listbox_MultiSelectMultipleValues']
                assert list(oracle['/V'])==values and oracle['/Opt'][2][1]=='Gamma'
            check(doc_id+'/second-export',before,after,lambda r:
                  r['summary']['canonical_form_fields_changed']==1 and r['summary']['form_fields_changed']==0
                  and r['summary']['pages_visual_changed']==0 and any(
                      f['old'] and f['new'] and f['old']['properties']['V']==['Epsilon','Gamma']
                      and f['new']['properties']['V']==['Epsilon','Gamma-export-change']
                      for f in r['canonical_form_changes']))
        rewritten = save(reader, directory / (doc_id + '-rewrite.pdf'))
        check(doc_id + '/rewrite', original, rewritten, unchanged)
        inserted = save(reader, directory / (doc_id + '-insert.pdf'), insert_page)
        check(doc_id + '/insert', original, inserted, lambda r:
              r['summary']['pages_added'] == 1 and r['summary']['pages_removed'] == 0
              and r['summary']['pages_modified'] == 0
              and r['summary']['pages_geometry_changed'] == 0
              and r['summary']['blocks_changed'] == 0)
        # Reverse comparison verifies removal without an independent mutation path.
        check(doc_id + '/delete', inserted, original, lambda r:
              r['summary']['pages_removed'] == 1 and r['summary']['pages_added'] == 0
              and r['summary']['pages_modified'] == 0
              and r['summary']['blocks_changed'] == 0)
        if len(reader.pages) > 1:
            reordered = save(reader, directory / (doc_id + '-reorder.pdf'), reverse_pages)
            check(doc_id + '/reorder', original, reordered, lambda r:
                  r['summary']['pages_added'] == 0 and r['summary']['pages_removed'] == 0
                  and r['summary']['pages_modified'] == 0
                  and r['summary']['blocks_changed'] == 0
                  and r['summary']['blocks_format_changed'] == 0)
        rotated = save(reader, directory / (doc_id + '-rotate.pdf'),
                       lambda w: w.pages[0].rotate(90))
        check(doc_id + '/rotate', original, rotated, lambda r:
              r['summary']['pages_geometry_changed'] == 1
              and r['summary']['pages_modified'] == 1
              and r['summary']['pages_added'] == 0
              and r['summary']['pages_removed'] == 0
              and r['summary']['blocks_changed'] == 0
              and r['summary']['blocks_format_changed'] == 0)
        check_crop(doc_id, reader)
        before = save(reader, directory / (doc_id + '-amount-old.pdf'),
                      lambda w: overlay(w, 'Total: $100'))
        after = save(reader, directory / (doc_id + '-amount-new.pdf'),
                     lambda w: overlay(w, 'Total: $200'))
        check(doc_id + '/footer-amount', before, after, lambda r:
              r['summary']['pages_modified'] == 1 and r['summary']['blocks_changed'] >= 1
              and any(b['old_text'] and '100' in b['old_text'] and b['new_text']
                      and '200' in b['new_text'] and not b['minor']
                      for p in r['pages'] for b in p['blocks']))
        # Glyphs 1 and 2 have different ink bounds at the SAME font size.
        before = save(reader, directory / (doc_id + '-glyph-old.pdf'),
                      lambda w: overlay(w, '111111', size=24))
        after = save(reader, directory / (doc_id + '-glyph-new.pdf'),
                     lambda w: overlay(w, '222222', size=24))
        check(doc_id + '/same-font', before, after, lambda r:
              r['summary']['pages_modified'] == 1 and r['summary']['blocks_changed'] >= 1
              and r['summary']['blocks_format_changed'] == 0)
        after = save(reader, directory / (doc_id + '-font-new.pdf'),
                     lambda w: overlay(w, '111111', size=30))
        check(doc_id + '/font-size', before, after, lambda r:
              r['summary']['pages_modified'] == 1 and r['summary']['blocks_format_changed'] >= 1)

    for pair in manifest.get('revision_pairs', []):
        if args.only is not None and not {pair['old'], pair['new']}.issubset(args.only):
            continue
        old = ROOT / 'corpus/cache' / (pair['old'] + '.pdf')
        new = ROOT / 'corpus/cache' / (pair['new'] + '.pdf')
        # These are selected, manually verified changes from the official editions,
        # not an exhaustive annotation of every changed glyph or layout difference.
        check(pair['id'] + '/page-alignment', old, new, lambda r:
              [[p['old_index'], p['new_index']] for p in r['pages']] == pair['page_pairs'])
        for change in pair['changes']:
            def found(result, change=change):
                old_page = change.get('old_page', change.get('page'))
                new_page = change.get('new_page', change.get('page'))
                old_blocks = [b for p in result['pages'] if p['old_index'] == old_page
                              for b in p['blocks'] if b['kind'] != 'unchanged' and not b['minor']]
                new_blocks = [b for p in result['pages'] if p['new_index'] == new_page
                              for b in p['blocks'] if b['kind'] != 'unchanged' and not b['minor']]
                deleted = ' '.join(b['old_text'] or '' for b in old_blocks)
                inserted = ' '.join(b['new_text'] or '' for b in new_blocks)
                return change['old'] in deleted and change['new'] in inserted
            check(pair['id'] + '/' + change['old'] + '-to-' + change['new'], old, new, found)
        for side, name in [('new', 'added'), ('old', 'removed')]:
            for index, change in enumerate(pair.get(name, [])):
                def found(result, change=change, side=side):
                    blocks = [b for p in result['pages'] if p[side+'_index'] == change['page']
                              for b in p['blocks'] if b['kind'] != 'unchanged' and not b['minor']]
                    text = ' '.join(b[side+'_text'] or '' for b in blocks)
                    opposite = 'old' if side == 'new' else 'new'
                    opposite_text = ' '.join(
                        b[opposite+'_text'] or '' for p in result['pages']
                        if p[opposite+'_index'] == change['page'] for b in p['blocks'])
                    return change['text'] in text and change['text'] not in opposite_text
                check(pair['id'] + '/' + name + '-clause-' + str(index+1), old, new, found)
        for index, unchanged_text in enumerate(pair.get('unchanged', [])):
            check(pair['id'] + '/unchanged-clause-' + str(index+1), old, new,
                  lambda r, text=unchanged_text: any(
                      b['old_text'] and text in b['old_text'] and b['old_text'] == b['new_text']
                      for p in r['pages'] for b in p['blocks']))

    passed = sum(r['passed'] for r in results)
    report = dict(
        version=1, manifest_sha256=hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
        binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
        methodology='Controlled PDF transformations; not natural-revision precision/recall.',
        documents=documents, cases=len(results), passed=passed, failed=len(results)-passed,
        results=results,
    )
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(f'{passed}/{len(results)} passed; report: {args.report}')
    snapshot_directory.cleanup()
    return passed != len(results)


if __name__ == '__main__':
    sys.exit(main())
