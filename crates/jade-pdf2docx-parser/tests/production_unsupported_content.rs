//! 未対応の描画内容を黙った変換成功にしない品質診断の回帰。
use jade_pdf2docx_parser::{
    ParseError, ParseOptions, QualityIssue, UnsupportedXObjectReason, parse_pdf_bytes,
    parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 二つのページで同じ Form を参照する正式 PDF を生成する。
fn form_pdf(first: &str, second: &str) -> Vec<u8> {
    pdf(&form_objects(first, second))
}

/// Form と親から継承する Resources を含む object 群を返す。
fn form_objects(first: &str, second: &str) -> Vec<String> {
    let mut objects = pages("/MediaBox [0 0 100 100] /Resources << /XObject << /Fm 7 0 R >> >>");
    objects[4] = stream(first);
    objects[5] = stream(second);
    let content = "BT /F 12 Tf 10 20 Td (FORM TEXT) Tj ET";
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> >> /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objects
}

/// Contents 配列では operand と operator の間に stream 境界を置いても診断する。
#[test]
fn content_array_preserves_the_do_operand_across_streams() {
    let mut objects = form_objects("/Fm", "");
    objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 11 0 R] >>".into();
    objects.push(stream("Do"));
    let parsed = parse_pdf_bytes("split.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.quality.issues.len(), 1, "{:?}", parsed.quality);
}

/// 配列や辞書の内側にある偽 operator は stream 境界を越えても実行扱いしない。
#[test]
fn content_array_preserves_container_depth_across_streams() {
    for (first, second) in [("[", "/Fm Do ] TJ"), ("<<", "/Fm Do >>")] {
        let mut objects = form_objects(first, "");
        objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 11 0 R] >>".into();
        objects.push(stream(second));
        let parsed = parse_pdf_bytes("split.pdf", &pdf(&objects)).unwrap();
        assert!(parsed.quality.issues.is_empty(), "{:?}", parsed.quality);
    }
}

/// 繰り返し描画された Form の欠落をページと object ごとに一度報告する。
#[test]
fn painted_form_loss_is_reported_per_page_and_object() {
    let bytes = form_pdf("/Fm Do /Fm Do", "/Fm Do");
    let parsed = parse_pdf_bytes("form.pdf", &bytes).unwrap();
    assert_eq!(parsed.text_elements.len(), 3);
    assert!(
        parsed
            .text_elements
            .iter()
            .all(|text| text.text == "FORM TEXT")
    );
    assert_eq!(parsed.quality.issues.len(), 2, "{:?}", parsed.quality);
    let message = parsed.quality.issues[0].to_string();
    assert!(message.contains("unsupported_xobject"), "{message}");
    assert!(
        message.contains("page 1") && message.contains("object 7"),
        "{message}"
    );
    assert!(message.contains("Form"), "{message}");
}

/// ページ本文に文字が残っていても Form の部分欠落は strict で拒否する。
#[test]
fn strict_rejects_partial_form_loss() {
    let bytes = form_pdf("BT (PAGE TEXT) Tj ET /Fm Do", "");
    let parsed = parse_pdf_bytes("form.pdf", &bytes).unwrap();
    assert!(parsed.text_preview.contains("PAGE TEXT"));
    assert!(matches!(
        parse_pdf_bytes_with_options(
            "form.pdf",
            &bytes,
            &ParseOptions {
                strict: true,
                ..Default::default()
            }
        ),
        Err(ParseError::QualityRejected { .. })
    ));
}

/// 未描画 resource とコメント・文字列・container 内の偽 Do は品質診断を増やさない。
#[test]
fn unpainted_forms_and_fake_operators_are_ignored() {
    for content in [
        "",
        "% /Fm Do\n",
        "(/Fm Do) Tj",
        "<2F466D20446F> Tj",
        "[/Fm Do] TJ",
        "<< /Note /Fm /Op /Do >>",
        "/Fm /Do",
        "/Fm DoOther",
    ] {
        let bytes = form_pdf(content, "");
        let parsed = parse_pdf_bytes("form.pdf", &bytes).unwrap();
        assert!(
            parsed.quality.issues.is_empty(),
            "{content}: {:?}",
            parsed.quality
        );
    }
}

/// 空白に依存せずコメントを挟む escaped resource 名も実際の Do として診断する。
#[test]
fn escaped_name_with_comment_before_do_is_reported() {
    let bytes = form_pdf("/F#6d% comment\nDo", "");
    let parsed = parse_pdf_bytes("form.pdf", &bytes).unwrap();
    assert_eq!(parsed.quality.issues.len(), 1, "{:?}", parsed.quality);
}

