#!/usr/bin/env python3
"""Qualify marked ActualText using original operators and public SDK identities.

Scope signatures are matched as multisets, never by source/SDK object ordinal.
Selected zero-ink source anchors additionally use an independent text-matrix
interpreter. This does not certify general reading order or glyph positions.
"""
import argparse
import concurrent.futures
import ctypes as C
import json
from collections import Counter
from pathlib import Path
from pypdf import PdfReader, __version__ as pypdf_version
from pypdf.generic import ContentStream, create_string_object
from audit_actual_text import DIAGNOSTICS, minimum, payload, run, sdk, sha, structure
from audit_form_discovery import library_path
from pdf_corpus import ROOT, fetch
from source_anchor_witness import declarations


def source_scopes(reader, page, ids):
    scopes = Counter()

    def visit(content, resources, context=(), active=False, structural=False, depth=0):
        assert depth <= 32
        if content is None:
            return
        stack = []
        for args, op in ContentStream(content, reader).operations:
            if op in (b'BMC', b'BDC'):
                stack.append((context, active, structural))
                if op == b'BDC':
                    d = args[1]
                    d = d if isinstance(d, dict) else resources['/Properties'][d].get_object()
                    if '/MCID' in d:
                        context = (*context, int(d['/MCID']))
                        structural |= int(d['/MCID']) in ids
                    if '/ActualText' in d:
                        assert not active and not structural, 'ambiguous marked scope'
                        text = d['/ActualText']
                        assert isinstance(text, str), 'undecodable ActualText'
                        if text and len(text.original_bytes) <= 4096:
                            scopes[(context, str(text))] += 1
                        active = True
            elif op == b'EMC':
                context, active, structural = stack.pop()
            elif op == b'Do':
                obj = resources['/XObject'][args[0]].get_object()
                if obj['/Subtype'] == '/Form':
                    visit(obj, obj.get('/Resources', resources), context, active, structural, depth + 1)
        assert not stack
    visit(page.get_contents(), page.get('/Resources', {}))
    return scopes


def sdk_scopes(library, path, page_number, expected):
    """Opaque equality only while page lives; no SDK private layouts."""
    b = C.CDLL(str(library))
    P, I, U = C.c_void_p, C.c_int, C.c_ulong

    def api(name, ret, args):
        f = getattr(b, name)
        f.restype, f.argtypes = ret, args
        return f
    init = api('FPDF_InitLibrary', None, [])
    end = api('FPDF_DestroyLibrary', None, [])
    load = api('FPDF_LoadMemDocument64', P, [P, C.c_size_t, C.c_char_p])
    close = api('FPDF_CloseDocument', None, [P])
    pagefn = api('FPDF_LoadPage', P, [P, I])
    closepage = api('FPDF_ClosePage', None, [P])
    objects = api('FPDFPage_CountObjects', I, [P])
    objfn = api('FPDFPage_GetObject', P, [P, I])
    kind = api('FPDFPageObj_GetType', I, [P])
    forms = api('FPDFFormObj_CountObjects', I, [P])
    formobj = api('FPDFFormObj_GetObject', P, [P, U])
    marks = api('FPDFPageObj_CountMarks', I, [P])
    markfn = api('FPDFPageObj_GetMark', P, [P, U])
    mcidfn = api('FPDFPageObjMark_GetParamIntValue', I, [P, C.c_char_p, C.POINTER(I)])
    typ = api('FPDFPageObjMark_GetParamValueType', I, [P, C.c_char_p])
    blob = api('FPDFPageObjMark_GetParamBlobValue', I, [P, C.c_char_p, P, U, C.POINTER(U)])
    textfn = api('FPDFText_LoadPage', P, [P])
    textclose = api('FPDFText_ClosePage', None, [P])
    count = api('FPDFText_CountChars', I, [P])
    charobj = api('FPDFText_GetTextObject', P, [P, I])
    data = Path(path).read_bytes()
    buf = C.create_string_buffer(data)
    init()
    doc = page = text = None
    groups, observed, object_owner = {}, set(), {}
    serial = 0
    try:
        doc = load(buf, len(data), None)
        assert doc
        page = pagefn(doc, page_number - 1)
        assert page

        def walk(obj, inherited=(), namespace=0, depth=0):
            nonlocal serial
            assert obj and depth <= 32
            n = marks(obj)
            assert 0 <= n <= 64
            own = [markfn(obj, j) for j in range(n)]
            assert all(own)
            prefix = len(own) >= len(inherited) and own[:len(inherited)] == [h for h, _ in inherited]
            context = list(inherited) + [(h, namespace) for h in own[len(inherited) if prefix else 0:]]
            ids, active = [], []
            for handle, owner in context:
                ident = I()
                if mcidfn(handle, b'MCID', C.byref(ident)):
                    ids.append(ident.value)
                if not typ(handle, b'ActualText'):
                    continue
                length = U()
                assert blob(handle, b'ActualText', None, 0, C.byref(length))
                if length.value > 4096:
                    continue
                raw, actual = C.create_string_buffer(length.value), U()
                assert blob(handle, b'ActualText', raw, length.value, C.byref(actual)) and actual.value == length.value
                value = create_string_object(raw.raw)
                assert isinstance(value, str)
                if not value:
                    continue
                key, signature = (owner, handle), (tuple(ids), str(value))
                group = groups.setdefault(key, dict(signature=signature, objects=set()))
                assert group['signature'] == signature
                if kind(obj) == 1:
                    group['objects'].add(obj)
                    assert obj not in object_owner or object_owner[obj] == key
                    object_owner[obj] = key
                active.append(key)
            assert len(active) <= 1, 'nested SDK ActualText'
            if kind(obj) == 5:
                serial += 1
                child_namespace = serial
                n = forms(obj)
                assert 0 <= n <= 200_000
                for j in range(n):
                    walk(formobj(obj, j), context, child_namespace, depth + 1)
        n = objects(page)
        assert 0 <= n <= 200_000
        for j in range(n):
            walk(objfn(page, j))
        actual = Counter(g['signature'] for g in groups.values())
        assert actual == expected, (page_number, expected - actual, actual - expected)
        text = textfn(page)
        assert text
        n = count(text)
        assert 0 <= n <= 2_000_000
        observed = {charobj(text, j) for j in range(n)}
        # Whitespace scopes already belonged to the v34 diagnostic exemption.
        covered = set().union(*(g['objects'] for g in groups.values() if g['signature'][1].strip()))
        missing = covered - observed
        return dict(page=page_number, scopes=sum(actual.values()),
                    signatures=[dict(mcids=k[0], text=k[1], count=v) for k, v in sorted(actual.items())],
                    objects_per_scope=dict(Counter(len(g['objects']) for g in groups.values())),
                    newly_represented_unobserved=len(missing), signature_multiset_exact=True)
    finally:
        if text: textclose(text)
        if page: closepage(page)
        if doc: close(doc)
        end()


