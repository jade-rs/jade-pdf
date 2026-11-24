//! 表セルの原本の表示行と行間・run styleを、単一の再flow段落へ潰さない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 正式xref付きの一行二列の表へ、異なる高さの二つの本文行を描く。
fn source(gap: u32, second_size: u32) -> Vec<u8> {
    source_with_padding(gap, second_size, "", "")
}

/// 原本の行頭・行末に明示的な空白を含め、字面と描画原点を区別する。
fn source_with_padding(gap: u32, second_size: u32, prefix: &str, suffix: &str) -> Vec<u8> {
    let content = format!(
        "1 w 72 600 m 272 600 l S 72 700 m 272 700 l S 72 600 m 72 700 l S 172 600 m 172 700 l S 272 600 m 272 700 l S BT /F1 10 Tf 82 680 Td ({prefix}Alpha{suffix}) Tj ET BT /F1 {second_size} Tf 92 {} Td (Beta) Tj ET BT /F1 10 Tf 182 680 Td (Control) Tj ET",
        680 - gap,
    );
    owned_pdf_content(&content)
}

/// 描画命令列から、正式xrefと一つのHelvetica resourceを持つ所有PDFを作る。
fn owned_pdf_content(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// 製品byte APIが出力した、最初の表セルの段落XMLを取り出す。
fn first_cell(gap: u32, second_size: u32) -> String {
    first_cell_from_source(&source(gap, second_size))
}

/// 指定した所有原本の最初の表セルXMLを、製品byte API経由で読む。
fn first_cell_from_source(source: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-cell-lines.pdf", source).unwrap();
    let xml = document_xml(bytes);
    let start = xml.find("<w:tc>").unwrap();
    let end = start + xml[start..].find("</w:tc>").unwrap();
    xml[start..end].to_string()
}

/// 製品DOCXの本文XMLを、閉じたarchiveから読む。
fn document_xml(bytes: Vec<u8>) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 段落ごとの本文・行box・前余白・font sizeを、属性順に依存せず読む。
fn paragraphs(xml: &str) -> Vec<(String, u32, u32, Vec<u32>)> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut current = (String::new(), 0, 0, Vec::new());
    let mut in_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:p" => {
                current = (String::new(), 0, 0, Vec::new())
            }
            Event::Start(tag) if tag.name().as_ref() == b"w:t" => in_text = true,
            Event::End(tag) if tag.name().as_ref() == b"w:t" => in_text = false,
            Event::End(tag) if tag.name().as_ref() == b"w:p" => out.push(current.clone()),
            Event::Text(text) if in_text => current.0.push_str(&text.decode().unwrap()),
            Event::Empty(tag) if tag.name().as_ref() == b"w:spacing" => {
                for a in tag.attributes() {
                    let a = a.unwrap();
                    if a.key.as_ref() == b"w:line" {
                        current.1 = std::str::from_utf8(&a.value).unwrap().parse().unwrap();
                    }
                    if a.key.as_ref() == b"w:before" {
                        current.2 = std::str::from_utf8(&a.value).unwrap().parse().unwrap();
                    }
                }
            }
            Event::Empty(tag) if tag.name().as_ref() == b"w:sz" => {
                for a in tag.attributes() {
                    let a = a.unwrap();
                    if a.key.as_ref() == b"w:val" {
                        current
                            .3
                            .push(std::str::from_utf8(&a.value).unwrap().parse().unwrap());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

/// 原本二行を二段落に保ち、セル全体の共通fontへ均さない。
#[test]
fn authored_cell_lines_and_font_sizes_remain_distinct() {
    let p = paragraphs(&first_cell(24, 14));
    assert_eq!(
        p.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(),
        ["Alpha", "Beta"]
    );
    assert_eq!(p[0].3, [20]);
    assert_eq!(p[1].3, [28]);
}

/// 行頭空白をtrimしても描画原点だけが残り、文字を左へ移動させてはならない。
#[test]
fn authored_cell_edge_spaces_are_not_trimmed() {
    let p = paragraphs(&first_cell_from_source(&source_with_padding(
        24, 10, "    ", "  ",
    )));
    assert_eq!(p[0].0, "    Alpha  ");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_table_text_lines.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 独立に指定したbaseline間隔は、同一fontの段落行boxと前余白の和に残る。
    #[test]
    fn source_cell_line_pitch_survives(gap in 16u32..40) {
        let p = paragraphs(&first_cell(gap, 10));
        prop_assert_eq!(p.len(), 2);
        prop_assert_eq!(p[0].1 + p[1].2, gap * 20);
    }

    /// 原本の先頭・末尾の空白数を、run境界の正規化で減らさない。
    #[test]
    fn source_cell_edge_space_counts_survive(leading in 0usize..9, trailing in 0usize..5) {
        let prefix = " ".repeat(leading);
        let suffix = " ".repeat(trailing);
        let p = paragraphs(&first_cell_from_source(&source_with_padding(24, 10, &prefix, &suffix)));
        prop_assert_eq!(&p[0].0, &format!("{prefix}Alpha{suffix}"));
    }
}

/// 原本の二つの行高を平均せず、上から下の対応を維持する所有PDFを作る。
fn unequal_row_source(top: u32, bottom: u32) -> Vec<u8> {
    let high = 600 + bottom + top;
    let middle = 600 + bottom;
    let mut content = format!(
        "1 w 72 600 m 272 600 l S 72 {middle} m 272 {middle} l S 72 {high} m 272 {high} l S 72 600 m 72 {high} l S 172 600 m 172 {high} l S 272 600 m 272 {high} l S"
    );
    for (label, y) in [("Top", high - 12), ("Bottom", middle - 12)] {
        content.push_str(&format!(" BT /F1 10 Tf 82 {y} Td ({label}) Tj ET BT /F1 10 Tf 82 {} Td (Second) Tj ET BT /F1 10 Tf 182 {y} Td (Control) Tj ET", y - 16));
    }
    owned_pdf_content(&content)
}

/// 製品byte APIの実際の行高属性を、XML属性順に依存せず読む。
fn row_heights(source: &[u8]) -> Vec<u32> {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("unequal-cell-rows.pdf", source).unwrap();
    let xml = document_xml(bytes);
    let mut reader = Reader::from_str(&xml);
    let mut heights = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Empty(tag) if tag.name().as_ref() == b"w:trHeight" => {
                for attr in tag.attributes() {
                    let attr = attr.unwrap();
                    if attr.key.as_ref() == b"w:val" {
                        heights.push(std::str::from_utf8(&attr.value).unwrap().parse().unwrap());
                    }
                }
            }
            Event::Eof => return heights,
            _ => {}
        }
    }
}

/// 行高が40ptと100ptの原本を、各70ptへ潰すと複数行の後半が切れる。
#[test]
fn unequal_source_row_heights_are_not_averaged() {
    assert_eq!(row_heights(&unequal_row_source(40, 100)), [800, 2000]);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_table_geometry.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 独立に変えた各行高は、総高や本文の値にかかわらず対応する行へ残る。
    #[test]
    fn each_source_row_height_survives(top in 32u32..96, bottom in 100u32..180) {
        prop_assert_eq!(row_heights(&unequal_row_source(top, bottom)), [top * 20, bottom * 20]);
    }
}

/// 原本の重なる字面を、セル高を超える段落boxの積み重ねへ変換しない所有PDFを作る。
fn overfull_cell_source(gap: u32, second_size: u32) -> Vec<u8> {
    owned_pdf_content(&format!(
        "1 w 72 670 m 272 670 l S 72 700 m 272 700 l S 72 670 m 72 700 l S 172 670 m 172 700 l S 272 670 m 272 700 l S BT /F1 10 Tf 82 680 Td (Alpha) Tj ET BT /F1 {second_size} Tf 92 {} Td (Beta) Tj ET BT /F1 10 Tf 182 680 Td (Control) Tj ET",
        680 - gap,
    ))
}

/// 二つの行boxと原本上余白が30ptを超えるなら、編集可能な旧セル本文を保持する。
#[test]
fn cell_line_boxes_exceeding_source_height_keep_legacy_layout() {
    let p = paragraphs(&first_cell_from_source(&overfull_cell_source(6, 14)));
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].0, "Alpha Beta");
}