/// inline image の binary を Do と誤認せず、未対応 inline image として拒否できる。
#[test]
fn inline_image_is_reported_without_scanning_its_binary_as_operators() {
    let bytes = form_pdf("BI /W 2 /H 1 /BPC 8 /CS /RGB ID /Fm Do EI", "");
    let parsed = parse_pdf_bytes("inline.pdf", &bytes).unwrap();
    assert_eq!(parsed.quality.issues.len(), 1, "{:?}", parsed.quality);
    assert!(
        parsed.quality.issues[0]
            .to_string()
            .contains("unsupported_inline_image")
    );
}

/// 同じ Form の resource alias は object 単位で重複除去する。
#[test]
fn aliases_share_one_diagnostic() {
    let mut objects = form_objects("/Fm Do /Alias Do", "");
    objects[1] = objects[1].replace("/Fm 7 0 R", "/Fm 7 0 R /Alias 7 0 R");
    let parsed = parse_pdf_bytes("alias.pdf", &pdf(&objects)).unwrap();
    assert_eq!(
        parsed.quality.issues,
        vec![QualityIssue::UnsupportedXObject {
            page: 0,
            object_number: 7,
            reason: UnsupportedXObjectReason::Form,
        }]
    );
}

/// escaped・間接 Subtype の Form とその他の未対応 Subtype を区別する。
#[test]
fn subtype_resolution_preserves_the_diagnostic_reason() {
    for (subtype, reason) in [
        ("/#46orm", UnsupportedXObjectReason::Form),
        ("/FutureKind", UnsupportedXObjectReason::Subtype),
        ("/Image", UnsupportedXObjectReason::Subtype),
    ] {
        let mut objects = form_objects("/Fm Do", "");
        objects[6] = objects[6].replace("/Subtype /Form", "/Subtype 11 0 R");
        objects.push(subtype.into());
        let parsed = parse_pdf_bytes("subtype.pdf", &pdf(&objects)).unwrap();
        assert_eq!(
            parsed.quality.issues,
            vec![QualityIssue::UnsupportedXObject {
                page: 0,
                object_number: 7,
                reason,
            }],
            "{subtype}"
        );
    }
}

/// inline image 後の同一ページの別 stream も binary の続きとして走査しない。
#[test]
fn inline_image_stops_only_its_page() {
    let mut objects = form_objects("BI /W 1 /H 1 ID", "/Fm Do");
    objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 11 0 R] >>".into();
    objects.push(stream("/Fm Do EI BI"));
    let parsed = parse_pdf_bytes("inline.pdf", &pdf(&objects)).unwrap();
    assert_eq!(
        parsed.quality.issues,
        vec![
            QualityIssue::UnsupportedInlineImage { page: 0 },
            QualityIssue::UnsupportedXObject {
                page: 1,
                object_number: 7,
                reason: UnsupportedXObjectReason::Form,
            },
        ]
    );
}

proptest! {
    /// 分割位置と描画回数によらず実行ページだけを一度診断し、偽命令を無視する。
    #[test]
    fn repeated_draws_and_stream_splits_keep_page_identity(
        first in any::<bool>(), second in any::<bool>(), repeats in 1usize..20,
        split in 0usize..3,
    ) {
        let draw = "/F#6d% name\nDo\n".repeat(repeats);
        let mut objects = form_objects(if first { &draw } else { "(/Fm Do) Tj" },
            if second { &draw } else { "% /Fm Do\n" });
        if first {
            let (left, right) = match split {
                0 => ("/F#6d", format!("Do\n{draw}")),
                1 => ("[/Fm", format!("Do] TJ\n{draw}")),
                _ => ("<< /Note /Fm", format!("/Op /Do >>\n{draw}")),
            };
            objects[4] = stream(left);
            objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 11 0 R] >>".into();
            objects.push(stream(&right));
        }
        let parsed = parse_pdf_bytes("generated.pdf", &pdf(&objects)).unwrap();
        let expected = [first, second].into_iter().enumerate()
            .filter(|(_, drawn)| *drawn)
            .map(|(page, _)| QualityIssue::UnsupportedXObject {
                page, object_number: 7, reason: UnsupportedXObjectReason::Form,
            }).collect::<Vec<_>>();
        prop_assert_eq!(parsed.quality.issues, expected);
    }
}
