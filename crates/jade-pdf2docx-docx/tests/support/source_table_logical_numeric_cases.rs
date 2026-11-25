//! 本文をWordの別行に分けても、元の数値群から確認できた右端を失わない。
use super::*;

/// 見出しを結合した既存対照の値列だけを、AFM幅が異なる三つの数値へ置き換える。
fn source(right_aligned: bool, inset: u16, reverse: bool) -> Vec<u8> {
    let pdf = String::from_utf8(super::source(3, 25, reverse, true)).unwrap();
    let start = pdf.find("\nstream\n").unwrap() + "\nstream\n".len();
    let end = start + pdf[start..].find("\nendstream").unwrap();
    let mut content = pdf[start..end].to_string();
    for (index, (text, em)) in [("9", 0.556), ("82", 1.112), ("1,234", 2.502)]
        .into_iter()
        .enumerate()
    {
        let y = 650 - index * 25;
        let x = if right_aligned {
            272.0 - f64::from(inset) - em * 10.0
        } else {
            182.0
        };
        let original = format!("BT /F1 10 Tf 182 {y} Td (Value-{index}) Tj ET");
        assert_eq!(content.matches(&original).count(), 1);
        content = content.replace(
            &original,
            &format!("BT /F1 10 Tf {x:.5} {y} Td ({text}) Tj ET"),
        );
    }
    owned_pdf_content(&content)
}

/// XML属性の順序によらず、原本で確認した右寄せと指定した右余白を数える。
fn right_anchors(xml: &str, inset: u16) -> (usize, usize) {
    let mut reader = Reader::from_str(xml);
    let mut alignments = 0;
    let mut insets = 0;
    let expected = (inset * 20).to_string();
    loop {
        match reader.read_event().unwrap() {
            Event::Empty(tag) => {
                let attributes = tag.attributes().map(Result::unwrap).collect::<Vec<_>>();
                if tag.name().as_ref() == b"w:jc"
                    && attributes
                        .iter()
                        .any(|a| a.key.as_ref() == b"w:val" && a.value.as_ref() == b"right")
                {
                    alignments += 1;
                }
                if tag.name().as_ref() == b"w:ind"
                    && attributes
                        .iter()
                        .any(|a| a.key.as_ref() == b"w:left" && a.value.as_ref() == b"0")
                    && attributes.iter().any(|a| {
                        a.key.as_ref() == b"w:right" && a.value.as_ref() == expected.as_bytes()
                    })
                {
                    insets += 1;
                }
            }
            Event::Eof => return (alignments, insets),
            _ => {}
        }
    }
}

/// 各行が単独になった後も、群で確認済みの右端へ数値を揃え、狭い左indentで折り返さない。
#[test]
fn numeric_group_right_anchor_survives_logical_row_split() {
    let source = source(true, 8, false);
    assert_eq!(table_rows(&source).len(), 4);
    assert_eq!(right_anchors(&document_xml(&source), 8), (3, 3));
}

/// 左端が揃う数値を、行分割に伴って右端へ動かしてはならない。
#[test]
fn left_aligned_numeric_group_does_not_gain_right_alignment() {
    let source = source(false, 8, false);
    assert_eq!(table_rows(&source).len(), 4);
    assert_eq!(right_anchors(&document_xml(&source), 8), (0, 0));
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 24,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/support/source_table_logical_numeric_cases.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 右余白や原本の描画順を変えても、論理行に分割した三つの数値は原本の右端を共有する。
    #[test]
    fn verified_numeric_anchor_survives_inset_and_paint_order(inset in 1u16..9, reverse in any::<bool>()) {
        let source = source(true, inset, reverse);
        prop_assert_eq!(table_rows(&source).len(), 4);
        prop_assert_eq!(right_anchors(&document_xml(&source), inset), (3, 3));
    }
}

/// 新規領域だけへ数値列の所有原本を保存し、file製品APIの前に全ページを確認する。
#[test]
#[ignore = "fresh JADE_LOGICAL_NUMERIC_EXPORT_DIR required; review all source pages before file conversion"]
fn export_owned_numeric_groups_before_file_product() {
    use std::io::Write;
    let dir =
        std::path::PathBuf::from(std::env::var_os("JADE_LOGICAL_NUMERIC_EXPORT_DIR").unwrap());
    std::fs::create_dir(&dir).unwrap();
    for (name, right) in [
        ("merged-header-right-numeric.pdf", true),
        ("merged-header-left-numeric.pdf", false),
    ] {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(name))
            .unwrap();
        file.write_all(&source(right, 8, false)).unwrap();
    }
}
