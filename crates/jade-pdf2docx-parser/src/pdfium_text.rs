//! native通常抽出の文字indexと結果保存を予算付きで扱う。
use crate::{ParseError, ParsedBounds, ParsedTextElement, decode::DecodeBudget};
use pdfium_render::prelude::*;

#[cfg(test)]
#[path = "pdfium_text_budget_tests.rs"]
pub(crate) mod budget_tests;

#[cfg(test)]
#[path = "pdfium_segment_recovery_tests.rs"]
mod segment_recovery_tests;

/// i32文字indexの全要求容量を予約してから、共有するページ配列を取得する。
pub(crate) fn characters<'a>(
    budget: &DecodeBudget,
    page_text: &'a PdfPageText<'_>,
) -> Result<PdfPageTextChars<'a>, ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let count = u64::try_from(page_text.len()).map_err(|_| {
        ParseError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "negative native character count",
        ))
    })?;
    budget.charge_metadata_workspace_bytes(count * std::mem::size_of::<i32>() as u64);
    budget.check()?;
    crate::metadata_workspace::ensure_active(budget)?;
    Ok(page_text.chars())
}

/// segment本文と先頭文字の幾何/styleを、共有indexと予算付き保存でIRへ変換する。
pub(crate) fn extract(
    budget: &DecodeBudget,
    path: &std::path::Path,
    page_number: usize,
    page_text: &PdfPageText<'_>,
    characters: &PdfPageTextChars<'_>,
) -> Result<Vec<ParsedTextElement>, ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let mut elements = Vec::new();
    // segment を分ける際に使う、ページの全文字の矩形の中心（文字の index 順）。
    let centers = characters
        .iter()
        .map(|character| {
            character.loose_bounds().ok().map(|bounds| {
                (
                    (bounds.left().value + bounds.right().value) / 2.0,
                    (bounds.bottom().value + bounds.top().value) / 2.0,
                )
            })
        })
        .collect::<Vec<_>>();
    for segment in page_text.segments().iter() {
        crate::metadata_workspace::ensure_active(budget)?;
        // API内部の一時Stringは現APIで確保前の正確な容量取得ができない。
        let mut raw = segment.text();
        crate::pdfium_printed_hyphens::segment(
            budget,
            path,
            &mut raw,
            characters,
            segment.bounds(),
        )?;
        // 描画された空白を単語境界として保持し、SDKの端の改行だけを除く。
        let content = raw.trim_matches(|ch| matches!(ch, '\r' | '\n'));
        if content.is_empty() {
            continue;
        }
        // 表の列のように大きく離れた文字は、同じ行の segment でも別の要素へ分ける。
        if let Some(pieces) = split_at_wide_gaps(&segment, characters, &centers) {
            for piece in pieces {
                let text = copy(budget, Some(piece.text.trim()))?.unwrap_or_default();
                if text.is_empty() {
                    continue;
                }
                let font_name = copy(budget, piece.font_name.as_deref())?;
                let fill_color = copy(budget, piece.fill_color.as_deref())?;
                crate::metadata_workspace::push(
                    budget,
                    &mut elements,
                    ParsedTextElement {
                        baseline_y: piece.baseline_y,
                        page: page_number,
                        bounds: piece.bounds,
                        font_size: piece.font_size,
                        font_name,
                        fill_color,
                        text,
                    },
                )?;
            }
            continue;
        }
        let bounds = parsed_bounds_from_pdf_rect(segment.bounds());
        // 文字を引けない場合は行の太さ（回転した行でも短い辺）を文字サイズとみなす。
        let mut font_size = (bounds.y1 - bounds.y0)
            .abs()
            .min((bounds.x1 - bounds.x0).abs());
        let mut font_name = None;
        let mut fill_color = None;
        let mut baseline_y = None;
        if let Some(first) = first_in_rect(characters, segment.bounds()) {
            font_size = effective_font_size(&first);
            baseline_y = horizontal_baseline(&first);
            let name = first.font_name();
            font_name = copy(budget, (!name.is_empty()).then_some(name.as_str()))?;
            let color = first.fill_color().ok().map(|color| color.to_hex());
            fill_color = copy(budget, color.as_deref())?;
        }
        let text = copy(budget, Some(content))?.unwrap_or_default();
        crate::metadata_workspace::push(
            budget,
            &mut elements,
            ParsedTextElement {
                baseline_y,
                page: page_number,
                bounds,
                font_size,
                font_name,
                fill_color,
                text,
            },
        )?;
    }
    Ok(elements)
}

