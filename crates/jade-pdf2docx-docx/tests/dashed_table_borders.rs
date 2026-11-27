use std::io::{Cursor, Read};

use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;

mod support;

/// 表の中央に重なりのない破線を置くと、その辺だけをWordの破線罫線にする。
#[test]
fn dashed_middle_rule_becomes_a_word_cell_border() {
    let content = "0.5 w 20 160 m 180 160 l S 20 100 m 180 100 l S
        20 100 m 20 160 l S 100 100 m 100 160 l S 180 100 m 180 160 l S
        q [2 2] 0 d 20 130 m 180 130 l S Q
        BT /F1 10 Tf 1 0 0 1 25 146 Tm (Head) Tj ET
        BT /F1 10 Tf 1 0 0 1 105 146 Tm (Value) Tj ET
        BT /F1 10 Tf 1 0 0 1 25 116 Tm (Alpha) Tj ET
        BT /F1 10 Tf 1 0 0 1 105 116 Tm (Beta) Tj ET";
    for (solid, expected) in [("", true), ("20 130 m 180 130 l S", false)] {
        let source = support::pdf_builders::pdf(&format!("{content} {solid}"), true);
        let bytes = convert_minimum_pdf_bytes_to_docx_bytes("dashed.pdf", source.as_bytes())
            .expect("表を変換できる");
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        zip.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("<w:tbl>"), "{xml}");
        assert_eq!(
            xml.contains("<w:bottom w:val=\"dashed\""),
            expected,
            "{xml}"
        );
        assert!(xml.contains("<w:top w:val=\"single\""), "{xml}");
    }
}
