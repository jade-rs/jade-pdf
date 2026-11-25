//! 罫線表と混在する反復列の表で、source書式と幾何を失わない公開byte変換回帰。
use super::*;
use quick_xml::events::BytesStart;

/// 一つのWord runの文字と検査対象書式。
#[derive(Debug, Default)]
struct StyledRun {
    /// 所属するWord表の出現順。
    table: usize,
    /// runの本文。
    text: String,
    /// 明示されたASCII font family。
    font: Option<String>,
    /// half point単位の明示サイズ。
    size: Option<usize>,
    /// 太字の明示状態。
    bold: Option<bool>,
    /// RGB色。
    color: Option<String>,
}

/// Word属性の値をUTF-8で読む。
fn attribute(tag: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    tag.attributes()
        .map(Result::unwrap)
        .find(|a| a.key.as_ref() == name)
        .map(|a| String::from_utf8(a.value.into_owned()).unwrap())
}

/// 複数の表を区別し、実際のrunへ付いた書式を取得する。
fn styled_runs(xml: &str) -> Vec<StyledRun> {
    let mut reader = Reader::from_str(xml);
    let mut result = Vec::new();
    let mut table_count = 0usize;
    let mut depth = 0usize;
    let mut run = None::<StyledRun>;
    let mut in_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(t) if t.name().as_ref() == b"w:tbl" => {
                table_count += 1;
                depth += 1;
            }
            Event::End(t) if t.name().as_ref() == b"w:tbl" => depth -= 1,
            Event::Start(t) if t.name().as_ref() == b"w:r" && depth > 0 => {
                run = Some(StyledRun {
                    table: table_count - 1,
                    ..Default::default()
                })
            }
            Event::Empty(t) => {
                if let Some(run) = run.as_mut() {
                    match t.name().as_ref() {
                        b"w:rFonts" => run.font = attribute(&t, b"w:ascii"),
                        b"w:sz" => run.size = attribute(&t, b"w:val").map(|v| v.parse().unwrap()),
                        b"w:b" => {
                            run.bold = Some(!matches!(
                                attribute(&t, b"w:val").as_deref(),
                                Some("0" | "false" | "off")
                            ))
                        }
                        b"w:color" => run.color = attribute(&t, b"w:val"),
                        _ => (),
                    }
                }
            }
            Event::Start(t) if t.name().as_ref() == b"w:t" => in_text = true,
            Event::End(t) if t.name().as_ref() == b"w:t" => in_text = false,
            Event::Text(t) if in_text => {
                if let Some(run) = run.as_mut() {
                    run.text.push_str(&t.decode().unwrap());
                }
            }
            Event::End(t) if t.name().as_ref() == b"w:r" => {
                if let Some(run) = run.take() {
                    if !run.text.is_empty() {
                        result.push(run);
                    }
                }
            }
            Event::Eof => break,
            _ => (),
        }
    }
    result
}

/// 元のHelvetica 10ptを、Wordの既定書体へ落とさず保持する。
#[test]
fn new_borderless_cells_retain_source_font_and_size() {
    let xml = document_xml(&source(&[true, false], 40, 240));
    let rows = styled_runs(&xml);
    let runs = rows.iter().filter(|r| r.table == 1).collect::<Vec<_>>();
    assert_eq!(runs.len(), 6);
    for run in runs {
        assert_eq!(run.font.as_deref(), Some("Arial"), "{run:?}");
        assert_eq!(run.size, Some(20), "{run:?}");
    }
}

/// 同じセルにある通常文字と赤い12pt太字を、別runのまま取り込む。
#[test]
fn new_borderless_cells_retain_mixed_source_run_styles() {
    for pages in [[true, false], [false, true]] {
        let xml = document_xml(&source_with_palette(&pages, 40, 240, None, None, true));
        assert_eq!(
            tables_in_xml(&xml),
            pages.into_iter().map(expected).collect::<Vec<_>>()
        );
        let grid_page = pages.iter().position(|ruled| !ruled).unwrap();
        let runs = styled_runs(&xml);
        for row in 0..3 {
            let suffix = format!("R{row}C1");
            let styled = runs
                .iter()
                .find(|r| r.table == grid_page && r.text == suffix)
                .expect("styled suffix must retain its own run");
            assert_eq!(styled.font.as_deref(), Some("Arial"));
            assert_eq!(styled.size, Some(24));
            assert_eq!(styled.bold, Some(true));
            assert_eq!(styled.color.as_deref(), Some("FF0000"));
        }
    }
}

/// 末列を通常体の概算幅で切らず、原本Helvetica-Boldのencoded advanceまで確保する。
#[test]
fn terminal_mixed_style_cell_covers_its_source_glyph_advances() {
    let xml = document_xml(&source_with_palette(
        &[true, false],
        40,
        240,
        None,
        None,
        true,
    ));
    let mut reader = Reader::from_str(&xml);
    let mut widths = Vec::<usize>::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Empty(tag) if tag.name().as_ref() == b"w:gridCol" => {
                widths.push(attribute(&tag, b"w:w").unwrap().parse().unwrap())
            }
            Event::Eof => break,
            _ => (),
        }
    }
    assert_eq!(widths.len(), 4);
    // GRID-: 28.33pt、R0C1: 30.672pt。固定ReportLabのglyph幅と符号名表で照合。
    assert!((widths[3] as f32 / 20. - 59.002).abs() < 0.05, "{widths:?}");
}

