"""描画順・セルの値とは独立な、所有PDFの行座標を製品WASMへ渡す。"""


def owned_table_order_cases(make_pdf):
    """正式なPDF builderへ三行二列を描き、独立した座標順の期待値を返す。"""
    grid = ' '.join(
        [f'72 {y} m 272 {y} l S' for y in (580, 620, 660, 700)]
        + [f'{x} 580 m {x} 700 l S' for x in (72, 172, 272)]
    )
    cases = []
    for name, labels, order in [
        ('table-descending-values', ['9', '5', '1'], [0, 1, 2]),
        ('table-reverse-paint', ['Alpha', 'Beta', 'Gamma'], [2, 1, 0]),
    ]:
        controls = ['Top', 'Middle', 'Bottom']
        text = []
        for row in order:
            y = 680 - row * 40
            text.extend([
                f'BT /F1 10 Tf 82 {y} Td ({labels[row]}) Tj ET',
                f'BT /F1 10 Tf 182 {y} Td ({controls[row]}) Tj ET',
            ])
        cases.append(dict(
            id=name,
            data=make_pdf(grid + ' ' + ' '.join(text)),
            body={},
            rows=[[labels[row], controls[row]] for row in range(3)],
        ))
    return cases


def assert_owned_table_rows(document, expected):
    """実際に出力された単一表の行・列対応を、推測で並べ替えず検証する。"""
    ns = {'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
    tables = document.findall('.//w:tbl', ns)
    assert len(tables) == 1, len(tables)
    actual = [
        [''.join(t.text or '' for t in cell.findall('.//w:t', ns))
         for cell in row.findall('w:tc', ns)]
        for row in tables[0].findall('w:tr', ns)
    ]
    assert actual == expected, dict(expected=expected, actual=actual)
    return dict(source_coordinate_rows=expected, actual_rows=actual)
