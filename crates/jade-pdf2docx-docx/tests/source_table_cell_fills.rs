//! 原本の矩形塗りを、罫線や文字色と区別して編集可能な表セルへ対応させる。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 青い見出し二セルと黄色の数値セルを持つ、所有する正式PDFを構成する。
fn source() -> Vec<u8> {
    source_with(
        ".2 .4 .6 rg 30 330 100 30 re f 1 1 0 rg 80 300 50 30 re f",
        "",
        "",
    )
}

/// 塗りの描画順・色・合成設定を変え、同じ所有gridへ対応付ける。
fn source_with(before: &str, after: &str, resources: &str) -> Vec<u8> {
    source_with_rule(before, after, resources, 1.)
}

/// 原本の罫線幅も指定し、中心線の丸めと内側の塗りの隙間を検査する。
fn source_with_rule(before: &str, after: &str, resources: &str, width: f32) -> Vec<u8> {
    source_with_words(before, after, resources, width, ["A0", "A1", "A2", "A3"])
}

/// 空セルを含む場合も正式なLengthとxrefを持つ所有原本へ組み立てる。
fn source_with_words(
    before: &str,
    after: &str,
    resources: &str,
    width: f32,
    words: [&str; 4],
) -> Vec<u8> {
    source_with_group(before, after, resources, width, words, "")
}

/// 所属ページに未確認の合成groupを持つ原本も、重複キーなしで構成する。
fn source_with_group(
    before: &str,
    after: &str,
    resources: &str,
    width: f32,
    words: [&str; 4],
    group: &str,
) -> Vec<u8> {
    let mut content = format!("{before} 0 g 0 G {width} w ");
    for y in [300, 330, 360] {
        content.push_str(&format!("30 {y} m 130 {y} l S "));
    }
    for x in [30, 80, 130] {
        content.push_str(&format!("{x} 300 m {x} 360 l S "));
    }
    for (word, x, y) in [
        (words[0], 35, 345),
        (words[1], 85, 345),
        (words[2], 35, 315),
        (words[3], 85, 315),
    ] {
        content.push_str(&format!("BT /F1 8 Tf {x} {y} Td ({word}) Tj ET "));
    }
    content.push_str(after);
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 300 420] >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> {resources} >> /Contents 4 0 R {group} >>"
        ),
        page_attribute_pdf::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ])
}

/// 不透明な既定RGBページGroupでは、原本どおり青い見出し二セルを保持する。
#[test]
fn default_rgb_page_transparency_group_keeps_confirmed_cell_background() {
    let input = source_with_group(
        ".2 .4 .6 rg 30 330 100 30 re f",
        "",
        "",
        1.,
        ["A0", "A1", "A2", "A3"],
        "/Group << /S /Transparency /CS /DeviceRGB >>",
    );
    assert_eq!(xml(&input).matches("w:fill=\"336699\"").count(), 2);
}

/// 未対応の合成属性や色空間を持つページGroupでは、従来どおり色を断定しない。
#[test]
fn unsupported_page_transparency_group_does_not_confirm_cell_background() {
    for group in [
        "/Group << /S /Transparency /CS /DeviceRGB /K true >>",
        "/Group << /S /Transparency /CS /DeviceRGB /I true >>",
        "/Group << /S /Transparency /CS /DeviceCMYK >>",
        "/Group << /S /Transparency >>",
    ] {
        let input = source_with_group(
            ".2 .4 .6 rg 30 330 100 30 re f",
            "",
            "",
            1.,
            ["A0", "A1", "A2", "A3"],
            group,
        );
        assert!(!xml(&input).contains("<w:shd"));
    }
}

/// 未選択のfontや独自glyph programの幾何から、背景とセルの所有を推測しない。
#[test]
fn missing_and_type3_fonts_do_not_confirm_text_background() {
    for resources in ["<< >>", "<< /Font << /F1 5 0 R >> >>"] {
        let content = ".2 .4 .6 rg 20 100 100 30 re f 0 g BT /F1 12 Tf 30 110 Td (A) Tj ET";
        let input=page_attribute_pdf::pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 300 240] >>".into(),
            format!("<< /Type /Page /Parent 2 0 R /Resources {resources} /Contents 4 0 R >>"),
            page_attribute_pdf::stream(content),
            "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 600 800] /FontMatrix [.001 0 0 .001 0 0] /CharProcs << /A 6 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [600] /Resources << >> >>".into(),
            page_attribute_pdf::stream("600 0 d0 0 0 600 800 re f"),
        ]);
        let parsed = parse_pdf_bytes("owned-font-background.pdf", &input).unwrap();
        assert!(!parsed.text_elements.is_empty());
        assert!(
            parsed
                .text_elements
                .iter()
                .all(|t| t.source_background.is_none()),
            "{resources}"
        );
    }
}

/// 原本の一枚の矩形が空セルも覆う場合、隣の文字で確認した矩形範囲を保持する。
#[test]
fn verified_source_rectangle_preserves_an_empty_header_cell() {
    let input = source_with_words(
        ".2 .4 .6 rg 30 330 100 30 re f",
        "",
        "",
        1.,
        ["", "A1", "A2", "A3"],
    );
    let xml = xml(&input);
    assert_eq!(xml.matches("<w:tc>").count(), 4);
    assert_eq!(xml.matches("w:fill=\"336699\"").count(), 2);
}

