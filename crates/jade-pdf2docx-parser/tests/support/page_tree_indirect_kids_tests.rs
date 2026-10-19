//! 全fixtureで見つかった間接Kids配列の所属・順序と参照境界を固定する。
use super::{ordered, pdf};
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;

/// Kidsを間接化し、指定した参照鎖の末尾へ配列を格納する。
fn indirect_objects(kids: &str, depth: usize) -> Vec<String> {
    let mut objects = ordered(kids);
    objects[1] = "<< /Type /Pages /Count 2 /Kids 9 0 R >>".into();
    for index in 0..depth {
        objects.push(if index + 1 == depth {
            format!("[{kids}]")
        } else {
            format!("{} 0 R", 10 + index)
        });
    }
    objects
}

proptest! {
    /// 間接Kidsの鎖を辿っても未所属Pageを除外し、配列順と本文の番号を保持する。
    #[test]
    fn indirect_kids_preserves_order(reverse in any::<bool>(), depth in 1usize..5) {
        let kids = if reverse { "4 0 R 3 0 R" } else { "3 0 R 4 0 R" };
        let bytes = pdf(&indirect_objects(kids, depth), "1 0 R");
        let parsed = parse_pdf_bytes("indirect-kids.pdf", &bytes).unwrap();
        prop_assert_eq!(parsed.page_count, 2);
        prop_assert_eq!(parsed.pages[0].width, if reverse { 200.0 } else { 100.0 });
        for element in &parsed.text_elements {
            prop_assert_ne!(element.text.as_str(), "orphan");
            let expected = if element.text == "first" { usize::from(reverse) } else { usize::from(!reverse) };
            prop_assert_eq!(element.page, expected);
        }
    }
}

/// 参照の欠落・世代・循環と配列以外の末尾は構造エラーとして拒否する。
#[test]
fn malformed_indirect_kids_is_rejected() {
    for body in ["99 0 R", "9 1 R", "9 0 R", "null", "<<>>", "[3 0 R 3 0 R]"] {
        let mut objects = indirect_objects("3 0 R 4 0 R", 1);
        objects[8] = body.into();
        let result = parse_pdf_bytes("bad-kids.pdf", &pdf(&objects, "1 0 R"));
        assert!(
            matches!(result, Err(ParseError::InvalidPdfStructure { .. })),
            "{body}: {result:?}"
        );
    }
}

/// 参照鎖の深度上限をinclusiveに守り、ページ木の深さとは独立に制限する。
#[test]
fn indirect_kids_depth_is_bounded() {
    let bytes = pdf(&indirect_objects("3 0 R 4 0 R", 3), "1 0 R");
    let mut limits = ParseLimits {
        max_container_nesting: 2,
        ..ParseLimits::default()
    };
    let result = parse_pdf_bytes_with_limits("kids-depth.pdf", &bytes, &limits);
    assert!(
        matches!(
            result,
            Err(ParseError::ResourceLimit {
                actual: 3,
                limit: 2,
                ..
            })
        ),
        "{result:?}"
    );
    limits.max_container_nesting = 3;
    assert_eq!(
        parse_pdf_bytes_with_limits("kids-depth.pdf", &bytes, &limits)
            .unwrap()
            .page_count,
        2
    );
}
