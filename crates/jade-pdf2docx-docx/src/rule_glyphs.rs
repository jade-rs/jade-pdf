//! 字で組んだ罫線（罫線用フォントの字・線の形の私用領域の字）を、罫線の graphics に置き換える。
//!
//! 官報の財務諸表は、合計の下の一重・二重の下線や表の罫線を罫線用フォント（MFHSLine など）の
//! 字で組む。字として落とすと下線や罫線が消えるので、細長い字は罫線として残し、表の罫線・
//! セルの下線の推定に使う（lawrenceanum の kanpo-pdf が、輪郭が長方形の字形を罫線・下線に
//! するのと同じ考え）。

use super::*;

/// 罫線とみなす字の太さの上限（pt）。
const MAX_THICKNESS_PT: f32 = 1.5;
/// 罫線とみなす字の長さの下限（pt）。
const MIN_LENGTH_PT: f32 = 3.0;

/// 罫線用の字だけからなる文字片か（罫線用フォント、または私用領域・CID の字だけ）。
fn is_rule_glyph_text(text: &ParsedTextElement) -> bool {
    let body = text.text.trim();
    if body.is_empty() {
        return false;
    }
    let rule_font = text
        .font_name
        .as_deref()
        .is_some_and(|font| font.contains("Line"));
    rule_font
        || body.chars().all(|ch| {
            ('\u{E000}'..='\u{F8FF}').contains(&ch) || raw_cid_glyphs::raw_cid(ch).is_some()
        })
}

/// 細長い罫線用の字を、同じ位置の罫線 graphics に置き換える。
pub(super) fn convert_to_graphics(parsed: &mut ParsedDocument) {
    let mut rules = Vec::new();
    parsed.text_elements.retain(|text| {
        if !is_rule_glyph_text(text) {
            return true;
        }
        let bounds = normalized_bounds(text.bounds);
        let (width, height) = (bounds.x1 - bounds.x0, bounds.y1 - bounds.y0);
        let rule = if height <= MAX_THICKNESS_PT && width >= MIN_LENGTH_PT {
            let y = (bounds.y0 + bounds.y1) / 2.0;
            Some((
                ParsedBounds {
                    x0: bounds.x0,
                    y0: y,
                    x1: bounds.x1,
                    y1: y,
                },
                height,
            ))
        } else if width <= MAX_THICKNESS_PT && height >= MIN_LENGTH_PT {
            let x = (bounds.x0 + bounds.x1) / 2.0;
            Some((
                ParsedBounds {
                    x0: x,
                    y0: bounds.y0,
                    x1: x,
                    y1: bounds.y1,
                },
                width,
            ))
        } else {
            None
        };
        let Some((rule_bounds, thickness)) = rule else {
            return true;
        };
        rules.push(ParsedGraphicElement {
            page: text.page,
            bounds: rule_bounds,
            stroke_width: thickness.clamp(0.25, MAX_THICKNESS_PT),
            dashed: false,
        });
        false
    });
    parsed.graphics.extend(rules);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用の文字片。
    fn text(font: &str, body: &str, bounds: (f32, f32, f32, f32)) -> ParsedTextElement {
        ParsedTextElement {
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds {
                x0: bounds.0,
                y0: bounds.1,
                x1: bounds.2,
                y1: bounds.3,
            },
            font_size: 8.0,
            font_name: Some(font.to_string()),
            fill_color: None,
            text: body.to_string(),
        }
    }

    /// 罫線用フォントの細長い字は横・縦の罫線になり、ほかの字は残る。
    #[test]
    fn thin_rule_font_glyphs_become_rules() {
        let mut parsed = ParsedDocument {
            quality: QualityReport::default(),
            path: std::path::PathBuf::new(),
            byte_len: 0,
            header: "%PDF-1.4".into(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 4 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            fills: Vec::new(),
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
            text_elements: vec![
                text(
                    "CMFEPC+MFHSLine-W3",
                    "\u{E005}\u{E005}",
                    (249.0, 463.3, 296.9, 463.8),
                ),
                text("CMFEPC+MFHSLine-W3", "娣娣娣", (359.5, 350.8, 360.0, 390.9)),
                text(
                    "DFHSMinchoRPro6N-W3",
                    "\u{EA75}",
                    (339.5, 537.3, 340.5, 539.0),
                ),
                text("DFHSMinchoRPro6N-W3", "合計", (100.0, 500.0, 116.0, 508.0)),
            ],
        };
        convert_to_graphics(&mut parsed);
        assert_eq!(parsed.text_elements.len(), 2);
        let rules = parsed
            .graphics
            .iter()
            .map(|graphic| {
                let b = graphic.bounds;
                (b.x0, b.y0, b.x1, b.y1)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rules,
            vec![
                (249.0, 463.55, 296.9, 463.55),
                (359.75, 350.8, 359.75, 390.9)
            ]
        );
    }
}