/// 行boxが収まらず旧配置へ戻っても、照合済みの原本runの文字サイズを失わない。
#[test]
fn overfull_source_cell_keeps_native_run_sizes_in_legacy_layout() {
    let p = paragraphs(&first_cell_from_source(&overfull_cell_source(6, 14)));
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].0, "Alpha Beta");
    assert_eq!(
        p[0].3,
        [20, 28],
        "overfull source cell must retain its exact native run sizes"
    );
    assert_eq!(
        p[0].1, 280,
        "fallback line box must fit its largest source run"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_table_fit.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 重なる原本字面の間隔やem高を変えても、原本セルより高い段落列を公開しない。
    #[test]
    fn overfull_cell_line_boxes_keep_all_text(gap in 5u32..9, second_size in 14u32..25) {
        let p = paragraphs(&first_cell_from_source(&overfull_cell_source(gap, second_size)));
        prop_assert_eq!(p.len(), 1);
        prop_assert_eq!(&p[0].0, "Alpha Beta");
        prop_assert_eq!(&p[0].3, &[20, second_size * 2]);
        prop_assert_eq!(p[0].1, second_size * 20);
    }
}

/// 表全体で文字サイズが異なる二つの単一行セルを、正式な所有PDFとして作る。
fn mixed_single_line_cells(small: u32, large: u32) -> Vec<u8> {
    owned_pdf_content(&format!(
        "1 w 72 600 m 272 600 l S 72 640 m 272 640 l S 72 600 m 72 640 l S 172 600 m 172 640 l S 272 600 m 272 640 l S BT /F1 {small} Tf 82 620 Td (Small) Tj ET BT /F1 {large} Tf 182 620 Td (Large) Tj ET"
    ))
}