def anchor_gold(reader, page, layer):
    source = declarations(reader, page)
    assert len(source) == 323 and all(len(s['anchors']) == 1 for s in source)
    candidates = [(s['text'], *s['anchors'][0]) for s in source if s['text'].strip()]
    errors = []
    for run_ in layer:
        for region in run_.get('source_regions', []):
            text = run_['text'][region['start']:region['end']]
            if not region.get('anchor') or not text.strip():
                continue
            rect = region['bounds']
            assert rect['x0'] == rect['x1'] and rect['y0'] == rect['y1']
            choices = [(i, max(abs(x - rect['x0']), abs(y - rect['y0'])))
                       for i, (s, x, y) in enumerate(candidates) if s == text]
            index, error = min(choices, key=lambda x: x[1])
            assert error < .01
            errors.append(error)
            candidates.pop(index)
    assert len(errors) == 238
    return dict(source_scopes=len(source), verified_nonwhite_anchors=len(errors),
                tolerance_points=.01, max_coordinate_error_points=max(errors),
                matching='text and original text-matrix coordinate multiset, without object ordinal')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('binary', 'baseline', 'report'):
        parser.add_argument('--' + name, required=True, type=Path)
    args = parser.parse_args()
    manifest_path = ROOT / 'corpus/manifest.json'
    manifest = json.loads(manifest_path.read_bytes())
    library, library_sha = library_path()

    def inspect(entry):
        fetch(entry, ROOT / 'corpus/cache', offline=True)
        path = ROOT / 'corpus/cache' / (entry['id'] + '.pdf')
        _, before = run(args.baseline, '--ir', path)
        _, after = run(args.binary, '--ir', path)
        exact = payload(before) == payload(after)
        assert exact or entry['id'] == 'pdfjs-actualtext-eset', entry['id']
        changes = []
        for a, b_ in zip(before['pages'], after['pages']):
            delta = {k: [a.get(k, 0), b_.get(k, 0)] for k in DIAGNOSTICS if a.get(k, 0) != b_.get(k, 0)}
            old = Counter(c for r in a['blocks'] for c in r['text'] if not c.isspace())
            new = Counter(c for r in b_['blocks'] for c in r['text'] if not c.isspace())
            expected = Counter({'.': 238}) if entry['id'] == 'pdfjs-actualtext-eset' and b_['index'] == 2 else Counter()
            assert new - old == expected and not old - new, (entry['id'], b_['index'], new - old, old - new)
            if delta or a['blocks'] != b_['blocks']:
                changes.append(dict(page=b_['index'], diagnostics=delta, blocks_changed=a['blocks'] != b_['blocks'],
                                    added_scalars=dict(new - old), removed_scalars=dict(old - new)))
        return dict(id=entry['id'], source_sha256=entry['sha256'], pages=after['pages'],
                    retained_payload=exact, changes=changes)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        rows = list(pool.map(inspect, manifest['documents']))
    inventories, checks, scopes, anchors = [], [], [], []
    for entry, row in zip(manifest['documents'], rows):
        path = ROOT / 'corpus/cache' / (entry['id'] + '.pdf')
        reader = PdfReader(path)
        structural = structure(reader)
        raw = sdk(library, path, [structural.get(p.indirect_reference.idnum, {}) for p in reader.pages])
        for page, facts, observed in zip(reader.pages, row['pages'], raw):
            # Structural replacements have a separate independently validated owner.
            ids = structural.get(page.indirect_reference.idnum, {})
            declared = source_scopes(reader, page, ids) if not ids else Counter()
            proof = None
            if declared:
                proof = sdk_scopes(library, path, observed['page'], declared)
                scopes.append(dict(id=entry['id'], **proof))
                _, layer = run(args.binary, '--text-layer', path, observed['page'])
                actual = Counter()
                for run_ in layer:
                    regions = run_.get('source_regions', [])
                    previous = 0
                    for region in regions:
                        assert previous <= region['start'] < region['end'] <= len(run_['text'])
                        previous = region['end']
                        text = run_['text'][region['start']:region['end']]
                        if text.strip(): actual[text] += 1
                expected = Counter()
                for (_, text), n in declared.items():
                    if text.strip(): expected[text] += n
                assert actual == expected, (entry['id'], observed['page'], actual - expected, expected - actual)
                if entry['id'] == 'pdfjs-actualtext-eset' and observed['page'] == 2:
                    assert ''.join(b['text'] for b in facts['blocks']).count('.') == 301
                    anchors.append(dict(id=entry['id'], page=2, **anchor_gold(reader, page, layer)))
            adjusted = observed['unobserved'] - (proof['newly_represented_unobserved'] if proof else 0)
            assert adjusted >= 0
            observed = dict(observed, unobserved=adjusted, legacy_unobserved=observed['unobserved'])
            if float(page.get('/UserUnit', 1)) != 1:
                continue
            assert adjusted == facts.get('sdk_text_objects_without_characters', 0), (entry['id'], observed, facts.get('sdk_text_objects_without_characters', 0))
            inventories.append(dict(id=entry['id'], **observed))
            if facts.get('source_text_accounting_unavailable'):
                continue
            try:
                lower = minimum(reader, page, structural)
            except (AssertionError, KeyError, ValueError, IndexError):
                continue
            expected = max(0, lower - observed['outside_replacements'])
            assert expected == facts.get('source_text_characters_missing', 0), (entry['id'], observed['page'], expected, facts.get('source_text_characters_missing', 0))
            checks.append(dict(id=entry['id'], page=observed['page'], minimum=lower,
                               sdk_outside=observed['outside_replacements'], deficit=expected))
    result = dict(version=1, tool_sha256=sha(__file__), anchor_witness_sha256=sha(Path(__file__).with_name('source_anchor_witness.py')),
                  manifest_sha256=sha(manifest_path), binary_sha256=sha(args.binary),
                  baseline_sha256=sha(args.baseline), sdk_library_sha256=library_sha, pypdf_version=pypdf_version,
                  documents=len(rows), pages=sum(len(r['pages']) for r in rows),
                  retained_original_payloads=sum(r['retained_payload'] for r in rows),
                  independent_source_count_checks=len(checks), independent_object_inventory_checks=len(inventories),
                  source_count_checks=checks, sdk_inventories=inventories, marked_scopes=scopes,
                  source_anchor_golds=anchors, rows=rows,
                  methodology=__doc__)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({k:v for k,v in result.items() if isinstance(v,int)}), flush=True)


if __name__ == '__main__':
    main()
