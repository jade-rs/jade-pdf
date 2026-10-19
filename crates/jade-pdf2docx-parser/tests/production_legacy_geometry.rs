//! Rootなし互換入力の幾何継承と深度制限を公開APIで検査する。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes_with_limits};

/// xrefとRootを持たない互換PDFを作る。
fn pdf(objects: &str) -> Vec<u8> {
    format!("%PDF-1.7\n{objects}\n%%EOF\n").into_bytes()
}

/// 更新後の親世代に含まれるCropBox・Rotateを公開ページへ適用する。
#[test]
fn public_api_uses_latest_parent_geometry() {
    let bytes = pdf(
        "2 0 obj << /MediaBox [0 0 10 20] /CropBox [0 0 5 5] /Rotate 90 >> endobj\n1 0 obj << /Type /Page /Parent 2 3 R >> endobj\n2 3 obj << /MediaBox [0 0 100 200] /CropBox [10 20 90 170] /Rotate 180 >> endobj",
    );
    let parsed =
        parse_pdf_bytes_with_limits("latest-parent.pdf", &bytes, &ParseLimits::default()).unwrap();
    assert_eq!(parsed.pages.len(), 1);
    assert_eq!(
        (
            parsed.pages[0].width,
            parsed.pages[0].height,
            parsed.pages[0].rotation
        ),
        (80.0, 150.0, 180)
    );
}

/// 公開APIは深さ境界で成功し、一つ超える親から取得した部分ページを返さない。
#[test]
fn public_api_parent_depth_is_inclusive() {
    let bytes = pdf(
        "1 0 obj << /Type /Page /Parent 2 0 R >> endobj\n2 0 obj << /Parent 3 0 R >> endobj\n3 0 obj << /Parent 4 7 R >> endobj\n4 7 obj << /MediaBox [0 0 100 200] >> endobj",
    );
    let mut limits = ParseLimits {
        max_container_nesting: 2,
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_limits("deep-parent.pdf", &bytes, &limits),
        Err(ParseError::ResourceLimit {
            resource: "page inheritance depth",
            limit: 2,
            actual: 3
        })
    ));
    limits.max_container_nesting = 3;
    let parsed = parse_pdf_bytes_with_limits("deep-parent.pdf", &bytes, &limits).unwrap();
    assert_eq!(parsed.pages.len(), 1);
    assert_eq!(
        (parsed.pages[0].width, parsed.pages[0].height),
        (100.0, 200.0)
    );
}