/// 製品APIの実際のセル段落から、独立に指定したサイズをラベルで照合する。
fn single_line_sizes(small: u32, large: u32) -> (Vec<u32>, Vec<u32>) {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-mixed-single-line.pdf",
        &mixed_single_line_cells(small, large),
    )
    .unwrap();
    let p = paragraphs(&document_xml(bytes));
    (
        p.iter().find(|p| p.0 == "Small").unwrap().3.clone(),
        p.iter().find(|p| p.0 == "Large").unwrap().3.clone(),
    )
}

/// サイズの混在する表で既定の11ptへ戻すと、小さい原本行で本文が切れる。
#[test]
fn single_line_cells_keep_their_source_run_sizes() {
    assert_eq!(single_line_sizes(6, 14), (vec![12], vec![28]));
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_single_line_styles.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// セルごとに変えたem高が、表全体の共通サイズや既定値に置き換わらない。
    #[test]
    fn single_line_cell_font_sizes_are_independent(small in 6u32..11, large in 12u32..21) {
        prop_assert_eq!(single_line_sizes(small, large), (vec![small * 2], vec![large * 2]));
    }
}

/// サイズの混在する単一行セルの行boxを、段落markの既定fontから独立して読む。
fn single_line_box_heights(small: u32, large: u32) -> (u32, u32) {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-single-line-boxes.pdf",
        &mixed_single_line_cells(small, large),
    )
    .unwrap();
    let p = paragraphs(&document_xml(bytes));
    (
        p.iter().find(|p| p.0 == "Small").unwrap().1,
        p.iter().find(|p| p.0 == "Large").unwrap().1,
    )
}

/// runが6ptでも段落markの既定11ptが自動行高へ残ると、短い原本行からはみ出す。
#[test]
fn single_line_cell_line_boxes_use_source_em_heights() {
    assert_eq!(single_line_box_heights(6, 14), (120, 280));
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_single_line_styles.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 既存の保存済み6pt/12pt反例と独立なem高で、単一行の行boxを検証する。
    #[test]
    fn single_line_cell_line_boxes_are_independent(small in 6u32..11, large in 12u32..21) {
        prop_assert_eq!(single_line_box_heights(small, large), (small * 20, large * 20));
    }
}

/// 行boxが収まらない所有表で、同じ字形の上付き・下付き文字を別の原本位置に描く。
fn overfull_script_source(rise: u32) -> Vec<u8> {
    overfull_script_source_with_underline(rise, "")
}

/// 表gridの端点へ届かない独立した下線を、原本の本文へ追加できる。
fn overfull_script_source_with_underline(rise: u32, underline: &str) -> Vec<u8> {
    owned_pdf_content(&format!(
        "{underline} 1 w 72 660 m 272 660 l S 72 700 m 272 700 l S 72 660 m 72 700 l S 172 660 m 172 700 l S 272 660 m 272 700 l S BT /F1 10 Tf 82 680 Td (Alpha) Tj ET BT /F1 5 Tf 110 {} Td (2) Tj ET BT /F1 14 Tf 92 674 Td (Beta) Tj ET BT /F1 10 Tf 82 665 Td (Gamma) Tj ET BT /F1 5 Tf 122 {} Td (2) Tj ET BT /F1 10 Tf 182 680 Td (Control) Tj ET",
        680 + rise,
        665 - rise,
    ))
}