/// 大きいfontで求めた行上端から、小さいfontだけのセルの原本上端までの差を保つ。
#[test]
fn smaller_source_cells_keep_their_own_top_within_a_mixed_size_row() {
    let xml = document_xml(&source_with_palette(
        &[true, false],
        40,
        240,
        None,
        None,
        true,
    ));
    let mut reader = Reader::from_str(&xml);
    let mut table = 0;
    let mut before = Vec::<u32>::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:tbl" => table += 1,
            Event::Empty(tag) if table == 2 && tag.name().as_ref() == b"w:spacing" => {
                before.push(attribute(&tag, b"w:before").unwrap().parse().unwrap());
            }
            Event::Eof => break,
            _ => (),
        }
    }
    assert_eq!(before, vec![40, 0, 40, 0, 40, 0]);
}

proptest! {
    /// 原点・列間隔・ページ順を変えても、太字の原文断片と対応する書式を失わない。
    #[test]
    fn mixed_source_styles_follow_their_cells(
        left in 20u16..80,
        width in 160u16..250,
        styled_first in any::<bool>(),
    ) {
        let pages = if styled_first { [false, true] } else { [true, false] };
        let xml = document_xml(&source_with_palette(&pages, left, width, None, None, true));
        prop_assert_eq!(tables_in_xml(&xml), pages.into_iter().map(expected).collect::<Vec<_>>());
        let runs = styled_runs(&xml);
        let grid_page = usize::from(!styled_first);
        let bold = runs.iter().filter(|run| run.table == grid_page && run.bold == Some(true)).collect::<Vec<_>>();
        prop_assert_eq!(bold.len(), 3);
        for (row, run) in bold.into_iter().enumerate() {
            prop_assert_eq!(&run.text, &format!("R{row}C1"));
            prop_assert_eq!(run.size, Some(24));
            prop_assert_eq!(run.color.as_deref(), Some("FF0000"));
        }
    }
}

/// 等分列・平均行高で原本の反復位置を変えず、borderlessセルへ罫線を発明しない。
#[test]
fn new_borderless_table_uses_source_columns_rows_and_unruled_edges() {
    let xml = document_xml(&source(&[true, false], 40, 240));
    let mut reader = Reader::from_str(&xml);
    let mut table = 0usize;
    let mut depth = 0usize;
    let mut widths = Vec::<usize>::new();
    let mut heights = Vec::<f32>::new();
    let mut alignments = Vec::new();
    let mut borders = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(t) if t.name().as_ref() == b"w:tbl" => {
                table += 1;
                depth += 1;
            }
            Event::End(t) if t.name().as_ref() == b"w:tbl" => depth -= 1,
            Event::Empty(t) if table == 2 && depth > 0 => match t.name().as_ref() {
                b"w:gridCol" => widths.push(attribute(&t, b"w:w").unwrap().parse().unwrap()),
                b"w:trHeight" => heights.push(attribute(&t, b"w:val").unwrap().parse().unwrap()),
                b"w:vAlign" => alignments.push(attribute(&t, b"w:val").unwrap()),
                b"w:top" | b"w:bottom" | b"w:left" | b"w:right" | b"w:insideH" | b"w:insideV" => {
                    if let Some(v) = attribute(&t, b"w:val") {
                        borders.push(v);
                    }
                }
                _ => (),
            },
            Event::Eof => break,
            _ => (),
        }
    }
    assert_eq!(widths.len(), 2);
    assert_eq!(widths[0], 4800);
    assert!(widths[1] > 0 && widths[1] < widths[0]);
    assert_eq!(heights, vec![1200., 1200., 200.]);
    assert_eq!(alignments, vec!["top"; 6]);
    assert!(!borders.iter().any(|v| v == "single"), "{borders:?}");
}

/// 各source sectionに保存された原点を表示順に読む。
fn section_origins(xml: &str) -> Vec<(String, String)> {
    let mut reader = Reader::from_str(xml);
    let mut origins = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Empty(tag) if tag.name().as_ref() == b"w:pgMar" => origins.push((
                attribute(&tag, b"w:left").unwrap(),
                attribute(&tag, b"w:top").unwrap(),
            )),
            Event::Eof => return origins,
            _ => (),
        }
    }
}

/// 同寸の混在ページでも、罫線ページの原点を別ページの表へ流用しない。
#[test]
fn mixed_source_tables_keep_each_pages_own_origin() {
    for palette in [false, true] {
        for pages in [[true, false], [false, true]] {
            let independent = pages
                .iter()
                .flat_map(|ruled| {
                    section_origins(&document_xml(&source_with_palette(
                        &[*ruled],
                        40,
                        240,
                        None,
                        None,
                        palette,
                    )))
                })
                .collect::<Vec<_>>();
            let xml = document_xml(&source_with_palette(&pages, 40, 240, None, None, palette));
            assert_eq!(section_origins(&xml), independent);
            assert!(!xml.contains("<w:br w:type=\"page\""));
        }
    }
}

/// 別の新規一時領域だけへ、色と太字を含む所有原本を書き出す。
#[test]
#[ignore = "new JADE_PAGE_LOCAL_STYLE_EXPORT_DIR required; review every source page before file conversion"]
fn export_owned_mixed_source_style_pages_before_file_product() {
    let dir =
        std::path::PathBuf::from(std::env::var_os("JADE_PAGE_LOCAL_STYLE_EXPORT_DIR").unwrap());
    std::fs::create_dir(&dir).unwrap();
    for (name, pages) in [
        ("ruled-then-styled-grid", [true, false]),
        ("styled-grid-then-ruled", [false, true]),
    ] {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(format!("{name}.pdf")))
            .unwrap();
        std::io::Write::write_all(
            &mut file,
            &source_with_palette(&pages, 40, 240, None, None, true),
        )
        .unwrap();
    }
}
