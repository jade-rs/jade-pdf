//! 段の区切り線で分けたページの表を、段ごとのセルに横に並べる回帰。
use super::*;

/// 区切り線の左右に表を 1 つずつ置いたページ（2 ページ目は通常の本文）。
fn banded(full_width_below: bool) -> String {
    let mut parsed = source(2, &[]);
    parsed.text_elements.retain(|text| text.page != 0);
    parsed.graphics.push(ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds {
            x0: 290.,
            y0: 200.,
            x1: 290.,
            y1: 742.,
        },
        stroke_width: 0.5,
        dashed: false,
    });
    // 区切り線とみなすには、その左右に字が要る（表のセルの字）。
    for (x, label) in [(52., "L"), (334., "R")] {
        for row in 0..6 {
            let y = 505. + row as f32 * 9.;
            parsed.text_elements.push(ParsedTextElement {
                baseline_y: Some(y),
                page: 0,
                bounds: ParsedBounds {
                    x0: x,
                    y0: y,
                    x1: x + 20.,
                    y1: y + 6.,
                },
                font_size: 6.,
                font_name: Some("Helvetica".into()),
                fill_color: None,
                text: format!("{label}{row}"),
            });
        }
    }
    if full_width_below {
        parsed.text_elements.push(ParsedTextElement {
            baseline_y: Some(120.),
            page: 0,
            bounds: ParsedBounds {
                x0: 48.,
                y0: 120.,
                x1: 540.,
                y1: 131.,
            },
            font_size: 11.,
            font_name: Some("Helvetica".into()),
            fill_color: None,
            text: "FULL WIDTH NOTE BELOW BOTH BANDS".into(),
        });
    }
    let left = positioned_table(0);
    let mut right = positioned_table(0);
    right.bounds.x0 = 330.;
    right.bounds.x1 = 530.;
    right.table = OcrTable::new(vec![
        vec!["RIGHT R0C0".into(), "RIGHT R0C1".into()],
        vec!["RIGHT R1C0".into(), "RIGHT R1C1".into()],
    ]);
    packaged(parsed, &[left, right])
}

/// 区切り線の左右の表は、罫線の無い 1 行の表のセルに入れて横に並べる。
#[test]
fn tables_on_both_sides_of_a_band_rule_sit_side_by_side() {
    let xml = banded(false);
    let first = &xml[xml.find("<w:tbl>").unwrap()..];
    // 外の表の中に 2 つの表があり、外の表は 2 列。
    assert_eq!(xml.matches("<w:tbl>").count(), 3, "{xml}");
    assert_eq!(
        first[..first.find("</w:tblGrid>").unwrap()]
            .matches("<w:gridCol ")
            .count(),
        2
    );
    let left = xml.find("TABLE PAGE0 R0C0").unwrap();
    let right = xml.find("RIGHT R0C0").unwrap();
    assert!(left < right);
}

/// 両方の段の下にある全幅の行は、段の表の後に全幅の段落として組む。
#[test]
fn a_full_width_line_below_the_bands_follows_the_band_table() {
    let xml = banded(true);
    assert_eq!(xml.matches("<w:tbl>").count(), 3, "{xml}");
    let note = xml.find("FULL WIDTH NOTE BELOW BOTH BANDS").unwrap();
    let last_table_end = xml.rfind("</w:tbl>").unwrap();
    assert!(note > last_table_end);
}

/// ページの端の狭い帯にある、字数に比べて幅のごく狭い行は、下から上へ読む段落にする。
#[test]
fn a_narrow_margin_line_is_set_sideways() {
    let line = |text: &str, left_x: f32, right_x: f32| StructuredTextLine {
        top_y: 500.,
        left_x,
        right_x,
        alignment: ParagraphAlignment::Left,
        runs: vec![StructuredTextRun {
            text: text.into(),
            style: StructuredTextStyle::default(),
        }],
        split_from_row: false,
        text: text.into(),
    };
    assert!(sideways_margin_line(&[line("令和八年八月十三日官報", 41., 54.)], 57., 842.).is_some());
    // 横に組んだ普通の行、本文の段の幅の帯は対象外。
    assert!(sideways_margin_line(&[line("令和八年", 10., 50.)], 57., 842.).is_none());
    assert!(
        sideways_margin_line(&[line("令和八年八月十三日官報", 41., 54.)], 200., 842.).is_none()
    );
}

/// 横罫線に接しない科目の列の外枠は、右の金額の列の一番上・一番下の横罫線の高さで、金額の列の
/// 縦罫線とつなぐ。間にほかの罫線がある外枠はつながない。
#[test]
fn a_lone_frame_rule_is_bridged_to_the_amount_column() {
    let rule = |x0: f32, y0: f32, x1: f32, y1: f32| ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds { x0, y0, x1, y1 },
        stroke_width: 0.36,
        dashed: false,
    };
    let mut graphics = vec![
        rule(437., 46., 437., 595.),
        rule(668., 46., 668., 595.),
        rule(780., 46., 780., 595.),
    ];
    for y in [46., 63., 81., 171., 459.] {
        graphics.push(rule(668., y, 780., y));
    }
    let bridged = bridge_lone_frame_rules(graphics.clone());
    let added = bridged[graphics.len()..]
        .iter()
        .map(|graphic| (graphic.bounds.x0, graphic.bounds.x1, graphic.bounds.y0))
        .collect::<Vec<_>>();
    assert_eq!(added, vec![(437., 668., 46.), (437., 668., 459.)]);

    let mut divided = graphics.clone();
    divided.push(rule(500., 200., 600., 200.));
    assert_eq!(
        bridge_lone_frame_rules(divided.clone()).len(),
        divided.len()
    );
}