/// segment を分ける判定に使う 1 文字分の情報。
#[derive(Debug, Clone, PartialEq)]
struct CharInfo {
    /// 文字列（空白を含む）。
    text: String,
    /// 文字の外接矩形。
    bounds: ParsedBounds,
    /// 実効 font size。
    font_size: f32,
    /// font 名。
    font_name: Option<String>,
    /// 塗り色。
    fill_color: Option<String>,
    /// 回転のない水平な文字のベースライン y。
    baseline_y: Option<f32>,
}

/// 大きな字間で分けた segment の一部。
#[derive(Debug, Clone, PartialEq)]
struct SegmentPiece {
    /// 空白以外の文字矩形の和。
    bounds: ParsedBounds,
    /// 文字列。
    text: String,
    /// 先頭文字の実効 font size。
    font_size: f32,
    /// 先頭文字の font 名。
    font_name: Option<String>,
    /// 先頭文字の塗り色。
    fill_color: Option<String>,
    /// 先頭文字のベースライン y。
    baseline_y: Option<f32>,
}

/// 字送り方向の隙間がこの em 数を超えたら別の要素に分ける。
const SEGMENT_SPLIT_GAP_EM: f32 = 1.0;

/// segment の文字を取り出し、1 em を超えて離れた位置で分ける。
///
/// 分ける必要がない場合や文字を取得できない場合は `None` を返し、segment 全体を 1 要素にする。
fn split_at_wide_gaps(
    segment: &PdfPageTextSegment<'_>,
    characters: &PdfPageTextChars<'_>,
    centers: &[Option<(f32, f32)>],
) -> Option<Vec<SegmentPiece>> {
    let rect = segment.bounds();
    let horizontal = rect.width().value >= rect.height().value;
    // `segment.chars()` は回転したページで別の文字を返すことがあるため、ページの文字から
    // 中心が segment の矩形に入る文字をテキスト順に拾う。
    let (left, bottom, right, top) = (
        rect.left().value - 0.5,
        rect.bottom().value - 0.5,
        rect.right().value + 0.5,
        rect.top().value + 0.5,
    );
    let mut infos = Vec::new();
    for (index, center) in centers.iter().enumerate() {
        let Some((center_x, center_y)) = *center else {
            continue;
        };
        if center_x < left || center_x > right || center_y < bottom || center_y > top {
            continue;
        }
        let character = characters.get(index).ok()?;
        let bounds = character.loose_bounds().ok()?;
        let name = character.font_name();
        infos.push(CharInfo {
            text: character.unicode_string().unwrap_or_default(),
            bounds: parsed_bounds_from_pdf_rect(bounds),
            font_size: effective_font_size(&character),
            font_name: (!name.is_empty()).then_some(name),
            fill_color: character.fill_color().ok().map(|color| color.to_hex()),
            baseline_y: horizontal_baseline(&character),
        });
    }
    // 矩形で拾う文字は隣の segment の文字を含むことがあるため、segment の本文と
    // 同じ文字列になる場合だけ分ける。
    let compact = |text: &str| {
        text.chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>()
    };
    let joined = infos
        .iter()
        .map(|info| info.text.as_str())
        .collect::<String>();
    if compact(&joined) != compact(&segment.text()) {
        return None;
    }
    let pieces = group_chars_at_wide_gaps(&infos, horizontal);
    (pieces.len() > 1).then_some(pieces)
}

