//! 原本では収まる小数pointの行列を、各em高の独立した丸め誤差だけで旧配置へ戻さない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 一つのセルへ同じem高の表示行を隙間なく描き、容量を小数pointの合計から決める。
fn source(count: usize, milli_points: u32) -> Vec<u8> {
    let size = f64::from(milli_points) / 1000.;
    let top = (300. + 3.45 + count as f64 * size).ceil();
    let mut content = format!(
        "1 w 72 300 m 272 300 l S 72 {top} m 272 {top} l S 72 300 m 72 {top} l S 172 300 m 172 {top} l S 272 300 m 272 {top} l S"
    );
    for i in 0..count {
        let y = top - 3.45 - (i + 1) as f64 * size;
        content.push_str(&format!(
            " BT /F1 {size:.3} Tf 82 {y:.5} Td (Row{i:02}) Tj ET"
        ));
    }
    content.push_str(&format!(
        " BT /F1 {size:.3} Tf 182 {} Td (Control) Tj ET",
        top - 3.45 - size
    ));
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

/// 製品byte APIが出力した最初のセルの本文と、段落ごとの前余白・行高を読む。
fn paragraphs(count: usize, milli_points: u32) -> Vec<(String, u32, u32)> {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-fractional-cell-lines.pdf",
        &source(count, milli_points),
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let start = xml.find("<w:tc>").unwrap();
    let end = start + xml[start..].find("</w:tc>").unwrap();
    let mut reader = Reader::from_str(&xml[start..end]);
    let mut out = Vec::new();
    let mut current = (String::new(), 0, 0);
    let mut in_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:p" => current = (String::new(), 0, 0),
            Event::Start(tag) if tag.name().as_ref() == b"w:t" => in_text = true,
            Event::End(tag) if tag.name().as_ref() == b"w:t" => in_text = false,
            Event::Text(text) if in_text => current.0.push_str(&text.decode().unwrap()),
            Event::Empty(tag) if tag.name().as_ref() == b"w:spacing" => {
                for a in tag.attributes() {
                    let a = a.unwrap();
                    if a.key.as_ref() == b"w:before" {
                        current.1 = std::str::from_utf8(&a.value).unwrap().parse().unwrap();
                    }
                    if a.key.as_ref() == b"w:line" {
                        current.2 = std::str::from_utf8(&a.value).unwrap().parse().unwrap();
                    }
                }
            }
            Event::End(tag) if tag.name().as_ref() == b"w:p" => out.push(current.clone()),
            Event::Eof => return out,
            _ => {}
        }
    }
}

/// 7.98ptを23回8ptへ丸めたためだけに、本来186.99ptの表示行を187ptセルから落とさない。
#[test]
fn fractional_source_line_stack_that_fits_keeps_all_source_paragraphs() {
    let p = paragraphs(23, 7980);
    assert_eq!(
        p.len(),
        23,
        "fractional source line stack must not overflow solely through accumulated rounding"
    );
    assert_eq!(
        p.iter().map(|p| p.0.clone()).collect::<Vec<_>>(),
        (0..23).map(|i| format!("Row{i:02}")).collect::<Vec<_>>()
    );
    assert_eq!(p.iter().map(|p| p.1 + p.2).sum::<u32>(), 3740);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 24,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_cell_twip_rounding.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 行数や小数em高を変えても、各段落末端の累積丸め誤差は1twip以内に収まる。
    #[test]
    fn each_fractional_line_endpoint_stays_near_source(count in 2usize..31, milli_points in 6000u32..14000) {
        let p = paragraphs(count, milli_points);
        prop_assert_eq!(p.len(), count);
        let mut used = 0u32;
        for (i, line) in p.iter().enumerate() {
            let expected_text = format!("Row{i:02}");
            prop_assert_eq!(&line.0, &expected_text);
            prop_assert!(line.2 > 0);
            used += line.1 + line.2;
            // 所有原本のmm単位ではない整数千分pointの指定から、独立に末端を計算する。
            let exact_milli = 3450u64 + (i + 1) as u64 * u64::from(milli_points);
            let expected = ((exact_milli * 20 + 500) / 1000) as i64;
            prop_assert!((i64::from(used) - expected).abs() <= 1);
        }
    }
}
