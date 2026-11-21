#!/usr/bin/env python3
"""原文のRTL順序と接続形を検証し、既存コーパスの本文・位置の保持を照合する。"""
import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import unicodedata

from pypdf import PdfReader, __version__ as pypdf_version
from pypdf.generic import ByteStringObject
from pdf_corpus import ROOT, fetch
from audit_text_objects import document, stream


def digest(path):
    """検証に使用した不変の入力と実行成果物を識別する。"""
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run(binary, *arguments, codes=(0,)):
    """隔離CLIの結果を、期待した終了コードとともに取得する。"""
    p = subprocess.run([str(binary), *map(str, arguments)], capture_output=True, timeout=180)
    assert p.returncode in codes, (arguments, p.returncode, p.stderr[-2000:])
    return p.returncode, json.loads(p.stdout)


def spelling(text, unit=1, rotation=0, shear=False):
    """原文の文字コード一個へ、明示的なUnicode列を割り当てる。"""
    target = text.encode('utf-16-be').hex().upper()
    transform = '1 .2 .3 1 0 0 cm ' if shear else ''
    return document([
        '<< /Type /Catalog /Pages 2 0 R >>',
        '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Rotate {rotation} /UserUnit {unit} /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
        '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>',
        stream(transform+'BT /F1 12 Tf 3 Tr 30 150 Td (A) Tj ET'),
        stream('/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /IndependentRTL def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfchar <41> <'+target+'> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end'),
    ])


def body(page):
    """文字・位置・順序を含む、元のページ本文全体を取り出す。"""
    return {k: v for k, v in page.items() if k in ('index','width','height','rotation','view_box','user_unit','blocks')}


def original_arabic(source, before, after):
    """独立パーサーで原本の対応表を読み、全ページの正解と照合する。"""
    page = PdfReader(source).pages[0]
    mappings = {}
    for name, font in page['/Resources']['/Font'].items():
        font = font.get_object()
        if '/ToUnicode' not in font:
            continue
        data = font['/ToUnicode'].get_data().decode('ascii')
        pairs = {}
        for section in re.findall(r'beginbfchar(.*?)endbfchar', data, re.S):
            for code, target in re.findall(r'<([0-9a-fA-F]+)>\s*<([0-9a-fA-F]+)>', section):
                pairs[int(code, 16)] = bytes.fromhex(target).decode('utf-16-be')
        assert len(pairs) == 13
        mappings[name] = pairs
    current, declared, blanks = None, [], 0
    for args, op in page.get_contents().operations:
        if op == b'Tf':
            current = args[0]
        elif op in (b'Tj', b'TJ'):
            values = args[0] if op == b'TJ' else [args[0]]
            for value in values:
                if not isinstance(value, (str, bytes)):
                    continue
                raw = bytes(value) if isinstance(value, ByteStringObject) else value.original_bytes
                if current not in mappings:
                    assert raw and set(raw) == {32}
                    blanks += len(raw)
                    continue
                assert len(raw) % 2 == 0
                declared.extend(mappings[current][int.from_bytes(raw[i:i+2], 'big')] for i in range(0,len(raw),2))
    source_text = ''.join(declared)
    extracted = '\n'.join(block['text'] for block in after['pages'][0]['blocks'])
    expected_line = 'انواع الخطوط العربية'
    assert [b['text'] for b in after['pages'][0]['blocks']] == [expected_line]*4
    def bag(text, compatibility):
        result = Counter()
        for c in text:
            value = ord(c)
            use_compat = compatibility and (0xfb50 <= value <= 0xfdff or 0xfe70 <= value <= 0xfeff or 0x0675 <= value <= 0x0678)
            for out in unicodedata.normalize('NFKD' if use_compat else 'NFD', c):
                if not out.isspace() and not unicodedata.category(out).startswith('C'):
                    result[out] += 1
        return result
    previous_source, sdk = bag(source_text, False), bag(extracted, False)
    discrepancy = sum((previous_source-sdk).values()) + sum((sdk-previous_source).values())
    assert discrepancy == before['pages'][0]['source_unicode_scalars_unmatched'] == 92
    assert bag(source_text, True) == sdk
    assert after['pages'][0].get('source_unicode_scalars_unmatched', 0) == 0
    return dict(source_sha256=digest(source), font_mappings={k: {f'{code:04X}': value for code,value in table.items()} for k,table in mappings.items()},
                original_scalar_discrepancies=92, current_scalar_discrepancies=0,
                source_blank_codes=blanks, full_page_text=extracted,
                gold_scope='Four visible complete lines on the original page; glyphs and positions unchanged. SDK omissions and other review gates remain.')


