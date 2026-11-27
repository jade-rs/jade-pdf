//! 表示枠の外と確認できた文字と画像を、通常変換のレイアウトへ再投入しない。
use super::{ParsedBounds, ParsedDocument};
use jade_pdf2docx_parser::ParsedPage;
use std::collections::BTreeMap;

/// ページ番号から、重複や未知の寸法をNoneで保持した表示寸法への索引。
type Frames = BTreeMap<usize, Option<(f32, f32)>>;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 所有する変換用IRだけを絞り、公開parserの抽出結果や未確認の寸法は変えない。
pub(super) fn retain(parsed: &mut ParsedDocument) -> bool {
    let before = (parsed.text_elements.len(), parsed.image_elements.len());
    let frames = frames(&parsed.pages);
    parsed
        .text_elements
        .retain(|t| !outside(&frames, t.page, t.bounds));
    parsed
        .image_elements
        .retain(|i| !outside(&frames, i.page, i.bounds));
    // 部分的に表示された表の接続を壊さないよう、罫線は個々に除外しない。
    before != (parsed.text_elements.len(), parsed.image_elements.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 任意のページ配置の索引を通して、公開変換と同じ判定を呼ぶ。
    fn outside(pages: &[ParsedPage], page: usize, bounds: ParsedBounds) -> bool {
        super::outside(&frames(pages), page, bounds)
    }

    /// 認識済みの非正方形ページを作る。
    fn page() -> ParsedPage {
        ParsedPage {
            page_number: 0,
            width: 200.,
            height: 300.,
            rotation: 0,
        }
    }

    /// 完全な表示外の面積を持つ矩形を作る。
    fn bounds() -> ParsedBounds {
        ParsedBounds {
            x0: 250.,
            x1: 270.,
            y0: 100.,
            y1: 110.,
        }
    }

    /// ページ欠落・重複・非有限寸法・未対応角度では、推測で原本要素を消さない。
    #[test]
    fn unknown_or_ambiguous_page_frames_preserve_elements() {
        assert!(!outside(&[], 0, bounds()));
        assert!(!outside(&[page(), page()], 0, bounds()));
        for p in [
            ParsedPage {
                width: f32::NAN,
                ..page()
            },
            ParsedPage {
                width: 0.,
                ..page()
            },
            ParsedPage {
                rotation: 45,
                ..page()
            },
        ] {
            assert!(!outside(&[p], 0, bounds()));
        }
        assert!(!outside(&[page()], 1, bounds()));
    }

    /// 一部だけ交差する面積と、有限な面積を確認できないboundsは従来経路へ残す。
    #[test]
    fn partial_or_unconfirmed_bounds_are_not_discarded() {
        assert!(!outside(
            &[page()],
            0,
            ParsedBounds {
                x0: 190.,
                x1: 210.,
                ..bounds()
            }
        ));
        assert!(!outside(
            &[page()],
            0,
            ParsedBounds {
                x1: 250.,
                ..bounds()
            }
        ));
        assert!(!outside(
            &[page()],
            0,
            ParsedBounds {
                x0: f32::NAN,
                ..bounds()
            }
        ));
        assert!(outside(&[page()], 0, bounds()));
    }

    /// 部分表の構造を壊さないよう、罫線はこの文字・画像の表示判定で個別に除外しない。
    #[test]
    fn graphic_rules_remain_available_for_partial_table_structure() {
        let source=fixture::pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 200 300] >>".into(),
            "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
            fixture::stream("1 w 10 400 m 190 400 l S BT /F1 12 Tf 20 30 Td (Visible) Tj ET BT /F1 12 Tf 400 30 Td (Outside) Tj ET"),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        ]);
        let mut parsed =
            jade_pdf2docx_parser::parse_pdf_bytes("owned-unmodified-graphic.pdf", &source).unwrap();
        let graphics = parsed.graphics.clone();
        assert_eq!(graphics.len(), 1);
        assert!(retain(&mut parsed));
        assert_eq!(parsed.graphics, graphics);
        assert_eq!(parsed.text_elements.len(), 1);
        assert_eq!(parsed.text_elements[0].text, "Visible");
    }
}

/// 一度だけページ群を走査し、要素ごとの全ページ再走査を避けて一意な寸法を記録する。
fn frames(pages: &[ParsedPage]) -> Frames {
    let mut out = Frames::new();
    for page in pages {
        out.entry(page.page_number)
            .and_modify(|v| *v = None)
            .or_insert_with(|| frame(page));
    }
    out
}

/// 有限な寸法と直角回転から、正規化済み座標の表示寸法を得る。
fn frame(page: &ParsedPage) -> Option<(f32, f32)> {
    if !page.width.is_finite()
        || !page.height.is_finite()
        || page.width <= 0.
        || page.height <= 0.
        || page.rotation.rem_euclid(90) != 0
    {
        return None;
    }
    Some(if page.rotation.rem_euclid(180) == 90 {
        (page.height, page.width)
    } else {
        (page.width, page.height)
    })
}

/// 面積を確認できる文字・画像が、確認済み表示矩形と完全に離れているか判定する。
fn outside(frames: &Frames, page: usize, bounds: ParsedBounds) -> bool {
    let Some((width, height)) = frames.get(&page).copied().flatten() else {
        return false;
    };
    if ![bounds.x0, bounds.x1, bounds.y0, bounds.y1]
        .into_iter()
        .all(f32::is_finite)
    {
        return false;
    }
    let (x0, x1, y0, y1) = (
        bounds.x0.min(bounds.x1),
        bounds.x0.max(bounds.x1),
        bounds.y0.min(bounds.y1),
        bounds.y0.max(bounds.y1),
    );
    if x0 >= x1 || y0 >= y1 {
        return false;
    }
    x1 <= 0. || y1 <= 0. || x0 >= width || y0 >= height
}
