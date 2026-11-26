//! 表示枠の外と確認できた要素を、通常変換のレイアウトへ再投入しない。
use super::{ParsedBounds, ParsedDocument};
use jade_pdf2docx_parser::ParsedPage;
use std::collections::BTreeMap;

/// ページ番号から、重複や未知の寸法をNoneで保持した表示寸法への索引。
type Frames = BTreeMap<usize, Option<(f32, f32)>>;

/// 所有する変換用IRだけを絞り、公開parserの抽出結果や未確認の寸法は変えない。
pub(super) fn retain(parsed: &mut ParsedDocument) -> bool {
    let before = (
        parsed.text_elements.len(),
        parsed.image_elements.len(),
        parsed.graphics.len(),
    );
    let frames = frames(&parsed.pages);
    parsed
        .text_elements
        .retain(|t| !outside(&frames, t.page, t.bounds, None));
    parsed
        .image_elements
        .retain(|i| !outside(&frames, i.page, i.bounds, None));
    parsed
        .graphics
        .retain(|g| !outside(&frames, g.page, g.bounds, Some(g.stroke_width)));
    before
        != (
            parsed.text_elements.len(),
            parsed.image_elements.len(),
            parsed.graphics.len(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 任意のページ配置の索引を通して、公開変換と同じ判定を呼ぶ。
    fn outside(
        pages: &[ParsedPage],
        page: usize,
        bounds: ParsedBounds,
        stroke: Option<f32>,
    ) -> bool {
        super::outside(&frames(pages), page, bounds, stroke)
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
        assert!(!outside(&[], 0, bounds(), None));
        assert!(!outside(&[page(), page()], 0, bounds(), None));
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
            assert!(!outside(&[p], 0, bounds(), None));
        }
        assert!(!outside(&[page()], 1, bounds(), None));
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
            },
            None
        ));
        assert!(!outside(
            &[page()],
            0,
            ParsedBounds {
                x1: 250.,
                ..bounds()
            },
            None
        ));
        assert!(!outside(
            &[page()],
            0,
            ParsedBounds {
                x0: f32::NAN,
                ..bounds()
            },
            None
        ));
        assert!(outside(&[page()], 0, bounds(), None));
    }

    /// 枠際の罫線は中心線だけで落とさず、原本の線幅と境界接触を含めて判定する。
    #[test]
    fn stroked_boundaries_keep_their_visible_edge() {
        let line = ParsedBounds {
            x0: 201.,
            x1: 201.,
            y0: 10.,
            y1: 100.,
        };
        assert!(!outside(&[page()], 0, line, Some(2.)));
        assert!(outside(&[page()], 0, line, Some(1.)));
        assert!(!outside(&[page()], 0, line, Some(f32::NAN)));
        assert!(!outside(&[page()], 0, line, Some(-1.)));
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

/// 面積のある要素または線幅を含む罫線が、確認済み表示矩形と完全に離れているか判定する。
fn outside(frames: &Frames, page: usize, bounds: ParsedBounds, stroke: Option<f32>) -> bool {
    let Some((width, height)) = frames.get(&page).copied().flatten() else {
        return false;
    };
    if ![bounds.x0, bounds.x1, bounds.y0, bounds.y1]
        .into_iter()
        .all(f32::is_finite)
    {
        return false;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (
        bounds.x0.min(bounds.x1),
        bounds.x0.max(bounds.x1),
        bounds.y0.min(bounds.y1),
        bounds.y0.max(bounds.y1),
    );
    if let Some(stroke) = stroke {
        if !stroke.is_finite() || stroke < 0. {
            return false;
        }
        let padding = stroke / 2.;
        x0 -= padding;
        x1 += padding;
        y0 -= padding;
        y1 += padding;
        if ![x0, x1, y0, y1].into_iter().all(f32::is_finite) {
            return false;
        }
        x1 < 0. || y1 < 0. || x0 > width || y0 > height
    } else {
        if x0 >= x1 || y0 >= y1 {
            return false;
        }
        x1 <= 0. || y1 <= 0. || x0 >= width || y0 >= height
    }
}
