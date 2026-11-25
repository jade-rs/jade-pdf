//! 別ページの表が実段組を無効化せず、表末尾にも原本ページ境界を保持する回帰。
use super::*;

#[path = "page_section_evidence.rs"]
mod evidence;
use evidence::sections;

/// 自作の2行2列の表を、原本ページ末尾に配置する。
fn positioned_table(page: usize) -> PositionedTable {
    PositionedTable {
        page,
        bounds: ParsedBounds {
            x0: 48.,
            y0: 500.,
            x1: 248.,
            y1: 560.,
        },
        stroke_width: 0.5,
        font_name: Some("Helvetica".into()),
        font_embedded: false,
        embedded_face: None,
        font_size: Some(11.),
        column_widths_twips: vec![2000; 2],
        row_heights_twips: vec![600.; 2],
        table: OcrTable::new(
            (0..2)
                .map(|row| {
                    (0..2)
                        .map(|column| format!("TABLE PAGE{page} R{row}C{column}"))
                        .collect()
                })
                .collect(),
        ),
        cell_underlines: vec![vec![false; 2]; 2],
        cell_alignments: vec![vec![ParagraphAlignment::Left; 2]; 2],
        docx_table: None,
        native_cells: vec![vec![None; 2]; 2],
        cell_vertical_alignments: Vec::new(),
        row_heights_pt: Vec::new(),
        row_line_counts: Vec::new(),
        cell_padding_pt: None,
        cell_font_sizes: Vec::new(),
        cell_tab_stops: Vec::new(),
        cell_fragments: Vec::new(),
        cell_shading: Vec::new(),
        nested_tables: Vec::new(),
    }
}

/// 一方だけに表を持つ原本を所有データから梱包し、実際の本文XMLを読む。
fn table_and_columns(table_page: usize) -> String {
    let mut parsed = source(2, &[]);
    parsed
        .text_elements
        .retain(|text| text.page != table_page || text.bounds.x0 == 48.);
    packaged(parsed, &[positioned_table(table_page)])
}

/// 本文がない表・画像ページも、同じ本番構築・ZIP経路で検証する。
fn packaged(mut parsed: ParsedDocument, tables: &[PositionedTable]) -> String {
    let summary = tables.first().map(|table| table.table.clone());
    let docx = build_structured_docx_from_owned_images(&mut parsed, &summary, tables).unwrap();
    let bytes = pack_docx_to_bytes(docx, &[]).unwrap();
    document_xml(bytes)
}

/// 本番file/bytes経路が渡す推定表の互換要約も、他ページの段組を無効化しない。
#[test]
fn inferred_table_summary_preserves_other_pages_columns() {
    let mut parsed = source(2, &[]);
    parsed.text_elements.retain(|text| text.page != 0);
    let table = positioned_table(0);
    let summary = Some(table.table.clone());
    let docx = build_structured_docx_from_owned_images(&mut parsed, &summary, &[table]).unwrap();
    let xml = document_xml(pack_docx_to_bytes(docx, &[]).unwrap());
    let sections = sections(&xml);
    assert_eq!(
        sections.len(),
        2,
        "inferred table summary must preserve source sections"
    );
    assert_eq!((sections[0].columns, sections[0].tables), (1, 1));
    assert_eq!((sections[1].columns, sections[1].tables), (2, 0));
    assert!(sections[1].text.contains("PAGEB RIGHT 1"));
}

/// 段間を横切る未分割行が本文内に残るページへ、狭い実段組を適用しない。
#[test]
fn unsplit_body_row_crossing_the_gutter_does_not_enter_columns() {
    let mut parsed = source(2, &[]);
    let mut wide = parsed.text_elements[0].clone();
    wide.bounds.x1 = 550.;
    wide.bounds.y0 = 682.;
    wide.bounds.y1 = 693.;
    wide.baseline_y = Some(682.);
    wide.text = "FULL WIDTH BODY ROW".into();
    parsed.text_elements.push(wide);
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert!(
        detected_text_columns_for_page(&parsed, &pages[0], &[]).is_none(),
        "unsplit body row crossing the gutter must not be forced into a narrow column"
    );
}

