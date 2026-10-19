use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;

/// 同じ PDF の末尾へ、Length object の更新または削除 entry を追加する。
fn append_length_revision(pdf: &mut Vec<u8>, length: usize, free: bool, custom_name: bool) {
    let marker = pdf
        .windows(9)
        .rposition(|word| word == b"startxref")
        .unwrap();
    let prev = std::str::from_utf8(&pdf[marker + 9..])
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let prev = prev.parse::<usize>().unwrap();
    let object_offset = pdf.len();
    pdf.extend_from_slice(format!("4 0 obj {length} endobj\n").as_bytes());
    let xref = pdf.len();
    let state = if free { "f" } else { "n" };
    let custom = if custom_name { " /Custom /Prev" } else { "" };
    pdf.extend_from_slice(format!("xref\n4 1\n{object_offset:010} 00000 {state} \ntrailer << /Size 6 /Root 1 0 R /Prev {prev}{custom} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
}

/// trailer の name 値を key と混同せず、Prev を辿って古い entry を補う。
#[test]
fn incremental_xref_trailer_name_value_is_not_a_prev_key() {
    let payload = b"xxxxxxx endstream endobj trailer << /Encrypt 9 0 R >>";
    let mut pdf = pdf_with_indirect_stream_length(payload, 0, false);
    append_length_revision(&mut pdf, payload.len(), false, true);
    let result = parse_pdf_bytes("incremental.pdf", &pdf);
    assert!(result.is_ok(), "{result:?}");
}

/// 最新 revision の free entry は古い有効 entry を復活させない。
#[test]
fn deleted_length_object_is_not_restored_from_previous_revision() {
    let mut pdf = pdf_with_indirect_stream_length(b"plain", 0, false);
    append_length_revision(&mut pdf, 5, true, false);
    assert!(matches!(
        parse_pdf_bytes("deleted.pdf", &pdf),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// 実際の stream 境界と合わない Length は本文内の marker で補修しない。
#[test]
fn incorrect_indirect_length_is_a_structure_error() {
    let mut pdf = pdf_with_indirect_stream_length(b"plain", 0, false);
    append_length_revision(&mut pdf, 1, false, false);
    assert!(matches!(
        parse_pdf_bytes("length.pdf", &pdf),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// 正式な xref table を持ち、Length object が本文より後に置かれた PDF を作る。
fn pdf_with_indirect_stream_length(payload: &[u8], generation: usize, encrypted: bool) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0];
    for object in [
        "1 0 obj << /Type /Catalog /Pages 5 0 R >> endobj\n".to_string(),
        "2 0 obj << /Type /Page /Parent 5 0 R /MediaBox [0 0 100 100] /Contents 3 0 R >> endobj\n"
            .to_string(),
        format!("3 0 obj << /Length 4 {generation} R >> stream\n"),
    ] {
        offsets.push(pdf.len());
        pdf.extend_from_slice(object.as_bytes());
    }
    pdf.extend_from_slice(payload);
    pdf.extend_from_slice(b"\nendstream endobj\n");
    offsets.push(pdf.len());
    pdf.extend_from_slice(format!("4 {generation} obj {} endobj\n", payload.len()).as_bytes());
    offsets.push(pdf.len());
    pdf.extend_from_slice(b"5 0 obj << /Type /Pages /Count 1 /Kids [2 0 R] >> endobj\n");
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for (object, offset) in offsets.iter().enumerate().skip(1) {
        let entry_generation = if object == 4 { generation } else { 0 };
        pdf.extend_from_slice(format!("{offset:010} {entry_generation:05} n \n").as_bytes());
    }
    let encrypt = if encrypted { " /Encrypt 6 0 R" } else { "" };
    pdf.extend_from_slice(
        format!("trailer << /Size 6 /Root 1 0 R{encrypt} >>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
    pdf
}

/// 前方参照の Length は正式な xref offset で解決し、本文の偽 object を使わない。
#[test]
fn indirect_length_uses_xref_instead_of_payload_markers() {
    let payload = b"xxxxxxx endstream endobj\n4 7 obj 7 endobj\ntrailer << /Encrypt 6 0 R >>";
    for generation in [0, 7] {
        let pdf = pdf_with_indirect_stream_length(payload, generation, false);
        let result = parse_pdf_bytes("indirect.pdf", &pdf);
        assert!(result.is_ok(), "{result:?}");
    }
}

/// Length 解決後も本物の暗号化 trailer を飛び越さない。
#[test]
fn indirect_length_does_not_hide_real_encryption() {
    let pdf = pdf_with_indirect_stream_length(b"trailer << /Encrypt null >>", 7, true);
    assert!(matches!(
        parse_pdf_bytes("encrypted.pdf", &pdf),
        Err(ParseError::UnsupportedEncryption { .. })
    ));
}

/// xref index 作成時も入力の宣言数を無制限に確保しない。
#[test]
fn xref_entry_budget_is_inclusive() {
    let pdf = pdf_with_indirect_stream_length(b"plain", 0, false);
    let mut limits = ParseLimits {
        max_xref_entries: 6,
        ..ParseLimits::default()
    };
    assert!(parse_pdf_bytes_with_limits("xref.pdf", &pdf, &limits).is_ok());
    limits.max_xref_entries = 5;
    assert!(matches!(
        parse_pdf_bytes_with_limits("xref.pdf", &pdf, &limits),
        Err(ParseError::ResourceLimit {
            resource: "xref entries",
            limit: 5,
            actual: 6
        })
    ));
}

/// Prev chain の entry も同じ予算を消費する。
#[test]
fn xref_budget_counts_previous_revisions() {
    let mut pdf = pdf_with_indirect_stream_length(b"plain", 0, false);
    append_length_revision(&mut pdf, 5, false, false);
    let mut limits = ParseLimits {
        max_xref_entries: 7,
        ..ParseLimits::default()
    };
    assert!(parse_pdf_bytes_with_limits("revision.pdf", &pdf, &limits).is_ok());
    limits.max_xref_entries = 6;
    assert!(matches!(
        parse_pdf_bytes_with_limits("revision.pdf", &pdf, &limits),
        Err(ParseError::ResourceLimit {
            resource: "xref entries",
            limit: 6,
            actual: 7
        })
    ));
}

/// xref と参照側の generation が違えば、同じ番号の object を使わない。
#[test]
fn indirect_length_generation_must_match_xref() {
    let pdf = pdf_with_indirect_stream_length(b"plain", 7, false);
    let pdf = String::from_utf8(pdf)
        .unwrap()
        .replacen("/Length 4 7 R", "/Length 4 8 R", 1);
    assert!(matches!(
        parse_pdf_bytes("generation.pdf", pdf.as_bytes()),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

proptest! {
    /// generation と任意 binary payload に依存せず、正式な Length を解決する。
    #[test]
    fn indirect_stream_data_is_not_pdf_syntax(payload in proptest::collection::vec(any::<u8>(), 0..512), generation in 0usize..65535) {
        let pdf = pdf_with_indirect_stream_length(&payload, generation, false);
        let result = parse_pdf_bytes("indirect-binary.pdf", &pdf);
        prop_assert!(result.is_ok(), "{result:?}");
    }

    /// 任意の stream 本文中の暗号化/構造 marker を宣言として解釈しない。
    #[test]
    fn stream_data_does_not_change_structure_validation(payload in proptest::collection::vec(any::<u8>(), 0..512)) {
        let mut pdf = format!("%PDF-1.7\n1 0 obj << /Length {} >> stream\n", payload.len()).into_bytes();
        pdf.extend_from_slice(&payload);
        pdf.extend_from_slice(b"\nendstream endobj\ntrailer << /Encrypt null >>");
        prop_assert!(parse_pdf_bytes("binary.pdf", &pdf).is_ok());
    }

    /// 入れ子上限を含む境界は許容し、1 段超過を拒否する。
    #[test]
    fn configurable_nesting_boundary_is_inclusive(depth in 1usize..64) {
        let pdf = format!("%PDF-1.7\n1 0 obj {}0{} endobj", "[".repeat(depth), "]".repeat(depth));
        let mut limits = ParseLimits { max_container_nesting: depth, ..ParseLimits::default() };
        prop_assert!(parse_pdf_bytes_with_limits("deep.pdf", pdf.as_bytes(), &limits).is_ok());
        limits.max_container_nesting -= 1;
        let result = parse_pdf_bytes_with_limits("deep.pdf", pdf.as_bytes(), &limits);
        prop_assert!(matches!(result, Err(ParseError::ResourceLimit { resource: "container nesting", .. })), "nesting limit not applied");
    }
}

/// 深い container を入力長に比例する巨大な状態配列へ展開しない。
#[test]
fn excessive_container_nesting_is_bounded() {
    let pdf = format!(
        "%PDF-1.7\n1 0 obj {}0{} endobj",
        "[".repeat(257),
        "]".repeat(257)
    );
    assert!(matches!(
        parse_pdf_bytes("deep.pdf", pdf.as_bytes()),
        Err(ParseError::ResourceLimit {
            resource: "container nesting",
            limit: 256,
            actual: 257
        })
    ));
}

/// header やコメント内の object marker だけでは正常な PDF とみなさない。
#[test]
fn header_only_or_fake_objects_are_rejected() {
    for bytes in [
        b"%PDF-1.7\n".as_slice(),
        b"%PDF-1.7\n% 1 0 obj << >> endobj\n",
        b"%PDF-1.7\n(1 0 obj << >> endobj)\n",
        b"%PDF-1.7\n1 0 obj << /Type /Catalog >>\n",
    ] {
        assert!(matches!(
            parse_pdf_bytes("broken.pdf", bytes),
            Err(ParseError::InvalidPdfStructure { .. })
        ));
    }
}

/// trailer / xref stream の暗号化宣言を typed error にする。
#[test]
fn declared_encryption_is_rejected_before_extracting_content() {
    for suffix in [
        "trailer << /Root 1 0 R /Encrypt 5 0 R >>",
        "trailer << /Encr#79pt << /Filter /Standard >> /Root 1 0 R >>",
        "2 0 obj << /Type /XRef /Encrypt 5 0 R /Length 0 >> stream\n\nendstream endobj",
        "2 0 obj << /Encrypt 5 0 R /Type /XRef /Length 0 >> stream\n\nendstream endobj",
    ] {
        let pdf = format!("%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\n{suffix}");
        assert!(matches!(
            parse_pdf_bytes("encrypted.pdf", pdf.as_bytes()),
            Err(ParseError::UnsupportedEncryption { .. })
        ));
    }
}

/// 本文・コメント・名前に含まれる Encrypt は暗号化宣言ではない。
#[test]
fn encryption_words_in_data_do_not_reject_unencrypted_documents() {
    let payload =
        b"trailer << /Encrypt 5 0 R >> endstream 8 0 obj << /Type /XRef /Encrypt 5 0 R >> endobj";
    let mut pdf = format!("%PDF-1.7\n1 0 obj << /Type /Catalog /Note (trailer << /Encrypt 5 0 R >>) /Custom << /Encrypt 5 0 R >> >> endobj\n% trailer << /Encrypt 5 0 R >>\n2 0 obj << /Length {} >> stream\n", payload.len()).into_bytes();
    pdf.extend_from_slice(payload);
    pdf.extend_from_slice(
        b"\nendstream endobj\ntrailer << /Encrypt null /Info << /Encrypt 5 0 R >> /Root 1 0 R >>",
    );
    assert!(parse_pdf_bytes("plain.pdf", &pdf).is_ok());
}

/// bytes API の軽量検証は xref を持たない既存の最小 object fixture も許容する。
#[test]
fn minimal_complete_object_is_still_supported() {
    assert!(
        parse_pdf_bytes(
            "minimal.pdf",
            b"%PDF-1.7\n1 0 obj << /Type /Page >> endobj\n"
        )
        .is_ok()
    );
}