/// 字送り方向に 1 em を超えて離れた文字の間で、文字列を複数の要素へ分ける。
///
/// 空白文字は直前の要素へ含め、要素の矩形には加えない。
fn group_chars_at_wide_gaps(chars: &[CharInfo], horizontal: bool) -> Vec<SegmentPiece> {
    // 字送りの向き（正なら座標が増える向き）を、空白以外の最初と最後の文字から決める。
    let ink = chars
        .iter()
        .filter(|info| !info.text.trim().is_empty())
        .collect::<Vec<_>>();
    let center = |bounds: &ParsedBounds| {
        if horizontal {
            (bounds.x0 + bounds.x1) / 2.0
        } else {
            (bounds.y0 + bounds.y1) / 2.0
        }
    };
    let forward = match (ink.first(), ink.last()) {
        (Some(first), Some(last)) => center(&last.bounds) >= center(&first.bounds),
        _ => true,
    };
    let mut pieces: Vec<SegmentPiece> = Vec::new();
    let mut previous: Option<ParsedBounds> = None;
    for info in chars {
        let blank = info.text.trim().is_empty();
        // 字送りの向きに進んだ空きだけを数える（重ね打ちや戻りは分けない）。
        let gap = previous.map(|previous| match (horizontal, forward) {
            (true, true) => info.bounds.x0 - previous.x1,
            (true, false) => previous.x0 - info.bounds.x1,
            (false, true) => info.bounds.y0 - previous.y1,
            (false, false) => previous.y0 - info.bounds.y1,
        });
        let wide = gap
            .is_some_and(|gap| info.font_size > 0.0 && gap > SEGMENT_SPLIT_GAP_EM * info.font_size);
        if !blank && (pieces.is_empty() || wide) {
            pieces.push(SegmentPiece {
                bounds: info.bounds,
                text: info.text.clone(),
                font_size: info.font_size,
                font_name: info.font_name.clone(),
                fill_color: info.fill_color.clone(),
                baseline_y: info.baseline_y,
            });
        } else if let Some(piece) = pieces.last_mut() {
            piece.text.push_str(&info.text);
            if !blank {
                piece.bounds = ParsedBounds {
                    x0: piece.bounds.x0.min(info.bounds.x0),
                    y0: piece.bounds.y0.min(info.bounds.y0),
                    x1: piece.bounds.x1.max(info.bounds.x1),
                    y1: piece.bounds.y1.max(info.bounds.y1),
                };
            }
        }
        if !blank {
            previous = Some(info.bounds);
        }
    }
    pieces
}

/// 字ごとに取り出した縦書きの文字片を、縦に続く字の並びへまとめ直す。
///
/// 文字 index から本文を回復すると縦書きの字も 1 字ずつの要素になり、横書きとして行を
/// 組むと段をまたいで字が混ざる。同じ font・大きさで、x がそろい、上から下へ字間をほとんど
/// 空けずに続く字だけをまとめる（横書きの字はそのまま残す）。
pub(crate) fn join_vertical_characters(elements: Vec<ParsedTextElement>) -> Vec<ParsedTextElement> {
    let mut out: Vec<ParsedTextElement> = Vec::with_capacity(elements.len());
    for element in elements {
        if let Some(previous) = out.last_mut() {
            let size = previous.font_size.max(1.0);
            let (a, b) = (previous.bounds, element.bounds);
            let vertical = (a.x0 - b.x0).abs() <= 0.3 * size
                && (a.x1 - b.x1).abs() <= 0.3 * size
                && b.y1 <= a.y0 + 0.3 * size
                && a.y0 - b.y1 <= 0.6 * size
                && a.x1 - a.x0 <= 1.3 * size;
            if vertical
                && previous.page == element.page
                && previous.font_name == element.font_name
                && (previous.font_size - element.font_size).abs() <= 0.1
                && !element.text.trim().is_empty()
                && !previous.text.trim().is_empty()
            {
                previous.text.push_str(&element.text);
                previous.bounds = ParsedBounds {
                    x0: a.x0.min(b.x0),
                    y0: a.y0.min(b.y0),
                    x1: a.x1.max(b.x1),
                    y1: a.y1.max(b.y1),
                };
                previous.baseline_y = None;
                continue;
            }
        }
        out.push(element);
    }
    out
}

/// 全ページSDK本文と非空白文字の順序・件数を照合し、矩形の重複読取りを検出する。
/// 字面の一致だけで反復を消さず、停止済み予算からも再開しない。
pub(crate) fn matches_preview(
    budget: &DecodeBudget,
    elements: &[ParsedTextElement],
    preview: &str,
) -> Result<bool, ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let mut expected = preview.chars().filter(|ch| !ch.is_whitespace());
    for element in elements {
        for ch in element.text.chars() {
            budget.check()?;
            if !ch.is_whitespace() && expected.next() != Some(ch) {
                return Ok(false);
            }
        }
    }
    Ok(expected.next().is_none())
}

