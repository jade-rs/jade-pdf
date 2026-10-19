//! 正式な catalog と page tree に従う所属・順序・不正参照を検証する。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;

#[path = "support/page_tree_indirect_kids_tests.rs"]
mod indirect_kids;

/// 指定本文を連番 object として正式な xref と Root を持つ PDF を作る。
fn pdf(objects: &[String], root: &str) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
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
            "trailer << /Size {} /Root {root} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

/// 本文 stream を直接 Length で包む。
fn stream(text: &str) -> String {
    format!("<< /Length {} >>\nstream\n{text}\nendstream", text.len())
}

/// 独立した寸法と本文を持つ二つの Page と未所属 Page を作る。
fn ordered(kids: &str) -> Vec<String> {
    vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Count 2 /Kids [{kids}] >>"),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 6 0 R >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 7 0 R >>".into(),
        "<< /Type /Page /MediaBox [0 0 999 999] /Contents 8 0 R >>".into(),
        stream("BT /F1 12 Tf 10 20 Td (first) Tj ET"),
        stream("BT /F1 12 Tf 10 20 Td (second) Tj ET"),
        stream("BT /F1 12 Tf 10 20 Td (orphan) Tj ET"),
    ]
}

proptest! {
    /// Kids の交換はページ寸法と本文のページ番号に同じ順序で反映される。
    #[test]
    fn kids_order_controls_membership_and_text(reverse in any::<bool>()) {
        let kids = if reverse { "4 0 R 3 0 R" } else { "3 0 R 4 0 R" };
        let bytes = pdf(&ordered(kids), "1 0 R");
        let parsed = parse_pdf_bytes("ordered.pdf", &bytes).unwrap();
        prop_assert_eq!(parsed.page_count, 2);
        prop_assert_eq!(parsed.pages.len(), 2);
        prop_assert_eq!(parsed.pages[0].width, if reverse { 200.0 } else { 100.0 });
        for element in &parsed.text_elements {
            prop_assert_ne!(element.text.as_str(), "orphan");
            let expected = if element.text == "first" { usize::from(reverse) } else { usize::from(!reverse) };
            prop_assert_eq!(element.page, expected);
        }
        prop_assert!(!parsed.text_preview.contains("orphan"));
    }
}

/// 未所属 Page は最大ページ数の消費対象に含めない。
#[test]
fn orphan_does_not_consume_page_limit() {
    let limits = ParseLimits {
        max_pages: 2,
        ..ParseLimits::default()
    };
    assert!(
        parse_pdf_bytes_with_limits("limit.pdf", &pdf(&ordered("4 0 R 3 0 R"), "1 0 R"), &limits)
            .is_ok()
    );
}

/// 欠落・世代不一致・重複・循環・異なる型の child は構造エラーで止める。
#[test]
fn malformed_tree_is_rejected() {
    for kids in ["99 0 R", "3 1 R", "3 0 R 3 0 R", "2 0 R", "6 0 R", "null"] {
        let result = parse_pdf_bytes("broken.pdf", &pdf(&ordered(kids), "1 0 R"));
        assert!(
            matches!(result, Err(ParseError::InvalidPdfStructure { .. })),
            "Kids {kids}: {result:?}"
        );
    }
}

/// Root は参照の存在と世代および catalog 型を満たす必要がある。
#[test]
fn invalid_root_is_rejected() {
    for root in ["99 0 R", "1 1 R", "3 0 R", "null"] {
        assert!(
            matches!(
                parse_pdf_bytes("root.pdf", &pdf(&ordered("3 0 R 4 0 R"), root)),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "Root {root}"
        );
    }
}

/// 間接参照を使った深い page tree にも container 上限を適用する。
#[test]
fn indirect_tree_depth_is_bounded() {
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 3 0 R /MediaBox [0 0 100 100] >>".into(),
    ];
    let limits = ParseLimits {
        max_container_nesting: 2,
        ..ParseLimits::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_limits("depth.pdf", &pdf(&objects, "1 0 R"), &limits),
        Err(ParseError::ResourceLimit {
            resource: "page tree depth",
            ..
        })
    ));
}