/// 同じ高さの科目と金額は 1 つの段落にまとめ、金額は科目の段落に付ける。本文の行どうしや、
/// 高さの違う金額はまとめない。
#[test]
fn amounts_on_a_label_row_join_the_label_paragraph() {
    let line = |text: &str, left_x: f32, right_x: f32, top_y: f32| StructuredTextLine {
        top_y,
        left_x,
        right_x,
        alignment: ParagraphAlignment::Left,
        runs: vec![StructuredTextRun {
            text: text.into(),
            style: StructuredTextStyle::default(),
        }],
        split_from_row: true,
        text: text.into(),
    };
    let block = |flow_index: usize, line: StructuredTextLine| StructuredBlock::Paragraph {
        top_y: line.top_y,
        flow_index,
        line,
    };
    let mut blocks = vec![
        block(0, line("有価証券", 97., 128., 543.)),
        block(1, line("18,346", 280., 305., 543.)),
        block(2, line("たな卸資産", 97., 137., 528.)),
        block(3, line("112", 290., 305., 520.)),
        block(4, line("左の段の本文の行", 97., 180., 500.)),
        block(5, line("右の段の本文の行", 330., 420., 500.)),
    ];
    let attached = attach_row_amounts(&mut blocks);
    assert_eq!(blocks.len(), 5);
    assert_eq!(
        attached
            .iter()
            .map(|(flow, lines)| (
                *flow,
                lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>()
            ))
            .collect::<Vec<_>>(),
        vec![(0, vec!["18,346".to_string()])]
    );
}

/// 左右の外枠が全高にわたり、行の横罫線が左端の狭い列の右から引かれた表（上端は開いている）は
/// 表の格子として扱う。行の横罫線が足りない枠は扱わない。
#[test]
fn a_side_framed_table_with_an_open_top_is_a_grid() {
    let rule = |x0: f32, y0: f32, x1: f32, y1: f32| ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds { x0, y0, x1, y1 },
        stroke_width: 0.36,
        dashed: false,
    };
    let mut graphics = vec![
        rule(83., 359., 83., 549.),
        rule(115., 359., 115., 549.),
        rule(411., 359., 411., 549.),
        rule(83., 359., 411., 359.),
    ];
    for y in [377., 421., 439., 482., 500.] {
        graphics.push(rule(115., y, 411., y));
    }
    let xs = [83, 115, 180, 411];
    let ys = [359, 377, 421, 439, 482, 500, 546];
    assert!(has_side_framed_grid_lines(&graphics, &xs, &ys));
    graphics.retain(|graphic| graphic.bounds.y0 != 439.);
    assert!(!has_side_framed_grid_lines(&graphics, &xs, &ys));
}

/// ページの高さの一部だけを区切る縦罫線の左右の段と、その下の全幅の表は、段を横に並べた表の
/// 後に全幅の表を置く。
#[test]
fn a_partial_rule_splits_columns_above_a_full_width_table() {
    let mut parsed = source(2, &[]);
    parsed.text_elements.retain(|text| text.page != 0);
    parsed.graphics.push(ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds {
            x0: 290.,
            y0: 420.,
            x1: 290.,
            y1: 742.,
        },
        stroke_width: 0.5,
        dashed: false,
    });
    for (x, label) in [(52., "LEFT"), (334., "RIGHT")] {
        for row in 0..6 {
            let y = 700. - row as f32 * 14.;
            parsed.text_elements.push(ParsedTextElement {
                baseline_y: Some(y),
                page: 0,
                bounds: ParsedBounds {
                    x0: x,
                    y0: y,
                    x1: x + 90.,
                    y1: y + 9.,
                },
                font_size: 9.,
                font_name: Some("Helvetica".into()),
                fill_color: None,
                text: format!("{label} COLUMN {row}"),
            });
        }
    }
    let mut wide = positioned_table(0);
    wide.bounds = ParsedBounds {
        x0: 48.,
        y0: 200.,
        x1: 540.,
        y1: 300.,
    };
    let xml = packaged(parsed, &[wide]);
    let container = xml.find("LEFT COLUMN 0").unwrap();
    let right = xml.find("RIGHT COLUMN 0").unwrap();
    let wide_table = xml.find("TABLE PAGE0 R0C0").unwrap();
    // 左右の段は同じ外の表の行に並び、全幅の表はその後。
    assert!(container < right && right < wide_table, "{xml}");
    assert!(xml[..right].matches("<w:tbl>").count() >= 1);
}

/// 横に何も無い狭い表は字の幅に合わせて組まれたとみなし、横に字がある表や広い表はみなさない。
#[test]
fn a_narrow_table_with_nothing_beside_it_has_its_natural_width() {
    let mut parsed = source(2, &[]);
    parsed.text_elements.retain(|text| text.page != 0);
    let xs = [100, 180, 260];
    let ys = [500, 540, 580];
    assert!(table_has_natural_width(&parsed, 0, &xs, &ys));
    assert!(!table_has_natural_width(&parsed, 0, &[40, 300, 560], &ys));
    parsed.text_elements.push(ParsedTextElement {
        baseline_y: Some(520.),
        page: 0,
        bounds: ParsedBounds {
            x0: 330.,
            y0: 520.,
            x1: 400.,
            y1: 530.,
        },
        font_size: 10.,
        font_name: None,
        fill_color: None,
        text: "BESIDE".into(),
    });
    assert!(!table_has_natural_width(&parsed, 0, &xs, &ys));
}
