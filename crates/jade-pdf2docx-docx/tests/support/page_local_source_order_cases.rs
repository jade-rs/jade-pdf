//! 描画operatorの順序だけを変えても、反復列の物理的な行順を変えない回帰。
use super::*;

/// 元の各文字・座標・fontを保ち、borderlessページの三行の描画順だけを並べ替える。
fn source_with_paint_row_order(
    pages: &[bool],
    left: u16,
    width: u16,
    order: [usize; 3],
) -> Vec<u8> {
    let mut sorted = order;
    sorted.sort();
    assert_eq!(sorted, [0, 1, 2]);
    let mut pdf = String::from_utf8(source(pages, left, width)).unwrap();
    let original_len = pdf.len();
    let marker = "\nstream\n";
    let (start, end) = pdf
        .match_indices(marker)
        .find_map(|(offset, _)| {
            let start = offset + marker.len();
            let end = start + pdf[start..].find("\nendstream")?;
            pdf[start..end]
                .contains("GRID-R0C0")
                .then_some((start, end))
        })
        .unwrap();
    let lines = pdf[start..end].split_inclusive('\n').collect::<Vec<_>>();
    let drawings = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("BT ") && line.contains("(GRID-"))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(drawings.len(), 6);
    let mut reordered = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    for (destination, original) in order.into_iter().enumerate() {
        for column in 0..2 {
            reordered[drawings[destination * 2 + column]] =
                lines[drawings[original * 2 + column]].to_string();
        }
    }
    let content = reordered.concat();
    assert_eq!(content.len(), end - start);
    pdf.replace_range(start..end, &content);
    assert_eq!(pdf.len(), original_len); // Lengthとxref offsetを保つ。
    pdf.into_bytes()
}

/// 行を下から描くPDFでも、表の出力は原本の上から下の行順を保つ。
#[test]
fn mixed_tables_follow_physical_rows_in_every_paint_order() {
    for pages in [[true, false], [false, true]] {
        assert_eq!(
            source_with_paint_row_order(&pages, 40, 240, [0, 1, 2]),
            source(&pages, 40, 240)
        );
        for order in [
            [2, 1, 0],
            [2, 0, 1],
            [1, 2, 0],
            [1, 0, 2],
            [0, 2, 1],
            [0, 1, 2],
        ] {
            let xml = document_xml(&source_with_paint_row_order(&pages, 40, 240, order));
            assert_eq!(
                tables_in_xml(&xml),
                pages.into_iter().map(expected).collect::<Vec<_>>(),
                "paint order {order:?}, pages {pages:?}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 原点・列間隔・ページ順・描画順を独立に変えても、表の行の所属と順序を保つ。
    #[test]
    fn physical_rows_are_independent_of_page_and_paint_order(
        left in 20u16..80,
        width in 160u16..250,
        ruled_first in any::<bool>(),
        priorities in any::<[u8;3]>(),
    ) {
        let pages = [ruled_first, !ruled_first];
        let mut order = [0, 1, 2];
        order.sort_by_key(|index| priorities[*index]);
        let xml = document_xml(&source_with_paint_row_order(&pages, left, width, order));
        prop_assert_eq!(tables_in_xml(&xml), pages.into_iter().map(expected).collect::<Vec<_>>());
    }
}

/// 元の座標・字面と表示順のgoldを確認するため、新規領域だけへ二つの所有原本を保存する。
#[test]
#[ignore = "new JADE_PHYSICAL_TABLE_EXPORT_DIR required; review all source pages before file conversion"]
fn export_owned_reversed_paint_order_sources_before_file_product() {
    let dir = std::path::PathBuf::from(std::env::var_os("JADE_PHYSICAL_TABLE_EXPORT_DIR").unwrap());
    std::fs::create_dir(&dir).unwrap();
    for (name, pages) in [
        ("ruled-then-bottom-up-grid", [true, false]),
        ("bottom-up-grid-then-ruled", [false, true]),
    ] {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(format!("{name}.pdf")))
            .unwrap();
        std::io::Write::write_all(
            &mut file,
            &source_with_paint_row_order(&pages, 40, 240, [2, 1, 0]),
        )
        .unwrap();
    }
}