def main():
    """既存資料・独立原文・人工制御を混同せず、評価結果を保存する。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    binary, baseline = args.binary.resolve(), args.baseline.resolve()
    manifest = json.loads((ROOT/'corpus/manifest.json').read_text())
    def inspect(entry):
        fetch(entry, ROOT/'corpus/cache', offline=True)
        path = ROOT/'corpus/cache'/(entry['id']+'.pdf')
        _, before = run(baseline, '--ir', path)
        _, after = run(binary, '--ir', path)
        assert len(before['pages']) == len(after['pages']) == entry['pages']
        changes = []
        for old, new in zip(before['pages'], after['pages']):
            assert body(old) == body(new), (entry['id'], new['index'], 'original text/order/geometry changed')
            delta = {k: [old.get(k,0),new.get(k,0)] for k in old.keys() | new.keys() if old.get(k,0) != new.get(k,0)}
            assert set(delta) <= {'source_unicode_scalars_unmatched','reading_order_requires_review'}, (entry['id'], delta)
            if delta: changes.append(dict(page=new['index'], changes=delta))
        evidence = original_arabic(path, before, after) if entry['id'] == 'arabic' else None
        print('PASS '+entry['id'], flush=True)
        return dict(id=entry['id'], pages=entry['pages'], source_sha256=entry['sha256'], original_body_exact=True, changes=changes, arabic_evidence=evidence)
    with ThreadPoolExecutor(max_workers=4) as pool:
        rows = list(pool.map(inspect, manifest['documents']))
    controls = []
    with tempfile.TemporaryDirectory(prefix='jade-rtl-controls-') as directory:
        directory = Path(directory)
        for label, original, equivalent, changed in [
            ('initial', '\ufe91', 'ب', 'ت'), ('ligature', '\ufccb', 'لخ', 'لح'),
            ('lam-alef', '\ufefb', 'لا', 'لب'), ('allah', '\ufdf2', 'الله', 'اللة'),
            ('hebrew', 'של', 'של', 'שם'),
            ('mixed-latin', 'بA', 'بA', 'بB'),
            ('mixed-digit', 'ب1', 'ب1', 'ب2'),
            ('mixed-mark', 'بَ', 'بَ', 'تَ'),
            ('mixed-composed', 'بé', 'بé', 'تé'),
            ('hebrew-mark', 'שׁל', 'שׁל', 'שׁם')]:
            paths = []
            for index, value in enumerate((original, equivalent, changed)):
                path = directory/f'{label}-{index}.pdf'; path.write_bytes(spelling(value)); paths.append(path)
            previous, old_diff = run(baseline, paths[0], paths[1], codes=(0,1,3))
            code, equal = run(binary, paths[0], paths[1], codes=(0,))
            edit, edited = run(binary, paths[0], paths[2], codes=(1,))
            assert not any(equal['summary'].values())
            assert edited['summary']['blocks_changed'] == 1
            baseline_texts = []
            for path, expected in zip(paths, (equivalent,equivalent,changed)):
                _, baseline_ir = run(baseline, '--ir', path)
                baseline_texts.append(baseline_ir['pages'][0]['blocks'][0]['text'])
                _, ir = run(binary, '--ir', path)
                assert ir['pages'][0]['blocks'][0]['text'] == expected
                _, layer = run(binary, '--text-layer', path, 1)
                assert ''.join(r['text'] for r in layer) == expected
            controls.append(dict(id=label, source_sha256=[digest(p) for p in paths], expected=equivalent,
                                 baseline_exit=previous, current_exit=code, real_edit_exit=edit,
                                 baseline_summary=old_diff['summary'], current_summary=equal['summary'], baseline_texts=baseline_texts,
                                 current_texts=[equivalent,equivalent,changed]))
    report = dict(version=1, tool_sha256=digest(__file__), baseline_sha256=digest(baseline), binary_sha256=digest(binary),
                  manifest_sha256=digest(ROOT/'corpus/manifest.json'), pypdf_version=pypdf_version,
                  unicode_version=unicodedata.unidata_version, documents=len(rows), pages=sum(r['pages'] for r in rows),
                  original_body_exact_documents=sum(r['original_body_exact'] for r in rows),
                  changed_documents=sum(bool(r['changes']) for r in rows), controls=controls, results=rows,
                  scope='Selected source and SDK glyph ownership profile; not exhaustive Unicode, reading-order, natural-revision or production qualification.')
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
    print(json.dumps({key:report[key] for key in ('documents','pages','original_body_exact_documents','changed_documents')},ensure_ascii=False))


if __name__ == '__main__':
    main()