/// 空の tree が未所属 Page の画像をページ0として復活させない。
#[test]
fn empty_tree_does_not_resurrect_orphan_image() {
    let mut objects = ordered("");
    objects[1] = "<< /Type /Pages /Count 0 /Kids [] >>".into();
    objects.push(stream("ignored"));
    objects[8] = "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Length 3 >>\nstream\nabc\nendstream".into();
    let parsed = parse_pdf_bytes("empty.pdf", &pdf(&objects, "1 0 R")).unwrap();
    assert_eq!(parsed.page_count, 0);
    assert!(parsed.image_elements.is_empty());
    assert!(parsed.text_elements.is_empty());
    assert!(parsed.graphics.is_empty());
    assert!(parsed.quality.issues.is_empty());
}

/// Kids の順序を画像の配置ページにも適用する。
#[test]
fn images_follow_kids_order() {
    let mut objects = ordered("4 0 R 3 0 R");
    objects[2] = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 6 0 R /Resources << /XObject << /Im 9 0 R >> >> >>".into();
    objects[3] = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 7 0 R /Resources << /XObject << /Im 9 0 R >> >> >>".into();
    objects[5] = stream("q 10 0 0 10 0 0 cm /Im Do Q");
    objects[6] = stream("q 20 0 0 20 0 0 cm /Im Do Q");
    objects.push("<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Length 3 >>\nstream\nabc\nendstream".into());
    let parsed = parse_pdf_bytes("images.pdf", &pdf(&objects, "1 0 R")).unwrap();
    assert_eq!(parsed.image_elements.len(), 2);
    for image in &parsed.image_elements {
        assert_eq!(
            image.bounds.x1 - image.bounds.x0,
            if image.page == 0 { 20.0 } else { 10.0 }
        );
    }
}

