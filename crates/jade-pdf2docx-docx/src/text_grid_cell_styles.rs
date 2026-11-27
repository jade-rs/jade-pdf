//! 推定表のセル文字列に完全一致する原本runだけへ書式を対応させる。
use super::*;

/// 原文と完全一致したrunと、同じ行内での原本上端の差。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Cell {
    /// セル本文に対応する原本run。
    runs: Vec<StructuredTextRun>,
    /// 行上端より下へ置く余白。元の座標差だけをtwipsに丸める。
    before_twips: u32,
}

/// セルの文字・書式を照合し、行全体ではなくそのセルの元の上端を保持する。
pub(super) fn cell(
    parsed: &ParsedDocument,
    texts: &[&ParsedTextElement],
    expected: &str,
    row_top: f32,
) -> Option<Cell> {
    let runs = runs(parsed, texts, expected)?;
    let top = texts.iter().try_fold(f32::NEG_INFINITY, |top, text| {
        text.bounds
            .y1
            .is_finite()
            .then_some(top.max(text.bounds.y1))
    })?;
    let before_twips = if texts.is_empty() {
        0
    } else {
        u32::try_from(pdf_points_to_twips((row_top - top).max(0.0))?).ok()?
    };
    Some(Cell { runs, before_twips })
}

/// 新しいコード説明表では原本の字間から本文を組み、同じrunと上端をセルへ渡す。
pub(super) fn source_cell(
    parsed: &ParsedDocument,
    texts: Vec<&ParsedTextElement>,
    row_top: f32,
) -> Option<(String, Cell)> {
    let start = texts.iter().position(|text| !text.text.trim().is_empty())?;
    let end = texts
        .iter()
        .rposition(|text| !text.text.trim().is_empty())?
        + 1;
    let texts = texts[start..end].to_vec();
    if texts.iter().any(|text| {
        pdf_font_size_to_half_points(text.font_size).is_none()
            || ![
                text.bounds.x0,
                text.bounds.x1,
                text.bounds.y0,
                text.bounds.y1,
            ]
            .iter()
            .all(|value| value.is_finite())
            || text.bounds.x1 < text.bounds.x0
            || text.bounds.y1 < text.bounds.y0
    }) {
        return None;
    }
    let width = page_width_for_layout(parsed, texts.first()?.page, &texts);
    let line = structured_text_line_from_segment(
        &TextRowSegment {
            texts,
            split_from_row: false,
        },
        &parsed.graphics,
        width,
    )?;
    let before_twips = u32::try_from(pdf_points_to_twips((row_top - line.top_y).max(0.0))?).ok()?;
    let mut runs = line.runs;
    for run in &mut runs {
        run.style.font_embedded = run.style.font_name.as_deref().is_some_and(|name| {
            parsed
                .embedded_fonts
                .iter()
                .any(|font| font.font_name == name)
        });
    }
    Some((line.text, Cell { runs, before_twips }))
}

/// 行内の原本上端までの空きを付けて照合済みセルを出力する。
pub(super) fn cell_paragraph(cell: &Cell) -> Option<Paragraph> {
    paragraph(&cell.runs, cell.before_twips)
}

/// 元セルと区切り空白まで一致する場合だけ、色・font・サイズを持つrunを返す。
pub(super) fn runs(
    parsed: &ParsedDocument,
    texts: &[&ParsedTextElement],
    expected: &str,
) -> Option<Vec<StructuredTextRun>> {
    if expected.is_empty() {
        return texts.is_empty().then(Vec::new);
    }
    let offsets = texts
        .iter()
        .copied()
        .zip(inline_baseline::offsets(texts))
        .filter_map(|(text, offset)| offset.map(|offset| (std::ptr::from_ref(text), offset)))
        .collect::<BTreeMap<_, _>>();
    let runs = native_table_text::legacy_runs(
        parsed,
        texts.iter().copied(),
        expected,
        &parsed.graphics,
        &offsets,
    );
    (!runs.is_empty()).then_some(runs)
}

/// 位置を推測せず、照合済みのセル本文を原本の各書式と最大em高で出力する。
fn paragraph(runs: &[StructuredTextRun], before_twips: u32) -> Option<Paragraph> {
    if runs.is_empty() {
        return None;
    }
    let height = runs.iter().try_fold(0.0f32, |height, run| {
        pdf_font_size_to_half_points(run.style.font_size)?;
        Some(height.max(run.style.font_size))
    })?;
    Some(
        runs.iter()
            .fold(Paragraph::new(), |paragraph, run| {
                paragraph.add_run(docx_run_from_structured_run(run))
            })
            .line_spacing(native_line_pitch::spacing(height, before_twips, 0)?),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 外部assetを含まない原本断片二つを、文字列対応の拒否条件に使用する。
    fn parsed() -> ParsedDocument {
        ParsedDocument {
            source_diagonals: Vec::new(),
            quality: Default::default(),
            path: "owned-style-identity.pdf".into(),
            byte_len: 1,
            header: "%PDF-1.7".into(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
            text_elements: ["Alpha", "Beta"]
                .into_iter()
                .enumerate()
                .map(|(i, text)| ParsedTextElement {
                    source_background: None,
                    fill_stroke_width: None,
                    page: 0,
                    text: text.into(),
                    font_size: 10.,
                    font_name: Some("Helvetica".into()),
                    font_weight: None,
                    fill_color: None,
                    baseline_y: Some(700.),
                    bounds: ParsedBounds {
                        x0: 10. + i as f32 * 30.,
                        y0: 700.,
                        x1: 30. + i as f32 * 30.,
                        y1: 710.,
                    },
                })
                .collect(),
        }
    }

    /// 文字の集合一致だけで別セル・別順序・異なる空白へ書式を割り当てない。
    #[test]
    fn source_run_mapping_requires_exact_cell_text_and_spaces() {
        let parsed = parsed();
        let texts = parsed.text_elements.iter().collect::<Vec<_>>();
        assert!(runs(&parsed, &texts, "Alpha Beta").is_some());
        for expected in ["AlphaBeta", "Beta Alpha", "Alpha", "Alpha  Beta", ""] {
            assert!(runs(&parsed, &texts, expected).is_none(), "{expected:?}");
        }
        assert_eq!(runs(&parsed, &[], ""), Some(Vec::new()));
        assert!(runs(&parsed, &[], "Alpha").is_none());
    }

    /// 空・未確認のem高をWordの既定値に置き換えず、書式段落への変換を拒否する。
    #[test]
    fn empty_and_nonfinite_styles_do_not_get_a_default_paragraph() {
        assert!(paragraph(&[], 0).is_none());
        for size in [0., -1., f32::NAN, f32::INFINITY] {
            let run = StructuredTextRun {
                text: "Owned".into(),
                style: StructuredTextStyle {
                    font_size: size,
                    ..Default::default()
                },
            };
            assert!(paragraph(&[run], 0).is_none());
        }
        let run = StructuredTextRun {
            text: "Owned".into(),
            style: Default::default(),
        };
        assert!(paragraph(&[run], 0).is_some());
    }
}
