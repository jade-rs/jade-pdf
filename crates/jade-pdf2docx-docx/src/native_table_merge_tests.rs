//! 結合セルの全物理行から、原本の文字とstyleを一度だけ復元する。
use super::*;

/// 二つの物理行に異なるfontを持つ、外部資産のない解析結果を作る。
fn source() -> (
    ParsedDocument,
    Vec<Vec<Vec<ParsedTextElement>>>,
    DocxTableModel,
) {
    let texts = vec![
        ParsedTextElement {
            fill_stroke_width: None,
            font_weight: None,
            page: 0,
            text: "TOP".into(),
            font_name: Some("Helvetica-Bold".into()),
            font_size: 13.,
            fill_color: Some("123456".into()),
            baseline_y: Some(53.),
            bounds: ParsedBounds {
                x0: 12.,
                x1: 40.,
                y0: 53.,
                y1: 66.,
            },
        },
        ParsedTextElement {
            fill_stroke_width: None,
            font_weight: None,
            page: 0,
            text: "lower".into(),
            font_name: Some("Courier".into()),
            font_size: 9.,
            fill_color: Some("654321".into()),
            baseline_y: Some(13.),
            bounds: ParsedBounds {
                x0: 14.,
                x1: 45.,
                y0: 13.,
                y1: 22.,
            },
        },
    ];
    let cell = |text: &str, merge| DocxTableCellModel {
        text: text.into(),
        underlined: false,
        alignment: ParagraphAlignment::Left,
        grid_span: 1,
        vertical_merge: Some(merge),
        width_twips: None,
        borders: Some(DocxCellBorders::all()),
    };
    let model = DocxTableModel {
        rows: vec![
            vec![cell("TOP\nlower", DocxVerticalMerge::Restart)],
            vec![cell("", DocxVerticalMerge::Continue)],
        ],
    };
    let grouped = vec![vec![vec![texts[0].clone()]], vec![vec![texts[1].clone()]]];
    let parsed = ParsedDocument {
        path: "owned-native-vertical-merge.pdf".into(),
        byte_len: 1,
        header: "%PDF-1.7".into(),
        version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
        page_count: 1,
        pages: vec![ParsedPage {
            page_number: 0,
            width: 200.,
            height: 200.,
            rotation: 0,
        }],
        text_elements: texts,
        graphics: Vec::new(),
        image_elements: Vec::new(),
        embedded_fonts: Vec::new(),
        text_preview: String::new(),
        quality: Default::default(),
    };
    (parsed, grouped, model)
}

/// 下側の物理行を照合から落とさず、開始セルの表示行・容量・runへ含める。
#[test]
fn restart_cell_retains_all_source_rows_and_run_styles() {
    let (parsed, grouped, model) = source();
    let result = cells(&parsed, &[0, 80], &[0, 40, 80], &grouped, &model);
    let root = result[0][0]
        .as_ref()
        .expect("merged native cell must include source from every physical row");
    assert_eq!((root.left, root.top, root.bottom), (0., 80., 0.));
    assert_eq!(
        root.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["TOP", "lower"]
    );
    assert_eq!(
        root.lines
            .iter()
            .map(structured_line_max_font_size)
            .collect::<Vec<_>>(),
        [13., 9.]
    );
    assert!(result[1][0].is_none());
    let paragraph =
        table_cell(root).expect("the original two line boxes fit the merged source height");
    use docx_rs::BuildXML;
    let xml = String::from_utf8(paragraph.build()).unwrap();
    assert!(xml.contains("w:sz w:val=\"26\"") && xml.contains("w:sz w:val=\"18\""));
    assert!(xml.contains("123456") && xml.contains("654321"));
    assert!(xml.contains("Courier New"));
    assert_eq!(xml.matches(">TOP</w:t>").count(), 1);
    assert_eq!(xml.matches(">lower</w:t>").count(), 1);
}

/// 一行のstyle対応が不明、または原本原点がセル外なら配置を捏造せず従来fallbackへ戻す。
#[test]
fn single_line_cell_without_verified_style_or_nonnegative_origin_retains_fallback() {
    let (parsed, grouped, model) = source();
    let result = cells(&parsed, &[0, 80], &[0, 40, 80], &grouped, &model);
    let mut cell = result[0][0].as_ref().unwrap().clone();
    cell.lines.truncate(1);
    cell.single_line_runs = cell.lines[0].runs.clone();
    assert!(single_line_horizontal_indent(&cell).is_some());
    let mut unknown_style = cell.clone();
    unknown_style.single_line_runs.clear();
    assert!(single_line_horizontal_indent(&unknown_style).is_none());
    cell.left = cell.lines[0].left_x + 1.;
    assert!(
        single_line_horizontal_indent(&cell).is_none(),
        "negative source cell inset must retain fallback"
    );
}

/// 元の一行が右罫線を越える推定gridでは、indentで残り幅を減らして折り返さない。
#[test]
fn single_line_cell_crossing_source_right_edge_retains_fallback() {
    let (parsed, grouped, model) = source();
    for (right, fits) in [(35, false), (40, true), (80, true)] {
        let result = cells(&parsed, &[0, right], &[0, 40, 80], &grouped, &model);
        let mut cell = result[0][0].as_ref().unwrap().clone();
        cell.lines.truncate(1);
        cell.single_line_runs = cell.lines[0].runs.clone();
        assert_eq!(
            single_line_horizontal_indent(&cell).is_some(),
            fits,
            "source single-line right edge must fit its source cell before applying indent"
        );
    }
}
