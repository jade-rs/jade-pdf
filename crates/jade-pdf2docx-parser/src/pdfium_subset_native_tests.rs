//! 同名familyの別resourceでも、SDK経路の太さを元descriptorへ対応させる契約。
use crate::{ParseLimits, decode::DecodeBudget};

/// binaryの所有CFFを埋め、同じfamily名のfont辞書を別resourceとして配置する。
fn source(weights: &[u16], broken: bool, unused_last: bool) -> Vec<u8> {
    let program = if broken {
        b"owned-invalid-CFF".to_vec()
    } else {
        crate::type1_outline::owned_subset_cff()
    };
    let count = weights.len();
    let content_number = 5 + count * 2;
    let resources = (0..count)
        .map(|i| format!("/F{i} {} 0 R", 5 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let used = count - usize::from(unused_last);
    let content = (0..used)
        .map(|i| format!("BT /F{i} 20 Tf {} 150 Td <01> Tj ET\n", 20 + i * 50))
        .collect::<String>();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /OwnedForm {content_number} 0 R >> >> /Contents {} 0 R >>",content_number+1).into_bytes(),
        stream(&program, "/Subtype /Type1C"),
    ];
    for (i, weight) in weights.iter().enumerate() {
        let prefix = ["ABCDEF", "GHIJKL"][i];
        objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{prefix}+OwnedSubset /FirstChar 1 /LastChar 1 /Widths [600] /Encoding << /BaseEncoding /WinAnsiEncoding /Differences [1 /A] >> /FontDescriptor {} 0 R >>",6+i*2).into_bytes());
        objects.push(format!("<< /Type /FontDescriptor /FontName /{prefix}+OwnedSubset /Flags 32 /FontBBox [0 0 500 700] /ItalicAngle 0 /Ascent 700 /Descent 0 /CapHeight 700 /StemV 80 /FontWeight {weight} /FontFile3 4 0 R >>").into_bytes());
    }
    objects.push(stream(content.as_bytes(), &format!("/Type /XObject /Subtype /Form /BBox [0 0 200 200] /Resources << /Font << {resources} >> >>")));
    objects.push(stream(b"/OwnedForm Do", ""));
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

/// 所有streamのbyte数を直接Lengthへ記録し、font programを文字列へ変換しない。
fn stream(payload: &[u8], entries: &str) -> Vec<u8> {
    let mut bytes = format!("<< {entries} /Length {} >>\nstream\n", payload.len()).into_bytes();
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(b"\nendstream");
    bytes
}

/// SDKをdropしてから所有する観察値を返し、assertion時の共有Mutex汚染を避ける。
fn original_fonts(bytes: &[u8]) -> Vec<(String, bool)> {
    let sdk = pdfium_auto::bind_pdfium_silent().unwrap();
    let doc = sdk.load_pdf_from_byte_slice(bytes, None).unwrap();
    let page = doc.pages().get(0).unwrap();
    let text = page.text().unwrap();
    text.chars()
        .iter()
        .filter_map(|ch| {
            let obj = ch.text_object().ok()?;
            Some((ch.font_name(), obj.font().is_embedded().unwrap()))
        })
        .collect()
}

/// 入力をファイルへ保存せず、通常のnative backendを通した本文spanだけを返す。
fn native(bytes: &[u8]) -> Vec<crate::ParsedTextElement> {
    let pure = crate::parser::parse_pdf_bytes_pure("owned-subset.pdf".into(), bytes).unwrap();
    assert!(!pure.text_elements.is_empty());
    assert!(
        pure.text_elements
            .iter()
            .all(|e| e.text.chars().all(|ch| ch == '\u{1}'))
    );
    let limits = ParseLimits::default();
    let elements = crate::pdfium_backend::parse_pdf(
        std::path::Path::new("owned-subset.pdf"),
        bytes,
        &limits,
        &DecodeBudget::new(&limits),
    )
    .unwrap()
    .text_elements;
    assert_eq!(
        elements
            .iter()
            .flat_map(|e| e.text.chars())
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>(),
        "A".repeat(pure.text_elements.len())
    );
    elements
}

/// metadataが700でも、実際にSDKで埋め込みが成立するfontだけを確認値にする。
#[test]
fn native_subset_embedded_descriptor_weight_is_preserved() {
    let bytes = source(&[700], false, false);
    let observed = original_fonts(&bytes);
    assert_eq!(observed, vec![("OwnedSubset".into(), true)]);
    let elements = native(&bytes);
    assert_eq!(
        elements.iter().map(|e| e.text.as_str()).collect::<String>(),
        "A"
    );
    assert!(
        elements
            .iter()
            .all(|e| e.font_name.as_deref() == Some("OwnedSubset"))
    );
    assert_eq!(
        elements.iter().map(|e| e.font_weight).collect::<Vec<_>>(),
        vec![Some(700)]
    );
}

/// 同名のnative familyでも、別のsource辞書が宣言する400と700を混同しない。
#[test]
fn native_subset_same_family_keeps_distinct_resource_weights() {
    let bytes = source(&[400, 700], false, false);
    assert_eq!(
        original_fonts(&bytes),
        vec![("OwnedSubset".into(), true); 2]
    );
    let elements = native(&bytes);
    assert_eq!(
        elements
            .iter()
            .flat_map(|e| e.text.chars())
            .filter(|c| !c.is_whitespace())
            .collect::<String>(),
        "AA"
    );
    assert_eq!(
        elements
            .iter()
            .filter(|e| !e.text.trim().is_empty())
            .map(|e| e.font_weight)
            .collect::<Vec<_>>(),
        vec![Some(400), Some(700)]
    );
}

/// 候補のfont programが壊れていたら元SDKの名前を維持し、推定値を採用しない。
#[test]
fn native_subset_failed_embedding_uses_original_unconfirmed_font() {
    let bytes = source(&[700], true, false);
    let observed = original_fonts(&bytes);
    assert!(!observed.is_empty() && observed.iter().all(|(_, embedded)| !*embedded));
    let elements = native(&bytes);
    assert!(!elements.is_empty());
    assert!(
        elements
            .iter()
            .all(|e| e.font_weight.is_none() && e.font_name.as_ref() == Some(&observed[0].0))
    );
}

/// 使用されずSDK確認できない候補が一つでもあれば、候補全体の書換えを採用しない。
#[test]
fn native_subset_unobserved_candidate_keeps_original_input() {
    let bytes = source(&[700, 400], false, true);
    assert_eq!(original_fonts(&bytes), vec![("OwnedSubset".into(), true)]);
    let elements = native(&bytes);
    assert_eq!(
        elements.iter().map(|e| e.text.as_str()).collect::<String>(),
        "A"
    );
    assert!(
        elements
            .iter()
            .all(|e| e.font_weight.is_none() && e.font_name.as_deref() == Some("OwnedSubset"))
    );
}

/// metadata候補の識別とSDK確認を分け、失敗した段階を固定する。
#[test]
fn native_subset_owned_candidate_reaches_sdk_preflight() {
    let bytes = source(&[700], false, false);
    let budget = DecodeBudget::new(&ParseLimits::default());
    crate::parser::parse_pdf_bytes_pure_with_budget(
        "owned-subset.pdf".into(),
        &bytes,
        &ParseLimits::default(),
        &budget,
    )
    .unwrap();
    let mut tags =
        crate::pdfium_font_tags::Tagged::prepare_weights(&budget, &bytes, false).unwrap();
    assert!(tags.has_weights(), "source candidate was not tagged");
    let sdk = pdfium_auto::bind_pdfium_silent().unwrap();
    let verified = tags.confirm_embedded(&budget, &sdk, 1000).unwrap();
    assert!(verified, "SDK did not confirm every candidate");
}