/// xref table の増分更新を Root の明示または継承で連結する。
fn revision(mut bytes: Vec<u8>, objects: &[(u32, &str)], root: Option<&str>) -> Vec<u8> {
    let marker = bytes
        .windows(9)
        .rposition(|word| word == b"startxref")
        .unwrap();
    let prev: usize = std::str::from_utf8(&bytes[marker + 9..])
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let mut offsets = Vec::new();
    for (number, body) in objects {
        offsets.push((*number, bytes.len()));
        bytes.extend_from_slice(format!("{number} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n");
    for (number, offset) in offsets {
        bytes.extend_from_slice(format!("{number} 1\n{offset:010} 00000 n \n").as_bytes());
    }
    let root = root
        .map(|value| format!(" /Root {value}"))
        .unwrap_or_default();
    bytes.extend_from_slice(
        format!("trailer << /Size 12 /Prev {prev}{root} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// 最新 trailer の Root を選び、省略された次の revision にも継承する。
#[test]
fn root_replacement_and_inheritance_select_one_tree() {
    let base = pdf(&ordered("3 0 R 4 0 R"), "1 0 R");
    let bytes = revision(
        base,
        &[
            (9, "<< /Type /Catalog /Pages 10 0 R >>"),
            (10, "<< /Type /Pages /Count 1 /Kids [11 0 R] >>"),
            (
                11,
                "<< /Type /Page /Parent 10 0 R /MediaBox [0 0 333 444] >>",
            ),
        ],
        Some("9 0 R"),
    );
    let bytes = revision(
        bytes,
        &[(
            11,
            "<< /Type /Page /Parent 10 0 R /MediaBox [0 0 555 666] >>",
        )],
        None,
    );
    let parsed = parse_pdf_bytes("root-revision.pdf", &bytes).unwrap();
    assert_eq!(parsed.page_count, 1);
    assert_eq!(parsed.pages[0].width, 555.0);
    assert!(parsed.text_elements.is_empty());
    assert!(parsed.text_preview.is_empty());
}

/// catalog が直接 Page を指定する破損を拒否する。
#[test]
fn catalog_pages_must_be_pages_node() {
    let mut objects = ordered("3 0 R 4 0 R");
    objects[0] = "<< /Type /Catalog /Pages 3 0 R >>".into();
    assert!(matches!(
        parse_pdf_bytes("leaf-root.pdf", &pdf(&objects, "1 0 R")),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// 入れ子の Pages も Kids を深さ優先で辿り、物理順序から独立する。
#[test]
fn nested_tree_preserves_kids_order() {
    let mut objects = ordered("9 0 R 3 0 R");
    objects.push("<< /Type /Pages /Parent 2 0 R /Count 1 /Kids [4 0 R] >>".into());
    objects[3] = objects[3].replace("/Parent 2 0 R", "/Parent 9 0 R");
    let parsed = parse_pdf_bytes("nested.pdf", &pdf(&objects, "1 0 R")).unwrap();
    assert_eq!(
        parsed
            .pages
            .iter()
            .map(|page| page.width as u32)
            .collect::<Vec<_>>(),
        [200, 100]
    );
}

/// graphics のページ番号も Kids の順序に従う。
#[test]
fn graphics_follow_kids_order() {
    let mut objects = ordered("4 0 R 3 0 R");
    objects[5] = stream("0 0 m 10 0 l S");
    objects[6] = stream("0 0 m 20 0 l S");
    let parsed = parse_pdf_bytes("graphics.pdf", &pdf(&objects, "1 0 R")).unwrap();
    assert_eq!(parsed.graphics.len(), 2);
    for graphic in &parsed.graphics {
        assert_eq!(
            graphic.bounds.x1 - graphic.bounds.x0,
            if graphic.page == 0 { 20.0 } else { 10.0 }
        );
    }
}

/// Catalog・Pages・Page が同じ ObjStm 内でも Root と Kids 順を使用する。
#[test]
fn compressed_catalog_and_tree_preserve_order() {
    let members = [
        (1u32, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Count 2 /Kids [4 0 R 3 0 R] >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>"),
        (4, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
        (7, "<< /Type /Page /MediaBox [0 0 999 999] >>"),
    ];
    let mut header = String::new();
    let mut area = String::new();
    for (number, body) in members {
        header.push_str(&format!("{number} {} ", area.len()));
        area.push_str(body);
        area.push(' ');
    }
    let first = header.len();
    let payload = format!("{header}{area}");
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let owner = bytes.len();
    bytes.extend_from_slice(format!("5 0 obj\n<< /Type /ObjStm /N 5 /First {first} /Length {} >>\nstream\n{payload}\nendstream\nendobj\n", payload.len()).as_bytes());
    let xref = bytes.len();
    let mut entries = Vec::new();
    for number in 0..8 {
        let (kind, offset, index): (u8, usize, u16) = match number {
            0 => (0, 0, 65535),
            5 => (1, owner, 0),
            6 => (1, xref, 0),
            7 => (2, 5, 4),
            _ => (2, 5, number - 1),
        };
        entries.push(kind);
        entries.extend_from_slice(&(offset as u32).to_be_bytes());
        entries.extend_from_slice(&index.to_be_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "6 0 obj\n<< /Type /XRef /Size 8 /Root 1 0 R /W [1 4 2] /Length {} >>\nstream\n",
            entries.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(&entries);
    bytes.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    let parsed = parse_pdf_bytes("compressed-tree.pdf", &bytes).unwrap();
    assert_eq!(parsed.page_count, 2);
    assert_eq!(
        parsed
            .pages
            .iter()
            .map(|page| page.width as u32)
            .collect::<Vec<_>>(),
        [200, 100]
    );
}

/// Kids に残る削除済み child を古い Page で補完しない。
#[test]
fn deleted_child_is_structural_error() {
    let mut bytes = pdf(&ordered("3 0 R 4 0 R"), "1 0 R");
    let marker = bytes
        .windows(9)
        .rposition(|word| word == b"startxref")
        .unwrap();
    let prev: usize = std::str::from_utf8(&bytes[marker + 9..])
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n3 1\n0000000000 00001 f \ntrailer << /Size 9 /Prev {prev} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    assert!(matches!(
        parse_pdf_bytes("deleted-child.pdf", &bytes),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// 同名 font resource のページ別定義にも Kids の順序を適用する。
#[test]
fn font_resources_follow_kids_order() {
    let mut objects = ordered("4 0 R 3 0 R");
    objects[2] = objects[2].replace(
        "/Contents",
        "/Resources << /Font << /F1 9 0 R >> >> /Contents",
    );
    objects[3] = objects[3].replace(
        "/Contents",
        "/Resources << /Font << /F1 10 0 R >> >> /Contents",
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    let parsed = parse_pdf_bytes("fonts.pdf", &pdf(&objects, "1 0 R")).unwrap();
    assert_eq!(parsed.text_elements.len(), 2);
    for element in &parsed.text_elements {
        assert_eq!(
            element.font_name.as_deref(),
            Some(if element.page == 0 {
                "Courier"
            } else {
                "Helvetica"
            })
        );
    }
}

/// Count は所属する子孫 Page の総数と一致し、非負の整数でなければならない。
#[test]
fn invalid_pages_count_is_rejected() {
    for count in [
        "",
        "/Count -1",
        "/Count 0",
        "/Count 1",
        "/Count 3",
        "/Count 2.0",
        "/Count null",
        "/Count (2)",
        "/Count 99 0 R",
        "/Count 9 1 R",
    ] {
        let mut objects = ordered("3 0 R 4 0 R");
        objects[1] = format!("<< /Type /Pages {count} /Kids [3 0 R 4 0 R] >>");
        objects.push("2".into());
        let result = parse_pdf_bytes("count.pdf", &pdf(&objects, "1 0 R"));
        assert!(
            matches!(result, Err(ParseError::InvalidPdfStructure { .. })),
            "{count}: {result:?}"
        );
    }
}

/// Parent は Kids を所有する Pages の番号と世代に一致する必要がある。
#[test]
fn inconsistent_page_parent_is_rejected() {
    for parent in [
        "",
        "/Parent 1 0 R",
        "/Parent 2 1 R",
        "/Parent 99 0 R",
        "/Parent null",
        "/Parent 4 0 R",
    ] {
        let mut objects = ordered("3 0 R 4 0 R");
        objects[2] = format!("<< /Type /Page {parent} /MediaBox [0 0 100 100] >>");
        let result = parse_pdf_bytes("parent.pdf", &pdf(&objects, "1 0 R"));
        assert!(
            matches!(result, Err(ParseError::InvalidPdfStructure { .. })),
            "{parent}: {result:?}"
        );
    }
}

/// Pages の Parent も実際の tree 辺に一致し、根の Pages は Parent を持たない。
#[test]
fn inconsistent_pages_parent_is_rejected() {
    for parent in ["", "/Parent 1 0 R", "/Parent 2 1 R", "/Parent null"] {
        let mut objects = ordered("9 0 R 3 0 R");
        objects[3] = objects[3].replace("/Parent 2 0 R", "/Parent 9 0 R");
        objects.push(format!(
            "<< /Type /Pages {parent} /Count 1 /Kids [4 0 R] >>"
        ));
        assert!(matches!(
            parse_pdf_bytes("pages-parent.pdf", &pdf(&objects, "1 0 R")),
            Err(ParseError::InvalidPdfStructure { .. })
        ));
    }
    let mut objects = ordered("3 0 R 4 0 R");
    objects[1] = "<< /Type /Pages /Parent 1 0 R /Count 2 /Kids [3 0 R 4 0 R] >>".into();
    assert!(matches!(
        parse_pdf_bytes("root-parent.pdf", &pdf(&objects, "1 0 R")),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// 各 Pages の Count は直接 child 数ではなく、その subtree の Page 数を数える。
#[test]
fn nested_count_validates_each_subtree() {
    let mut objects = ordered("9 0 R 3 0 R");
    objects[3] = objects[3].replace("/Parent 2 0 R", "/Parent 9 0 R");
    objects.push("<< /Type /Pages /Parent 2 0 R /Count 2 /Kids [4 0 R] >>".into());
    assert!(matches!(
        parse_pdf_bytes("nested-count.pdf", &pdf(&objects, "1 0 R")),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
    objects[8] = "<< /Type /Pages /Parent 2 0 R /Count 1 /Kids [4 0 R] >>".into();
    objects[1] = "<< /Type /Pages /Count 2 /Kids [9 0 R] >>".into();
    assert!(matches!(
        parse_pdf_bytes("root-count.pdf", &pdf(&objects, "1 0 R")),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
    objects[1] = "<< /Type /Pages /Count 1 /Kids [9 0 R] >>".into();
    assert_eq!(
        parse_pdf_bytes("valid-count.pdf", &pdf(&objects, "1 0 R"))
            .unwrap()
            .page_count,
        1
    );
}

/// Count の間接整数も世代を検査して読み、循環する scalar 参照を拒否する。
#[test]
fn indirect_count_is_resolved_and_cycles_are_rejected() {
    let mut objects = ordered("3 0 R 4 0 R");
    objects[1] = "<< /Type /Pages /Count 9 0 R /Kids [3 0 R 4 0 R] >>".into();
    objects.push("+2 % count\n".into());
    assert_eq!(
        parse_pdf_bytes("indirect-count.pdf", &pdf(&objects, "1 0 R"))
            .unwrap()
            .page_count,
        2
    );
    objects[8] = "9 0 R".into();
    assert!(matches!(
        parse_pdf_bytes("cycle-count.pdf", &pdf(&objects, "1 0 R")),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

proptest! {
    /// 任意の child 数に対し Count が一致する場合だけ成功する。
    #[test]
    fn count_matches_leaf_cardinality(leaves in 0usize..12, count in 0usize..16) {
        let kids = (0..leaves).map(|i| format!("{} 0 R", i + 3)).collect::<Vec<_>>().join(" ");
        let mut objects = vec!["<< /Type /Catalog /Pages 2 0 R >>".into(), format!("<< /Type /Pages /Count {count} /Kids [{kids}] >>")];
        objects.extend((0..leaves).map(|_| "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".into()));
        let result = parse_pdf_bytes("count-property.pdf", &pdf(&objects, "1 0 R"));
        if count == leaves {
            prop_assert_eq!(result.unwrap().page_count, leaves);
        } else {
            prop_assert!(matches!(result, Err(ParseError::InvalidPdfStructure { .. })), "mismatched Count must fail");
        }
    }
}

/// Count の参照 chain にも独立した深度上限を適用し、inclusive 境界を守る。
#[test]
fn indirect_count_depth_is_bounded() {
    let mut objects = ordered("3 0 R 4 0 R");
    objects[1] = "<< /Type /Pages /Count 9 0 R /Kids [3 0 R 4 0 R] >>".into();
    objects.extend(["10 0 R".into(), "11 0 R".into(), "2".into()]);
    let bytes = pdf(&objects, "1 0 R");
    let limits = ParseLimits {
        max_container_nesting: 2,
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_limits("count-depth.pdf", &bytes, &limits),
        Err(ParseError::ResourceLimit {
            resource: "page tree scalar depth",
            actual: 3,
            limit: 2
        })
    ));
    let limits = ParseLimits {
        max_container_nesting: 3,
        ..Default::default()
    };
    assert_eq!(
        parse_pdf_bytes_with_limits("count-depth.pdf", &bytes, &limits)
            .unwrap()
            .page_count,
        2
    );
}

/// 間接 Count は最新 revision を使い、古い値で不一致を隠さない。
#[test]
fn updated_indirect_count_is_validated() {
    let mut objects = ordered("3 0 R 4 0 R");
    objects[1] = "<< /Type /Pages /Count 9 0 R /Kids [3 0 R 4 0 R] >>".into();
    objects.push("2".into());
    let bytes = revision(pdf(&objects, "1 0 R"), &[(9, "1")], None);
    assert!(matches!(
        parse_pdf_bytes("count-update.pdf", &bytes),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
    let bytes = revision(bytes, &[(9, "2")], None);
    assert_eq!(
        parse_pdf_bytes("count-restored.pdf", &bytes)
            .unwrap()
            .page_count,
        2
    );
}

/// Count の整数には符号とコメントを許し、実数と整数風の複合値を拒否する。
#[test]
fn count_uses_pdf_integer_syntax() {
    for value in ["+2", "002", "2 % comment\n"] {
        let mut objects = ordered("3 0 R 4 0 R");
        objects[1] = format!("<< /Type /Pages /Count {value} /Kids [3 0 R 4 0 R] >>");
        assert_eq!(
            parse_pdf_bytes("count-syntax.pdf", &pdf(&objects, "1 0 R"))
                .unwrap()
                .page_count,
            2
        );
    }
    for value in ["2.0", "2e0", "(2)", "[2]", "18446744073709551616", "2 0 R"] {
        let mut objects = ordered("3 0 R 4 0 R");
        objects[1] = format!("<< /Type /Pages /Count {value} /Kids [3 0 R 4 0 R] >>");
        assert!(
            matches!(
                parse_pdf_bytes("count-syntax.pdf", &pdf(&objects, "1 0 R")),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{value}"
        );
    }
}

proptest! {
    /// 空の subtree や左右で異なる Page 数でも、各 subtree の Count を独立に照合する。
    #[test]
    fn nested_subtree_counts_are_independent(left in 0usize..5, right in 0usize..5, invalid_left in any::<bool>()) {
        let left_kids = (0..left).map(|i| format!("{} 0 R", i + 5)).collect::<Vec<_>>().join(" ");
        let right_kids = (0..right).map(|i| format!("{} 0 R", i + left + 5)).collect::<Vec<_>>().join(" ");
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            format!("<< /Type /Pages /Count {} /Kids [3 0 R 4 0 R] >>", left + right),
            format!("<< /Type /Pages /Parent 2 0 R /Count {} /Kids [{left_kids}] >>", left + usize::from(invalid_left)),
            format!("<< /Type /Pages /Parent 2 0 R /Count {right} /Kids [{right_kids}] >>"),
        ];
        objects.extend((0..left).map(|_| "<< /Type /Page /Parent 3 0 R /MediaBox [0 0 100 100] >>".into()));
        objects.extend((0..right).map(|_| "<< /Type /Page /Parent 4 0 R /MediaBox [0 0 200 200] >>".into()));
        let result = parse_pdf_bytes("subtree-property.pdf", &pdf(&objects, "1 0 R"));
        if invalid_left {
            prop_assert!(matches!(result, Err(ParseError::InvalidPdfStructure { .. })), "invalid subtree Count must fail");
        } else {
            let parsed = result.unwrap();
            prop_assert_eq!(parsed.page_count, left + right);
            prop_assert_eq!(parsed.pages.iter().map(|page| page.width as usize).collect::<Vec<_>>(), [vec![100; left], vec![200; right]].concat());
        }
    }
}

/// Parent の世代は固定の0ではなく、Kids を所有する Pages の実世代に合わせる。
#[test]
fn nonzero_parent_generation_is_supported() {
    let mut objects = ordered("3 0 R 4 0 R");
    objects[0] = objects[0].replace("/Pages 2 0 R", "/Pages 2 7 R");
    objects[2] = objects[2].replace("/Parent 2 0 R", "/Parent 2 7 R");
    objects[3] = objects[3].replace("/Parent 2 0 R", "/Parent 2 7 R");
    let mut bytes = pdf(&objects, "1 0 R");
    let offset = bytes
        .windows(7)
        .position(|word| word == b"2 0 obj")
        .unwrap();
    bytes[offset + 2] = b'7';
    let bytes = String::from_utf8(bytes)
        .unwrap()
        .replacen(
            &format!("{offset:010} 00000 n"),
            &format!("{offset:010} 00007 n"),
            1,
        )
        .into_bytes();
    assert_eq!(
        parse_pdf_bytes("parent-generation.pdf", &bytes)
            .unwrap()
            .page_count,
        2
    );
    let bytes = String::from_utf8(bytes)
        .unwrap()
        .replacen("/Parent 2 7 R", "/Parent 2 0 R", 1)
        .into_bytes();
    assert!(matches!(
        parse_pdf_bytes("parent-generation.pdf", &bytes),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}