/// 同じ添字文字を持つ各runから、文字サイズと符号付き位置指定を独立に読む。
fn script_run_positions(xml: &str) -> Vec<(Option<u32>, Option<i32>)> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut text = String::new();
    let mut size = None;
    let mut position = None;
    let mut in_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:r" => {
                text.clear();
                size = None;
                position = None;
            }
            Event::Start(tag) if tag.name().as_ref() == b"w:t" => in_text = true,
            Event::End(tag) if tag.name().as_ref() == b"w:t" => in_text = false,
            Event::Text(value) if in_text => text.push_str(&value.decode().unwrap()),
            Event::Empty(tag) if tag.name().as_ref() == b"w:sz" => {
                size = tag
                    .try_get_attribute(b"w:val")
                    .unwrap()
                    .map(|a| std::str::from_utf8(&a.value).unwrap().parse().unwrap());
            }
            Event::Empty(tag) if tag.name().as_ref() == b"w:position" => {
                position = tag
                    .try_get_attribute(b"w:val")
                    .unwrap()
                    .map(|a| std::str::from_utf8(&a.value).unwrap().parse().unwrap());
            }
            Event::End(tag) if tag.name().as_ref() == b"w:r" && text.trim() == "2" => {
                out.push((size, position));
            }
            Event::Eof => return out,
            _ => {}
        }
    }
}

/// 旧配置の一段落でも、同じ5pt文字の上付きと下付きの基線差を失わない。
#[test]
fn overfull_source_scripts_keep_independent_baseline_positions() {
    let cell = first_cell_from_source(&overfull_script_source(3));
    let p = paragraphs(&cell);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].0, "Alpha 2 Beta Gamma 2");
    assert_eq!(
        script_run_positions(&cell),
        [(Some(10), Some(6)), (Some(10), Some(-6))],
        "overfull source scripts must retain signed native baseline offsets"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_table_text_lines.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 同じ字形・font・サイズの二つの断片が互いの上下位置を借用しない。
    #[test]
    fn overfull_source_script_positions_follow_each_fragment(rise in 2u32..=3) {
        let cell = first_cell_from_source(&overfull_script_source(rise));
        let expected = [(Some(10), Some((rise * 2) as i32)), (Some(10), Some(-((rise * 2) as i32)))];
        prop_assert_eq!(script_run_positions(&cell), expected);
        prop_assert!(underlined_run_texts(&cell).is_empty());
    }
}

/// 原本の表下辺は文字の下線ではなく、旧配置の書式復元でも下線を新設しない。
#[test]
fn overfull_source_grid_border_does_not_become_run_underline() {
    let cell = first_cell_from_source(&overfull_script_source(3));
    assert!(
        !cell.contains("<w:u "),
        "verified source grid border must not become restored run underline: {cell}"
    );
}

/// 表罫線を除外しても、gridの端点へ届かない独立した本文下線は保持する。
#[test]
fn overfull_source_keeps_independent_text_underline() {
    let cell = first_cell_from_source(&overfull_script_source_with_underline(
        3,
        "0.5 w 83 664 m 116 664 l S",
    ));
    assert_eq!(underlined_run_texts(&cell), ["Gamma"]);
    assert_eq!(paragraphs(&cell)[0].0, "Alpha 2 Beta Gamma 2");
}

/// 下線の有無を本文断片へ対応させ、隣のrunに付いた別の下線で成功しないよう読む。
fn underlined_run_texts(xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut text = String::new();
    let mut underlined = false;
    let mut in_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:r" => {
                text.clear();
                underlined = false;
            }
            Event::Start(tag) if tag.name().as_ref() == b"w:t" => in_text = true,
            Event::End(tag) if tag.name().as_ref() == b"w:t" => in_text = false,
            Event::Text(value) if in_text => text.push_str(&value.decode().unwrap()),
            Event::Empty(tag) if tag.name().as_ref() == b"w:u" => {
                underlined = tag
                    .try_get_attribute(b"w:val")
                    .unwrap()
                    .is_some_and(|a| a.value.as_ref() != b"none");
            }
            Event::End(tag) if tag.name().as_ref() == b"w:r" && underlined => {
                out.push(text.trim().to_string());
            }
            Event::Eof => return out,
            _ => {}
        }
    }
}
