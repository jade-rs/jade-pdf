"""既存の固定公開fixtureからToUnicodeだけを外し、描画用FontFileを保持する。"""
import argparse
import hashlib
import json
from pathlib import Path

from pypdf import PdfReader, PdfWriter


def generate(destination):
    """新規ディレクトリにだけ生成し、source・gold・FontFileの固定値を記録する。"""
    root = Path(__file__).resolve().parents[1]
    manifest_path = root / 'fixtures/manifest.json'
    manifest = json.loads(manifest_path.read_text())
    destination.mkdir(parents=True, exist_ok=False)
    rows = []
    for name in ['pdflatex-text', 'pdflatex-four-pages']:
        case = next(case for case in manifest['cases'] if case['name'] == name)
        source = root / 'fixtures' / case['input']
        gold = root / 'fixtures' / case['expected']
        assert hashlib.sha256(source.read_bytes()).hexdigest() == case['sha256']
        assert hashlib.sha256(gold.read_bytes()).hexdigest() == case['expected_sha256']
        reader = PdfReader(source)
        writer = PdfWriter(clone_from=reader)
        seen = set()
        removed = 0
        fonts = {}
        for page in writer.pages:
            for reference in page['/Resources']['/Font'].values():
                font = reference.get_object()
                if id(font) in seen:
                    continue
                seen.add(id(font))
                assert font['/Subtype'] == '/Type1'
                program = font['/FontDescriptor']['/FontFile'].get_data()
                assert b'SIL Open Font License, Version 1.1' in program
                fonts[str(font['/BaseFont'])] = hashlib.sha256(program).hexdigest()
                assert '/ToUnicode' in font
                del font['/ToUnicode']
                removed += 1
        assert removed == 1
        output = destination / f'{name}-without-ToUnicode.pdf'
        writer.write(output)
        check = PdfReader(output)
        for page in check.pages:
            for reference in page['/Resources']['/Font'].values():
                font = reference.get_object()
                assert '/ToUnicode' not in font
                program = font['/FontDescriptor']['/FontFile'].get_data()
                assert hashlib.sha256(program).hexdigest() == fonts[str(font['/BaseFont'])]
        rows.append(dict(name=name, source_sha256=case['sha256'], gold_sha256=case['expected_sha256'],
                         pages=len(check.pages), output=output.name, output_sha256=hashlib.sha256(output.read_bytes()).hexdigest(),
                         unchanged_FontFile_sha256=fonts, source_attribution=case['attribution'],
                         source_license=manifest['license'], embedded_font_license='SIL OFL 1.1; notice retained unchanged',
                         modification='Remove ToUnicode only; retain all drawing/font program bytes; rewrite objects and xref'))
    report = dict(pypdf_version=__import__('pypdf').__version__, source_manifest_sha256=hashlib.sha256(manifest_path.read_bytes()).hexdigest(), rows=rows)
    (destination / 'generation.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path, help='New ignored or temporary directory; must not exist')
    generate(parser.parse_args().destination)
