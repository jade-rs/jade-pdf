use std::io::Write;

use flate2::{Compression, write::ZlibEncoder};
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;

/// 必要なら stream 本文を Flate 圧縮する。
fn encoded(bytes: &[u8], flate: bool) -> Vec<u8> {
    if !flate {
        return bytes.to_vec();
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// xref stream の正式 offset から Length を読む最小 PDF を作る。
fn pdf_with_xref_stream(payload: &[u8], compressed_length: bool, flate: bool) -> Vec<u8> {
    pdf_with_xref_stream_options(payload, compressed_length, flate, false)
}

/// xref の PNG Up predictor を選択できる fixture を作る。
fn pdf_with_xref_stream_options(
    payload: &[u8],
    compressed_length: bool,
    flate: bool,
    predictor: bool,
) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = [0usize; 9];
    let generation = if compressed_length { 0 } else { 7 };
    for (number, object) in [
        (
            1,
            "1 0 obj << /Type /Catalog /Pages 8 0 R >> endobj\n".to_string(),
        ),
        (
            2,
            "2 0 obj << /Type /Page /Parent 8 0 R /MediaBox [0 0 100 100] /Contents 3 0 R >> endobj\n"
                .to_string(),
        ),
        (
            3,
            format!("3 0 obj << /Length 4 {generation} R >> stream\n"),
        ),
    ] {
        offsets[number] = pdf.len();
        pdf.extend_from_slice(object.as_bytes());
    }
    pdf.extend_from_slice(payload);
    pdf.extend_from_slice(b"\nendstream endobj\n");
    let filter = if flate { " /Filter /FlateDecode" } else { "" };
    if compressed_length {
        offsets[6] = pdf.len();
        let body = encoded(format!("4 0 {}", payload.len()).as_bytes(), flate);
        pdf.extend_from_slice(
            format!(
                "6 0 obj << /Type /ObjStm /N 1 /First 4 /Length {}{filter} >> stream\n",
                body.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&body);
        pdf.extend_from_slice(b"\nendstream endobj\n");
    } else {
        offsets[4] = pdf.len();
        pdf.extend_from_slice(format!("4 7 obj {} endobj\n", payload.len()).as_bytes());
    }
    offsets[8] = pdf.len();
    pdf.extend_from_slice(b"8 0 obj << /Type /Pages /Count 1 /Kids [2 0 R] >> endobj\n");
    offsets[5] = pdf.len();
    let size = 9;
    let mut entries = Vec::new();
    for (number, offset) in offsets.iter().enumerate().take(size) {
        let (kind, field1, field2) =
            if number == 0 || (*offset == 0 && !(number == 4 && compressed_length)) {
                (0, 0, 65535)
            } else if number == 4 && compressed_length {
                (2, 6, 0)
            } else {
                (1, *offset as u32, if number == 4 { generation } else { 0 })
            };
        entries.push(kind);
        entries.extend_from_slice(&field1.to_be_bytes());
        entries.extend_from_slice(&(field2 as u16).to_be_bytes());
    }
    let params = if predictor {
        " /DecodeParms << /Predictor 12 /Columns 7 >>"
    } else {
        ""
    };
    if predictor {
        let mut previous = [0u8; 7];
        let mut predicted = Vec::new();
        for row in entries.chunks_exact(7) {
            predicted.push(2);
            for (byte, above) in row.iter().zip(previous.iter()) {
                predicted.push(byte.wrapping_sub(*above));
            }
            previous.copy_from_slice(row);
        }
        entries = predicted;
    }
    let entries = encoded(&entries, flate);
    pdf.extend_from_slice(format!("5 0 obj << /Type /XRef /Size {size} /Root 1 0 R /W [1 4 2] /Length {}{filter}{params} >> stream\n", entries.len()).as_bytes());
    pdf.extend_from_slice(&entries);
    pdf.extend_from_slice(
        format!("\nendstream endobj\nstartxref\n{}\n%%EOF\n", offsets[5]).as_bytes(),
    );
    pdf
}

/// xref stream の type 1 entry は generation と正式 offset を使う。
#[test]
fn xref_stream_resolves_uncompressed_length_object() {
    for flate in [false, true] {
        let pdf = pdf_with_xref_stream(
            b"xxxxxxx endstream endobj trailer << /Encrypt 9 0 R >>",
            false,
            flate,
        );
        let result = parse_pdf_bytes("xref-stream.pdf", &pdf);
        assert!(result.is_ok(), "{result:?}");
    }
}

/// type 2 entry の object stream 内にある整数 Length も正式な index で解決する。
#[test]
fn xref_stream_resolves_compressed_length_object() {
    for flate in [false, true] {
        let pdf = pdf_with_xref_stream(
            b"xxxxxxx endstream endobj\n4 0 obj 7 endobj\ntrailer << /Encrypt 9 0 R >>",
            true,
            flate,
        );
        let result = parse_pdf_bytes("compressed-length.pdf", &pdf);
        assert!(result.is_ok(), "{result:?}");
    }
}

/// stream・Length・格納先を削除しても、過去の xref stream の境界情報を使う。
#[test]
fn deleted_stream_resolves_historical_compressed_length_and_owner() {
    for compressed in [false, true] {
        for flate in [false, true] {
            for stream_revision in [false, true] {
                let mut pdf = pdf_with_xref_stream(b"endstream endobj 99 0 obj << /Type /Page >> endobj trailer << /Encrypt null >>", compressed, flate);
                let start = pdf
                    .windows(9)
                    .rposition(|word| word == b"startxref")
                    .unwrap();
                let prev: usize = std::str::from_utf8(&pdf[start + 9..])
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                let xref = pdf.len();
                if stream_revision {
                    let mut rows = Vec::new();
                    for (kind, offset, generation) in [
                        (0u8, 0, 1u16),
                        (0, 0, if compressed { 1 } else { 8 }),
                        (0, 0, 1),
                        (1, xref as u32, 0),
                    ] {
                        rows.push(kind);
                        rows.extend_from_slice(&offset.to_be_bytes());
                        rows.extend_from_slice(&generation.to_be_bytes());
                    }
                    pdf.extend_from_slice(format!("7 0 obj << /Type /XRef /Size 9 /Root 1 0 R /W [1 4 2] /Index [3 2 6 2] /Prev {prev} /Length {} >> stream\n", rows.len()).as_bytes());
                    pdf.extend_from_slice(&rows);
                    pdf.extend_from_slice(b"\nendstream endobj\n");
                } else {
                    pdf.extend_from_slice(format!("xref\n3 2\n0000000000 00001 f \n0000000000 {:05} f \n6 1\n0000000000 00001 f \ntrailer << /Size 9 /Root 1 0 R /Prev {prev} >>\n", if compressed {1} else {8}).as_bytes());
                }
                pdf.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
                let parsed = parse_pdf_bytes("historical-compressed-length.pdf", &pdf).unwrap();
                assert_eq!(parsed.page_count, 1);
                assert!(parsed.text_elements.is_empty());
                assert!(parsed.image_elements.is_empty());
                assert!(parsed.quality.issues.is_empty());
            }
        }
    }
}

/// xref stream の entry 数も index 予算に含む。
#[test]
fn xref_stream_entry_budget_is_inclusive() {
    let pdf = pdf_with_xref_stream(b"plain", false, false);
    let mut limits = ParseLimits {
        max_xref_entries: 9,
        ..ParseLimits::default()
    };
    assert!(parse_pdf_bytes_with_limits("xref.pdf", &pdf, &limits).is_ok());
    limits.max_xref_entries = 8;
    assert!(matches!(
        parse_pdf_bytes_with_limits("xref.pdf", &pdf, &limits),
        Err(ParseError::ResourceLimit {
            resource: "xref entries",
            limit: 8,
            actual: 9
        })
    ));
}

/// Flate 後の PNG Up predictor を復元してから xref entry を読む。
#[test]
fn xref_stream_png_up_predictor_resolves_length() {
    for compressed in [false, true] {
        let pdf = pdf_with_xref_stream_options(
            b"xxxxxxx endstream endobj trailer << /Encrypt 9 0 R >>",
            compressed,
            true,
            true,
        );
        let result = parse_pdf_bytes("predicted-xref.pdf", &pdf);
        assert!(result.is_ok(), "{result:?}");
    }
}

/// 構造検査の xref 展開も parser 全体と同じ zlib 予算を使う。
#[test]
fn indexed_xref_decode_obeys_shared_budget() {
    let pdf = pdf_with_xref_stream(b"plain", true, true);
    let limits = ParseLimits {
        max_total_decoded_bytes: 48,
        ..ParseLimits::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_limits("budget.pdf", &pdf, &limits),
        Err(ParseError::ResourceLimit {
            resource: "total decoded bytes",
            limit: 48,
            actual: 49
        })
    ));
}

/// compressed entry の index が指す header 番号は参照先と一致しなければならない。
#[test]
fn compressed_length_header_number_must_match() {
    let mut pdf = pdf_with_xref_stream(b"plain", true, false);
    // binary xref を維持したまま、object stream 内の同じ幅の番号だけを差し替える。
    let header = pdf.windows(6).position(|word| word == b"4 0 5\n").unwrap();
    pdf[header] = b'9';
    assert!(matches!(
        parse_pdf_bytes("member.pdf", &pdf),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// xref field 幅・object stream member 数・First の破損を型付きで拒否する。
#[test]
fn malformed_indexed_stream_parameters_are_rejected() {
    for (before, after) in [
        (b"/W [1 4 2]".as_slice(), b"/W [0 0 0]".as_slice()),
        (b"/N 1".as_slice(), b"/N 9".as_slice()),
        (b"/First 4".as_slice(), b"/First 9".as_slice()),
    ] {
        let mut pdf = pdf_with_xref_stream(b"plain", true, false);
        let position = pdf
            .windows(before.len())
            .position(|word| word == before)
            .unwrap();
        pdf[position..position + before.len()].copy_from_slice(after);
        assert!(matches!(
            parse_pdf_bytes("invalid-index.pdf", &pdf),
            Err(ParseError::InvalidPdfStructure { .. })
        ));
    }
}

proptest! {
    /// 任意の binary 本文も xref stream の Length 解決結果を変えない。
    #[test]
    fn xref_stream_binary_data_stays_out_of_structure(payload in proptest::collection::vec(any::<u8>(), 0..512), compressed in any::<bool>(), predicted in any::<bool>()) {
        let pdf = pdf_with_xref_stream_options(&payload, compressed, true, predicted);
        let result = parse_pdf_bytes("binary-xref.pdf", &pdf);
        prop_assert!(result.is_ok(), "{result:?}");
    }
}

/// xref stream を参照する通常 table と、同じ object を変更する増分 revision を作る。
fn hybrid_pdf(payload: &[u8], table_free: bool, later_free: bool) -> Vec<u8> {
    hybrid_pdf_options(payload, table_free, later_free, true)
}

/// 補助 stream の Prev を辿ってはならない入力も作る。
fn hybrid_pdf_options(
    payload: &[u8],
    table_free: bool,
    later_free: bool,
    table_prev: bool,
) -> Vec<u8> {
    let mut pdf = pdf_with_xref_stream(payload, false, true);
    if !table_prev {
        let marker = b"/Type /XRef";
        let position = pdf
            .windows(marker.len())
            .position(|word| word == marker)
            .unwrap();
        pdf.splice(position..position, b"/Prev 999999999 ".iter().copied());
    }
    let start = pdf
        .windows(9)
        .rposition(|word| word == b"startxref")
        .unwrap();
    let xref_stream = std::str::from_utf8(&pdf[start + 10..])
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let table = pdf.len();
    let entry = if table_free {
        "4 1\n0000000000 00008 f \n"
    } else {
        "0 1\n0000000000 65535 f \n"
    };
    let prev = if table_prev {
        format!(" /Prev {xref_stream}")
    } else {
        String::new()
    };
    pdf.extend_from_slice(format!("xref\n{entry}trailer << /Size 9 /Root 1 0 R{prev} /XRefStm {xref_stream} >>\nstartxref\n{table}\n%%EOF\n").as_bytes());
    if later_free {
        let update = pdf.len();
        pdf.extend_from_slice(format!("xref\n4 1\n0000000000 00008 f \ntrailer << /Size 9 /Root 1 0 R /Prev {table} >>\nstartxref\n{update}\n%%EOF\n").as_bytes());
    }
    pdf
}

/// XRefStm は同じ revision の free entry を qpdf と同じ互換方針で上書きする。
#[test]
fn hybrid_xref_stream_overrides_same_revision_free_entry() {
    let pdf = hybrid_pdf(
        b"xxxxxxx endstream endobj trailer << /Encrypt 9 0 R >>",
        true,
        false,
    );
    let result = parse_pdf_bytes("hybrid.pdf", &pdf);
    assert!(result.is_ok(), "{result:?}");
}

/// 後の revision で削除された object を、過去の XRefStm は復活させない。
#[test]
fn hybrid_xref_stream_does_not_override_newer_free_entry() {
    let pdf = hybrid_pdf(b"plain", true, true);
    assert!(matches!(
        parse_pdf_bytes("hybrid.pdf", &pdf),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// XRefStm と Prev が同じ stream を指しても、読んだ各 entry の予算を計上する。
#[test]
fn hybrid_xref_counts_supplemental_entries() {
    let pdf = hybrid_pdf(b"plain", false, false);
    let limits = ParseLimits {
        max_xref_entries: 10,
        ..ParseLimits::default()
    };
    let inclusive = ParseLimits {
        max_xref_entries: 19,
        ..ParseLimits::default()
    };
    assert!(parse_pdf_bytes_with_limits("hybrid.pdf", &pdf, &inclusive).is_ok());
    assert!(matches!(
        parse_pdf_bytes_with_limits("hybrid.pdf", &pdf, &limits),
        Err(ParseError::ResourceLimit {
            resource: "xref entries",
            limit: 10,
            actual: 19
        })
    ));
}

/// 補助 stream の Prev を使わず、元の table trailer の Prev だけを使う。
#[test]
fn hybrid_xref_ignores_supplemental_prev() {
    let pdf = hybrid_pdf_options(b"plain", true, false, false);
    let result = parse_pdf_bytes("hybrid.pdf", &pdf);
    assert!(result.is_ok(), "{result:?}");
}

/// trailer の offset は直接整数でなければならず、参照先が table でも拒否する。
#[test]
fn hybrid_xref_rejects_indirect_and_non_stream_offsets() {
    for indirect in [false, true] {
        let mut pdf = hybrid_pdf(b"plain", false, false);
        let marker = b"/XRefStm ";
        let start = pdf
            .windows(marker.len())
            .rposition(|word| word == marker)
            .unwrap()
            + marker.len();
        let end = start
            + pdf[start..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
        if indirect {
            pdf.splice(end..end, b" 0 R".iter().copied());
        } else {
            // 最新 table の開始位置は、その直前の PDF 全体の長さ。
            let table = pdf
                .windows(6)
                .rposition(|word| word == b"\nxref\n")
                .unwrap()
                + 1;
            pdf.splice(start..end, table.to_string().bytes());
        }
        assert!(matches!(
            parse_pdf_bytes("hybrid.pdf", &pdf),
            Err(ParseError::InvalidPdfStructure { .. })
        ));
    }
}