/// SDKが実描画glyphを制御文字へ誤対応したページでは、文字index経路の空白除去を採用しない。
/// 生成された改行と、別途確認した印刷hyphenだけを制御文字の例外とする。
pub(crate) fn index_preserves_controls(
    budget: &DecodeBudget,
    path: &std::path::Path,
    characters: &PdfPageTextChars<'_>,
) -> Result<bool, ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    for character in characters.iter() {
        budget.check()?;
        let value = character.unicode_value();
        if value > 31 && !(127..=159).contains(&value) {
            continue;
        }
        if crate::pdfium_printed_hyphens::is_printed(budget, path, &character)? {
            continue;
        }
        if !character
            .is_generated()
            .map_err(|error| ParseError::Pdfium {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// segment.charsと同じ端点・許容幅で検索し、範囲の先頭だけを無確保で取得する。
/// 両端が見つかる場合は依存先のinclusive範囲（end+1）と同じ空判定を行う。
fn first_in_rect<'a>(
    characters: &'a PdfPageTextChars<'_>,
    rect: PdfRect,
) -> Option<PdfPageTextChar<'a>> {
    let tolerance_x = rect.width() / 2.0;
    let tolerance_y = rect.height() / 2.0;
    let center = rect.bottom() + tolerance_y;
    let start = characters.get_char_near_point(rect.left(), tolerance_x, center, tolerance_y);
    let end = characters.get_char_near_point(rect.right(), tolerance_x, center, tolerance_y);
    match (start, end) {
        (Some(start), Some(end)) if start.index() <= end.index().saturating_add(1) => Some(start),
        (Some(_), Some(_)) => None,
        (Some(start), None) => Some(start),
        (None, Some(end)) => Some(end),
        (None, None) => None,
    }
}

/// 文字行列の縦方向の長さで font size を拡大縮小する。
///
/// Pdfium の `scaled_font_size` は行列の `d` 成分だけを掛けるため、90 度回転した文字では 0 になる。
/// 回転に依らない縦ベクトル `(c, d)` の長さを使い、取得できない場合は未拡大の値を返す。
pub(crate) fn effective_font_size(character: &PdfPageTextChar<'_>) -> f32 {
    let unscaled = character.unscaled_font_size().value;
    let scale = character
        .matrix()
        .ok()
        .map(|matrix| matrix.c().hypot(matrix.d()))
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(1.0);
    let size = unscaled * scale;
    // 文字行列に入らない座標変換（cm）で拡大した字は、字の矩形より極端に小さい大きさになる。
    // その場合は字の矩形の大きさ（縦横の長い方）を使う。
    let extent = character
        .loose_bounds()
        .ok()
        .map(|bounds| {
            (bounds.top().value - bounds.bottom().value)
                .abs()
                .max((bounds.right().value - bounds.left().value).abs())
        })
        .filter(|extent| extent.is_finite());
    match extent {
        Some(extent) if size < 0.5 * extent => extent,
        _ => size,
    }
}

/// 本文・属性の保存コピーを予約し、容量不足を公開エラーへ伝播する。
fn copy(budget: &DecodeBudget, source: Option<&str>) -> Result<Option<String>, ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let result = crate::text_result_storage::copy(budget, source);
    budget.check()?;
    result.ok_or_else(|| ParseError::Io(std::io::Error::other("native text storage stopped")))
}

/// Pdfiumの矩形をparser IRのboundsへ変換する。
fn parsed_bounds_from_pdf_rect(rect: PdfRect) -> ParsedBounds {
    ParsedBounds {
        x0: rect.left().value,
        y0: rect.bottom().value,
        x1: rect.right().value,
        y1: rect.top().value,
    }
}

/// 回転のない水平glyphでSDKが返す有限の原点だけを、字面矩形と区別して保持する。
pub(crate) fn horizontal_baseline(character: &PdfPageTextChar<'_>) -> Option<f32> {
    let angle = character.angle_degrees().ok()?;
    if !angle.is_finite() || angle.rem_euclid(360.0) != 0.0 {
        return None;
    }
    let y = character.origin_y().ok()?.value;
    y.is_finite().then_some(y)
}

#[cfg(test)]
mod split_tests {
    use super::*;

    /// 1 文字分の情報を作る。
    fn info(text: &str, x0: f32, x1: f32) -> CharInfo {
        CharInfo {
            text: text.to_string(),
            bounds: ParsedBounds {
                x0,
                y0: 0.0,
                x1,
                y1: 8.0,
            },
            font_size: 8.0,
            font_name: Some("F".into()),
            fill_color: None,
            baseline_y: None,
        }
    }

    /// 隣の表の列ほど離れた文字は別の要素に分け、語間の空白では分けない。
    #[test]
    fn splits_only_at_gaps_wider_than_one_em() {
        let chars = [
            info("2", 405.0, 409.0),
            info("4", 409.0, 413.0),
            info("0", 413.0, 417.0),
            info(" ", 417.0, 417.0),
            info("４", 567.0, 575.0),
            info(" ", 575.0, 575.0),
            info("級", 578.0, 586.0),
        ];
        let pieces = group_chars_at_wide_gaps(&chars, true);
        let texts = pieces
            .iter()
            .map(|piece| piece.text.trim())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["240", "４ 級"]);
        assert_eq!(pieces[0].bounds.x1, 417.0);
        assert_eq!(pieces[1].bounds.x0, 567.0);
    }
}
