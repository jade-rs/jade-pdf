//! 別ページの表が実段組を無効化せず、表末尾にも原本ページ境界を保持する回帰。
use super::*;

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
    let docx = build_structured_docx_from_owned_images(&mut parsed, &None, tables).unwrap();
    let bytes = pack_docx_to_bytes(docx, &[]).unwrap();
    document_xml(bytes)
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

/// 原本ページに対応するsectionの内容と段数。
#[derive(Debug)]
struct Evidence {
    /// このsectionに所属する本文とセルの文字。
    text: String,
    /// Wordのsection段数。未指定時は1。
    columns: usize,
    /// 直接bodyに属する表の個数。
    tables: usize,
    /// このsection内の画像描画の個数。
    drawings: usize,
    /// Wordページの表示幅（twips）。
    width: usize,
    /// Wordページの表示高さ（twips）。
    height: usize,
}

impl Default for Evidence {
    /// Word既定の1段から検証を始める。
    fn default() -> Self {
        Self {
            text: String::new(),
            columns: 1,
            tables: 0,
            drawings: 0,
            width: 0,
            height: 0,
        }
    }
}

/// 実際のWord要素の整数属性を、prefixの表記順に依存せず読む。
fn integer(e: &quick_xml::events::BytesStart<'_>, name: &[u8], fallback: usize) -> usize {
    e.attributes()
        .map(Result::unwrap)
        .find(|a| a.key.local_name().as_ref() == name)
        .map(|a| std::str::from_utf8(&a.value).unwrap().parse().unwrap())
        .unwrap_or(fallback)
}

/// pPrのsectPrも、その段落自身を含めて前のsource sectionへ所属させる。
fn sections(xml: &str) -> Vec<Evidence> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut pending = Evidence::default();
    let mut result = Vec::new();
    let mut boundary = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) => {
                let tag = e.local_name().as_ref().to_vec();
                if tag == b"tbl" && stack.len() == 2 {
                    pending.tables += 1;
                }
                if tag == b"drawing" {
                    pending.drawings += 1;
                }
                if tag == b"sectPr" && stack.len() == 4 && stack[2] == b"p" {
                    boundary = true;
                }
                stack.push(tag);
            }
            Event::Empty(e) if e.local_name().as_ref() == b"cols" => {
                pending.columns = integer(&e, b"num", 1);
            }
            Event::Empty(e) if e.local_name().as_ref() == b"pgSz" => {
                pending.width = integer(&e, b"w", 0);
                pending.height = integer(&e, b"h", 0);
            }
            Event::Text(e) if stack.last().is_some_and(|tag| tag == b"t") => {
                pending
                    .text
                    .push_str(std::str::from_utf8(e.as_ref()).unwrap());
                pending.text.push('\n');
            }
            Event::End(e) => {
                let tag = e.local_name();
                let paragraph_end = tag.as_ref() == b"p" && stack.len() == 3 && boundary;
                let document_end = tag.as_ref() == b"sectPr" && stack.len() == 3;
                if paragraph_end || document_end {
                    result.push(std::mem::take(&mut pending));
                    boundary = false;
                }
                stack.pop().unwrap();
            }
            Event::Eof => break,
            _ => (),
        }
    }
    result
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
    let xml = document_xml(bytes.unwrap());
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