/// 梱包済みDOCXから、Wordが解釈する本文を読み出す。
fn document_xml(bytes: Vec<u8>) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 表だけのページがあっても、他のページの二段組を一列へ戻さない。
#[test]
fn table_only_page_preserves_later_columns() {
    let mut parsed = source(2, &[]);
    parsed.text_elements.retain(|text| text.page != 0);
    let xml = packaged(parsed, &[positioned_table(0)]);
    let sections = sections(&xml);
    assert_eq!(
        sections.len(),
        2,
        "table-only page must not disable page-local sections"
    );
    assert_eq!((sections[0].columns, sections[0].tables), (1, 1));
    assert!(sections[0].text.contains("TABLE PAGE0 R1C1"));
    assert_eq!((sections[1].columns, sections[1].tables), (2, 0));
    assert!(sections[1].text.contains("PAGEB RIGHT 1"));
}

/// 最終ページの表のために前ページの左右段を一つの縦列へ流さない。
#[test]
fn table_on_last_page_preserves_earlier_columns_and_cells() {
    let xml = table_and_columns(1);
    assert_eq!(
        xml.matches("w:num=\"2\"").count(),
        1,
        "table on another page must not erase columns"
    );
    assert_eq!(xml.matches("<w:sectPr").count(), 2);
    assert_eq!(xml.matches("<w:tbl>").count(), 1);
    for row in 0..2 {
        for column in 0..2 {
            assert_eq!(
                xml.matches(&format!("TABLE PAGE1 R{row}C{column}")).count(),
                1
            );
        }
    }
    assert!(xml.contains("PAGEA LEFT 1") && xml.contains("PAGEA RIGHT 1"));
}

/// 表が最後のblockでも、次ページの実段組に表や本文を取り残さない。
#[test]
fn table_at_page_end_keeps_its_own_section_before_later_columns() {
    let xml = table_and_columns(0);
    let sections = sections(&xml);
    assert_eq!(
        sections.len(),
        2,
        "last table must retain its source page boundary"
    );
    assert_eq!((sections[0].columns, sections[0].tables), (1, 1));
    assert!(sections[0].text.contains("TABLE PAGE0 R1C1"));
    assert!(sections[0].text.contains("PAGEA LEFT 1"));
    assert!(!sections[0].text.contains("PAGEB"));
    assert_eq!((sections[1].columns, sections[1].tables), (2, 0));
    assert!(
        sections[1].text.contains("PAGEB LEFT 1") && sections[1].text.contains("PAGEB RIGHT 1")
    );
    assert!(!sections[1].text.contains("TABLE PAGE0"));
}

/// 本文のないfloating画像ページも、A4既定値へ変えず原本寸法で保持する。
#[test]
fn floating_image_only_page_keeps_source_size_and_later_columns() {
    let mut parsed = source(2, &[0]);
    parsed.text_elements.retain(|text| text.page != 0);
    parsed.image_elements[0].floating = true;
    parsed.pages[0].width = 420.;
    parsed.pages[0].height = 300.;
    let xml = packaged(parsed, &[]);
    let sections = sections(&xml);
    assert_eq!(sections.len(), 2);
    assert_eq!(
        (sections[0].width, sections[0].height),
        (8400, 6000),
        "floating-only source page must retain its own dimensions"
    );
    assert_eq!((sections[0].columns, sections[0].drawings), (1, 1));
    assert_eq!((sections[1].columns, sections[1].drawings), (2, 0));
}

/// 全ページに同じ上下見出しを持ち、最初だけ本文が空の原本を作る。
fn header_only_first_page() -> ParsedDocument {
    let mut parsed = source(2, &[]);
    parsed.text_elements.retain(|text| text.page != 0);
    let template = parsed.text_elements[0].clone();
    for page in 0..2 {
        for (text, top) in [("RUNNING HEADER", 752.), ("RUNNING FOOTER", 40.)] {
            let mut line = template.clone();
            line.page = page;
            line.text = text.into();
            line.font_size = 9.;
            line.bounds = ParsedBounds {
                x0: 190.,
                x1: 350.,
                y0: top - 9.,
                y1: top,
            };
            line.baseline_y = Some(top - 9.);
            parsed.text_elements.push(line);
        }
    }
    parsed
}

/// 見出し移動後に本文が空でも、source pageとそのpart参照を捨てない。
#[test]
fn header_only_page_keeps_source_boundary_before_later_columns() {
    let mut parsed = header_only_first_page();
    let docx = build_structured_docx_from_owned_images(&mut parsed, &None, &[]).unwrap();
    let bytes = pack_docx_to_bytes(docx, &[]);
    assert!(
        bytes.is_ok(),
        "header-only page must preserve source section and parts: {:?}",
        bytes.as_ref().err()
    );
    let bytes = bytes.unwrap();
    evidence::assert_chrome_relationships(&bytes);
    let xml = document_xml(bytes);
    let sections = sections(&xml);
    assert_eq!(
        sections.len(),
        2,
        "header-only page must preserve source section and parts"
    );
    assert_eq!((sections[0].width, sections[0].height), (12240, 15840));
    assert!(sections[0].text.is_empty());
    assert_eq!((sections[0].columns, sections[1].columns), (1, 2));
    assert!(sections[1].text.contains("PAGEB RIGHT 1"));
    assert!(!xml.contains("RUNNING HEADER") && !xml.contains("RUNNING FOOTER"));
}