/// 隣の一セルだけの塗りは、空のセルへ広げない。
#[test]
fn an_empty_cell_does_not_inherit_neighbor_only_fill() {
    let input = source_with_words(
        ".2 .4 .6 rg 80 330 50 30 re f",
        "",
        "",
        1.,
        ["", "A1", "A2", "A3"],
    );
    assert_eq!(xml(&input).matches("w:fill=\"336699\"").count(), 1);
}

/// 太さを確認した原本罫線の内側と、整数gridの丸め範囲だけを背景へ対応させる。
#[test]
fn source_border_half_width_and_grid_rounding_define_background_extent() {
    let input = source_with_rule(".2 .4 .6 rg 31.2 331.2 97.6 27.6 re f", "", "", 2.);
    assert_eq!(xml(&input).matches("w:fill=\"336699\"").count(), 2);
}

/// 公開変換の編集可能な本文XMLだけを読む。
fn xml(source: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-cell-fills.pdf", source).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 公開変換が見出し行の塗りと一セルの強調を別々のセル背景として保持する。
#[test]
fn source_header_and_highlight_fills_belong_to_editable_cells() {
    let xml = xml(&source());
    assert_eq!(xml.matches("<w:tbl>").count(), 1);
    assert_eq!(xml.matches("<w:tr>").count(), 2);
    assert_eq!(xml.matches("<w:tc>").count(), 4);
    for (word, color) in [
        ("A0", Some("336699")),
        ("A1", Some("336699")),
        ("A2", None),
        ("A3", Some("FFFF00")),
    ] {
        let cell = xml
            .split("<w:tc>")
            .find(|cell| cell.contains(&format!(">{word}</w:t>")))
            .unwrap()
            .split("</w:tc>")
            .next()
            .unwrap();
        if let Some(color) = color {
            assert!(
                cell.contains(&format!("w:fill=\"{color}\"")),
                "{word} must retain its source background: {cell}"
            );
        } else {
            assert!(
                !cell.contains("<w:shd"),
                "{word} must not inherit neighboring fill"
            );
        }
        assert_eq!(xml.matches(&format!(">{word}</w:t>")).count(), 1);
    }
}

/// 部分塗り・後続の遮蔽・文字の後から塗る矩形を、セル全体の背景へ広げない。
#[test]
fn partial_or_later_overpaint_does_not_invent_cell_shading() {
    for (before, after) in [
        (".2 .4 .6 rg 35 342 15 15 re f", ""),
        ("", ".2 .4 .6 rg 30 330 100 30 re f"),
        (
            ".2 .4 .6 rg 30 330 100 30 re f",
            "1 0 0 rg 60 330 10 30 re f",
        ),
        (
            ".2 .4 .6 rg 30 330 100 30 re f",
            ".2 .4 .6 rg 30 330 100 30 re f",
        ),
        (".2 .4 .6 rg 30 330 100 30 re 50 335 20 20 re f*", ""),
    ] {
        let xml = xml(&source_with(before, after, ""));
        assert!(!xml.contains("<w:shd"), "{before} / {after}: {xml}");
    }
}

/// 不明な色変換・透明度・合成・clipでは、矩形の色を断定しない。
#[test]
fn unknown_paint_and_clipping_keep_existing_unshaded_cells() {
    for (prefix, resources) in [
        ("/P gs", "/ExtGState << /P << /ca .5 >> >>"),
        ("/P gs", "/ExtGState << /P << /BM /Multiply >> >>"),
        ("/P gs", "/ExtGState << /P << /TR /Identity >> >>"),
        ("/Unknown gs", ""),
        ("", "/ColorSpace << /DefaultRGB /DeviceRGB >>"),
        ("0 0 20 20 re W n", ""),
    ] {
        let content = format!("{prefix} .2 .4 .6 rg 30 330 100 30 re f");
        let xml = xml(&source_with(&content, "", resources));
        assert!(!xml.contains("<w:shd"), "{prefix} / {resources}");
    }
}

/// 同じ色の内側矩形を重ねても外側の原本範囲を保持し、文字色と混同しない。
#[test]
fn same_color_inner_rectangle_and_saved_paint_keep_background_extent() {
    let input = source_with(
        ".2 .4 .6 rg q 1 0 0 rg 180 100 30 30 re f Q 30 330 100 30 re f 35 342 60 15 re f",
        "",
        "",
    );
    let parsed = parse_pdf_bytes("owned-background-state.pdf", &input).unwrap();
    for text in parsed
        .text_elements
        .iter()
        .filter(|t| t.text == "A0" || t.text == "A1")
    {
        let background = text.source_background.unwrap();
        assert_eq!(background.rgb, [51, 102, 153]);
        assert_eq!(
            [
                background.bounds.x0,
                background.bounds.y0,
                background.bounds.x1,
                background.bounds.y1
            ],
            [30., 330., 130., 360.]
        );
        assert_eq!(text.fill_color.as_deref(), Some("000000"));
    }
    assert_eq!(xml(&input).matches("w:fill=\"336699\"").count(), 2);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 原本全体を平行移動しても、対応する二セルだけの色と四つの文字を保持する。
    #[test]
    fn translated_source_background_keeps_cell_ownership(dx in 0i32..16,dy in 0i32..16) {
        let input=source_with(&format!("q 1 0 0 1 {dx} {dy} cm .2 .4 .6 rg 30 330 100 30 re f")," Q","");
        let xml=xml(&input);
        prop_assert_eq!(xml.matches("w:fill=\"336699\"").count(),2);
        prop_assert_eq!(xml.matches("<w:tc>").count(),4);
        for word in ["A0","A1","A2","A3"] {prop_assert_eq!(xml.matches(&format!(">{word}</w:t>")).count(),1);}
    }
}
