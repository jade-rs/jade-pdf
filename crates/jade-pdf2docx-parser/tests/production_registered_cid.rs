//! Adobeの登録collectionを、Font局所のIdentity-H CIDとして復号する公開API回帰。
use jade_pdf2docx_parser::{ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;

#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// ページまたは同名Fontを持つFormへ、指定したType0 Fontを置く。
fn document(font: &str, descendant: &str, operand: &str, in_form: bool) -> Vec<u8> {
    let mut objects = fixture::pages(&format!(
        "/MediaBox [0 0 300 300] /Resources << /Font << /F 10 0 R >> {} >>",
        if in_form {
            "/XObject << /Fm 7 0 R >>"
        } else {
            ""
        }
    ));
    let content = format!("BT /F 10 Tf 20 100 Td {operand} Tj ET");
    objects[4] = fixture::stream(if in_form { "/Fm Do" } else { &content });
    objects[5] = fixture::stream("");
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F 13 0 R >> >> /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    objects.push(font.into());
    objects.push(descendant.into());
    objects.push(fixture::stream("1 beginbfchar <0cd4> <0058> endbfchar"));
    objects.push(font.into());
    fixture::pdf(&objects)
}

/// 公式Adobe-Japan1-UCS2のCID 3284/3722/1952が「日本語」に対応する。
const FONT: &str = "<< /Type /Font /Subtype /Type0 /BaseFont /Owned /Encoding /Identity-H /DescendantFonts [11 0 R] >>";
/// Supplement 2の登録collectionを宣言したdescendant。
const CHILD: &str = "<< /Type /Font /Subtype /CIDFontType0 /BaseFont /Owned /DW 1000 /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 2 >> >>";

/// 任意の抽出要素を連結し、空白の挿入に依存せず本文を比較する。
fn text(bytes: &[u8]) -> String {
    parse_pdf_bytes("registered-cid.pdf", bytes)
        .unwrap()
        .text_elements
        .iter()
        .flat_map(|element| element.text.chars())
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// hexとliteralは同じ二byte CIDで、ページとFormの局所resourceも同じ規則を使う。
#[test]
fn japan1_identity_h_restores_hex_and_literal_japanese() {
    for form in [false, true] {
        for operand in ["<0cd40e8a07a0>", "(\\014\\324\\016\\212\\007\\240)"] {
            assert_eq!(text(&document(FONT, CHILD, operand, form)), "日本語");
        }
    }
}

/// 登録表は明示ToUnicodeが存在するFontを上書きしない。
#[test]
fn explicit_tounicode_remains_authoritative() {
    let font = FONT.replace("/BaseFont", "/ToUnicode 12 0 R /BaseFont");
    assert_eq!(text(&document(&font, CHILD, "<0cd4>", false)), "X");
}

/// 登録情報とType0/Identity-Hの全条件が揃った場合だけCID表を適用する。
#[test]
fn unrelated_or_ambiguous_font_metadata_never_claims_japan1() {
    for (font, child) in [
        (FONT.replace("Identity-H", "Identity-V"), CHILD.into()),
        (FONT.replace("/Type0", "/Type1"), CHILD.into()),
        (FONT.replace("[11 0 R]", "[11 0 R 11 0 R]"), CHILD.into()),
        (FONT.replace("[11 0 R]", "[11 1 R]"), CHILD.into()),
        (FONT.into(), CHILD.replace("(Adobe)", "(Other)")),
        (FONT.into(), CHILD.replace("(Japan1)", "(GB1)")),
        (FONT.into(), CHILD.replace("/Supplement 2", "/Supplement 8")),
        (
            FONT.into(),
            CHILD.replace("/Supplement 2", "/Supplement 2.5"),
        ),
        (
            FONT.into(),
            CHILD.replace("/Registry (Adobe)", "/Registry (Adobe) /Registry (Other)"),
        ),
        (FONT.into(), CHILD.replace("/CIDSystemInfo", "/Unrelated")),
    ] {
        assert_ne!(
            text(&document(&font, &child, "<0cd40e8a07a0>", false)),
            "日本語"
        );
    }
}

/// 登録済みCIDでも奇数byte・notdef・collection外の値を別encodingとして推測しない。
#[test]
fn malformed_or_out_of_supplement_cids_fail_without_partial_text() {
    for operand in ["<0cd400>", "<0000>", "<ffff>", "<2210>"] {
        let result = parse_pdf_bytes(
            "bad-registered-cid.pdf",
            &document(FONT, CHILD, operand, false),
        );
        assert!(result.is_err(), "accepted {operand}");
        assert!(result.unwrap_err().to_string().contains("registered CID"));
    }
}

/// 末尾Supplementの対応文字も、正式な補助面文字と複数字を失わず保持する。
#[test]
fn highest_supported_supplement_decodes_reiwa_symbol() {
    let child = CHILD.replace("/Supplement 2", "/Supplement 7");
    assert_eq!(text(&document(FONT, &child, "<5a125a13>", false)), "㋿㋿");
}

/// 固定表は入力ToUnicodeのentry予算を消費せず、出力にはmetadata予算が適用される。
#[test]
fn static_registered_mapping_needs_no_tounicode_entries() {
    let bytes = document(FONT, CHILD, "<0cd4>", false);
    let parsed = parse_pdf_bytes_with_limits(
        "static-cid.pdf",
        &bytes,
        &ParseLimits {
            max_total_tounicode_entries: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        parsed
            .text_elements
            .iter()
            .any(|element| element.text == "日")
    );
}

proptest! {
    /// CID列の連結と順番を保ち、文字byteをShift_JISへ再解釈しない。
    #[test]
    fn japanese_cid_sequence_preserves_order(indices in prop::collection::vec(0usize..3, 1..40)) {
        let codes = ["0cd4", "0e8a", "07a0"];
        let chars = ['日', '本', '語'];
        let operand = format!("<{}>", indices.iter().map(|i| codes[*i]).collect::<String>());
        let expected = indices.iter().map(|i| chars[*i]).collect::<String>();
        prop_assert_eq!(text(&document(FONT, CHILD, &operand, false)), expected);
    }
}