proptest! {
    /// 表・画像・本文なしの並びを変えても、各source pageの内容を他sectionへ流さない。
    #[test]
    fn page_local_tables_images_and_bodyless_pages_preserve_ownership(
        page_count in 2usize..=5,
        table_mask in 0u8..32,
        image_mask in 0u8..32,
        bodyless_mask in 0u8..32,
    ) {
        // 最終ページは必ず二段組。残りのページの全組合せを同じ本番経路で検証する。
        let earlier = (1usize << (page_count - 1)) - 1;
        let table_mask = usize::from(table_mask) & earlier;
        let image_mask = usize::from(image_mask) & earlier;
        let bodyless_mask = usize::from(bodyless_mask) & (table_mask | image_mask);
        let image_pages: Vec<_> = (0..page_count)
            .filter(|page| image_mask & (1 << page) != 0).collect();
        let mut parsed = source(page_count, &image_pages);
        parsed.text_elements.retain(|text| bodyless_mask & (1 << text.page) == 0);
        for image in &mut parsed.image_elements {
            image.floating = true;
        }
        let tables: Vec<_> = (0..page_count)
            .filter(|page| table_mask & (1 << page) != 0).map(positioned_table).collect();
        let xml = packaged(parsed, &tables);
        let sections = sections(&xml);
        prop_assert_eq!(sections.len(), page_count);
        for (page, section) in sections.iter().enumerate() {
            let has_table = table_mask & (1 << page) != 0;
            let has_image = image_mask & (1 << page) != 0;
            prop_assert_eq!(section.tables, usize::from(has_table));
            prop_assert_eq!(section.drawings, usize::from(has_image));
            prop_assert_eq!(section.columns, if has_table || has_image {1} else {2});
            prop_assert_eq!((section.width, section.height), (12240, 15840));
            let own_marker = format!("PAGE{}", char::from(b'A' + page as u8));
            prop_assert_eq!(section.text.contains(&own_marker), bodyless_mask & (1 << page) == 0);
            for other in 0..page_count {
                if other != page {
                    prop_assert!(!section.text.contains(&format!("PAGE{}", char::from(b'A' + other as u8))), "foreign body page");
                    prop_assert!(!section.text.contains(&format!("TABLE PAGE{other} ")), "foreign table page");
                }
            }
            if has_table {
                for row in 0..2 {
                    for column in 0..2 {
                        prop_assert_eq!(section.text.matches(&format!("TABLE PAGE{page} R{row}C{column}")).count(), 1);
                    }
                }
            }
        }
    }
}

/// 大きい表の範囲に重ねて推定した小さい表は除き、離れた表は残す。
#[test]
fn table_inside_a_larger_table_is_dropped() {
    let large = positioned_table(0);
    let mut small = positioned_table(0);
    small.bounds = ParsedBounds {
        x0: 60.,
        y0: 510.,
        x1: 200.,
        y1: 550.,
    };
    let mut apart = positioned_table(1);
    apart.bounds = small.bounds;
    let kept = drop_tables_inside_larger_tables(vec![large, small, apart]);
    assert_eq!(
        kept.iter()
            .map(|table| (table.page, table.bounds.x0))
            .collect::<Vec<_>>(),
        vec![(0, 48.), (1, 60.)]
    );
}

/// 子表は、実際の行の高さで分けた親表の行の 1 つにだけ入る（等分の行境界で二つの行に入らない）。
#[test]
fn nested_table_goes_to_the_row_of_its_center_by_real_row_heights() {
    let mut parent = positioned_table(0);
    // 上の行は 10pt、下の行は 50pt（範囲は y 500〜560）。
    parent.row_heights_pt = vec![10., 50.];
    let mut child = positioned_table(0);
    child.bounds = ParsedBounds {
        x0: 50.,
        y0: 505.,
        x1: 140.,
        y1: 545.,
    };
    parent.nested_tables = vec![child];
    assert!(nested_tables_for_cell(&parent, 0, 0, 1).is_empty());
    assert_eq!(nested_tables_for_cell(&parent, 1, 0, 1).len(), 1);
}
