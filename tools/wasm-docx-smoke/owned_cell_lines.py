"""所有PDFの複数行セルを、実際の製品WASM byte APIで照合する。"""


def owned_cell_lines(make_pdf):
    """独立した原本座標・文字サイズ・明示空白を持つ一行二列の表を作る。"""
    grid = ' '.join(
        [f'72 {y} m 272 {y} l S' for y in (600, 700)]
        + [f'{x} 600 m {x} 700 l S' for x in (72, 172, 272)]
    )
    text = ('BT /F1 10 Tf 82 680 Td (    Alpha  ) Tj ET '
            'BT /F1 14 Tf 92 656 Td (Beta) Tj ET '
            'BT /F1 10 Tf 182 680 Td (Control) Tj ET')
    return dict(id='native-cell-lines', data=make_pdf(grid + ' ' + text),
                body={}, cell_lines=True)


def assert_cell_lines(document):
    """第一セルの表示行・runサイズと、隣の単一行セルの互換配置を照合する。"""
    ns = {'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
    w = '{' + ns['w'] + '}'
    tables = document.findall('.//w:tbl', ns)
    assert len(tables) == 1, len(tables)
    cells = tables[0].findall('w:tr/w:tc', ns)
    assert len(cells) == 2, len(cells)
    paragraphs = cells[0].findall('w:p', ns)
    actual = [''.join(t.text or '' for t in p.findall('.//w:t', ns))
              for p in paragraphs]
    assert actual == ['    Alpha  ', 'Beta'], actual
    sizes = [[int(s.attrib[w + 'val']) for s in p.findall('.//w:sz', ns)]
             for p in paragraphs]
    assert sizes == [[20], [28]], sizes
    spacing = [p.find('w:pPr/w:spacing', ns).attrib for p in paragraphs]
    assert all(s[w + 'lineRule'] == 'exact' for s in spacing), spacing
    assert int(spacing[0][w + 'line']) > 0
    assert ''.join(t.text or '' for t in cells[1].findall('.//w:t', ns)) == 'Control'
    assert cells[1].find('w:tcPr/w:vAlign', ns).attrib[w + 'val'] == 'center'
    return dict(source_lines=actual, font_half_points=sizes,
                single_line_control='unchanged vertically centered')


def owned_cell_row_heights(make_pdf):
    """40ptと100ptの所有表を作り、平均70ptでは切れる本文行を含める。"""
    grid = ' '.join(
        [f'72 {y} m 272 {y} l S' for y in (600, 700, 740)]
        + [f'{x} 600 m {x} 740 l S' for x in (72, 172, 272)]
    )
    text = []
    for label, y in [('Top', 728), ('Bottom', 688)]:
        text.extend([
            f'BT /F1 10 Tf 82 {y} Td ({label}) Tj ET',
            f'BT /F1 10 Tf 82 {y - 16} Td (Second) Tj ET',
            f'BT /F1 10 Tf 182 {y} Td (Control) Tj ET',
        ])
    return dict(id='native-cell-row-heights',
                data=make_pdf(grid + ' ' + ' '.join(text)),
                body={}, row_heights=[800, 2000])
