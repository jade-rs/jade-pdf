//! 文字描画より前の矩形塗りを、別色の遮蔽と未知の描画から区別する。
use super::{DecodeBudget, ParsedBounds, stroked_rectangles};
use crate::{
    ParsedTextBackground,
    metadata_workspace::try_push,
    parser,
    syntax::{Lexer, Token},
    text_draws::Operand,
};

#[path = "text_background_clip.rs"]
mod clipping;

/// q/Qで復元する矩形の色と座標系。
#[derive(Clone, Copy)]
struct State {
    /// 現在のCTM。
    matrix: [f32; 6],
    /// 確認できたDevice色。不明な色空間ではNone。
    color: Option<[u8; 3]>,
    /// q/Qで復元する、確認済みの矩形clip。
    clip: clipping::Clip,
}

/// 一つの塗りの原本範囲と描画順。
struct Fill {
    /// 軸平行な原本矩形。
    bounds: ParsedBounds,
    /// 未確認色は別の塗りを遮蔽する候補としてだけ残す。
    color: Option<[u8; 3]>,
    /// 塗り命令直後のbyte位置。
    end: usize,
}

/// 追加確保なしで、非白・非黒のDevice色と矩形が存在するページだけを選ぶ。
fn has_candidate(content: &[u8]) -> bool {
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    let mut values = [0.; 4];
    let mut count = 0;
    let mut colored = false;
    let mut rect = false;
    while let Some((token, span)) = lexer.next_spanned() {
        let Token::Word(word) = token else {
            if stroked_rectangles::drawing_operand(&mut lexer, token, span.start).is_none() {
                return false;
            }
            count = 0;
            continue;
        };
        if let Some(n) = crate::page_attributes::number_value(word) {
            if count < 4 {
                values[count] = n;
                count += 1;
            } else {
                count = 0;
            }
            continue;
        }
        if let Some(rgb) = device_color(word, &values[..count]) {
            colored |= rgb != [0; 3] && rgb != [255; 3];
        }
        rect |= word == b"re";
        count = 0;
    }
    colored && rect
}

/// 有限で正しい成分数のDevice色だけを既存のRGB変換へ渡す。
fn device_color(word: &[u8], values: &[f32]) -> Option<[u8; 3]> {
    if !values.iter().all(|v| v.is_finite()) {
        return None;
    }
    match (word, values) {
        (b"g", [g]) => Some([parser::pdf_color_component_to_u8(*g); 3]),
        (b"rg", [r, g, b]) => Some([*r, *g, *b].map(parser::pdf_color_component_to_u8)),
        (b"k", [c, m, y, k]) => Some(parser::pdf_cmyk_to_rgb(*c, *m, *y, *k)),
        _ => None,
    }
}

/// 非矩形の塗りが細い既存罫線の場合だけ、セル背景の遮蔽候補から除く。
fn thin(bounds: ParsedBounds) -> bool {
    (bounds.x1 - bounds.x0).abs() <= 2. || (bounds.y1 - bounds.y0).abs() <= 2.
}

/// page本文を一度走査して描画順を保存する。未知のclip・画像・広い非矩形塗りは推測しない。
pub(super) fn scan(budget: &DecodeBudget, content: &[u8]) -> Option<Backgrounds> {
    if !has_candidate(content) {
        return None;
    }
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    let mut args = [Operand::Other; 6];
    let mut count = 0;
    let mut state = State {
        matrix: [1., 0., 0., 1., 0., 0.],
        color: Some([0; 3]),
        clip: clipping::Clip::Unbounded,
    };
    let mut saved = Vec::new();
    let mut fills = Vec::new();
    let mut rectangle = None;
    let mut path_bounds: Option<ParsedBounds> = None;
    let mut nonrect = false;
    let mut curved = false;
    let mut pending_clip = None;
    while let Some((token, span)) = lexer.next_spanned() {
        if budget.is_stopped() {
            return None;
        }
        let Token::Word(word) = token else {
            let operand = stroked_rectangles::drawing_operand(&mut lexer, token, span.start)?;
            if count == 6 {
                return None;
            }
            args[count] = operand;
            count += 1;
            continue;
        };
        if let Some(n) = crate::page_attributes::number_value(word) {
            if count == 6 || !n.is_finite() {
                return None;
            }
            args[count] = Operand::Number(n);
            count += 1;
            continue;
        }
        let mut numbers = [0.; 6];
        let numeric = args[..count].iter().zip(&mut numbers).all(|(a, b)| {
            if let Operand::Number(n) = a {
                *b = *n;
                true
            } else {
                false
            }
        });
        let values = &numbers[..count];
        match word {
            b"g" | b"rg" | b"k" => {
                state.color = device_color(word, values).filter(|_| numeric);
                if state.color.is_none() {
                    return None;
                }
            }
            b"cs" | b"sc" | b"scn" => state.color = None,
            b"q" if count == 0 => try_push(budget, &mut saved, state)?,
            b"Q" if count == 0 => state = saved.pop()?,
            b"q" | b"Q" => return None,
            b"cm" if numeric && count == 6 => {
                state.matrix = parser::concat_matrix(state.matrix, numbers)
            }
            b"cm" => return None,
            b"re" if numeric && count == 4 => {
                if rectangle.is_some() || nonrect {
                    return None;
                }
                rectangle = Some(stroked_rectangles::rectangle(
                    state.matrix,
                    [numbers[0], numbers[1], numbers[2], numbers[3]],
                )?);
            }
            b"re" => return None,
            b"m" | b"l" if numeric && count == 2 => {
                nonrect = true;
                let (x, y) = parser::transform_point(state.matrix, numbers[0], numbers[1]);
                if !x.is_finite() || !y.is_finite() {
                    return None;
                }
                path_bounds = Some(path_bounds.map_or(
                    ParsedBounds {
                        x0: x,
                        y0: y,
                        x1: x,
                        y1: y,
                    },
                    |b| ParsedBounds {
                        x0: b.x0.min(x),
                        y0: b.y0.min(y),
                        x1: b.x1.max(x),
                        y1: b.y1.max(y),
                    },
                ));
            }
            b"m" | b"l" => return None,
            b"c" | b"v" | b"y" => curved = true,
            b"W" | b"W*" => {
                if count != 0 || nonrect || curved {
                    return None;
                }
                pending_clip = Some(rectangle?);
            }
            b"Do" | b"BI" | b"sh" => return None,
            b"Tr" if !numeric || count != 1 || numbers[0] >= 4. => return None,
            b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" => {
                if count != 0 || curved {
                    return None;
                }
                if nonrect {
                    if !path_bounds.is_some_and(thin) {
                        return None;
                    }
                } else if let Some(bounds) = rectangle.and_then(|bounds| state.clip.apply(bounds)) {
                    if !thin(bounds) {
                        if fills.len() == 256 {
                            return None;
                        }
                        try_push(
                            budget,
                            &mut fills,
                            Fill {
                                bounds,
                                color: state.color,
                                end: lexer.cursor,
                            },
                        )?;
                    }
                }
                // Wの新clipは現在のpathを描画した後に適用する。
                if let Some(next) = pending_clip.take() {
                    state.clip = state.clip.intersect(next);
                }
                rectangle = None;
                path_bounds = None;
                nonrect = false;
                curved = false;
            }
            b"n" | b"S" | b"s" => {
                if let Some(next) = pending_clip.take() {
                    state.clip = state.clip.intersect(next);
                }
                rectangle = None;
                path_bounds = None;
                nonrect = false;
                curved = false;
            }
            _ => {}
        }
        count = 0;
    }
    (count == 0 && saved.is_empty() && pending_clip.is_none()).then_some(Backgrounds { fills })
}

/// 確認した塗りだけを描画順で借用する。
pub(super) struct Backgrounds {
    /// 原本の描画順で並ぶ矩形。
    fills: Vec<Fill>,
}

/// 二つの矩形が面積を持って重なるかを判定する。
fn overlaps(a: ParsedBounds, b: ParsedBounds) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}

/// 原本矩形が文字矩形を完全に含むかを判定する。
fn contains(a: ParsedBounds, b: ParsedBounds) -> bool {
    a.x0 <= b.x0 && a.y0 <= b.y0 && a.x1 >= b.x1 && a.y1 >= b.y1
}

impl Backgrounds {
    /// 文字より先に塗られ、別色や後続塗りに遮蔽されない最大の色矩形だけを返す。
    pub(super) fn behind(
        &self,
        position: usize,
        bounds: ParsedBounds,
    ) -> Option<ParsedTextBackground> {
        let nearest = self
            .fills
            .iter()
            .rev()
            .find(|fill| fill.end < position && contains(fill.bounds, bounds))?;
        let rgb = nearest.color.filter(|c| *c != [0; 3] && *c != [255; 3])?;
        let candidate = self
            .fills
            .iter()
            .filter(|fill| {
                fill.end < position && fill.color == Some(rgb) && contains(fill.bounds, bounds)
            })
            .max_by(|a, b| {
                ((a.bounds.x1 - a.bounds.x0) * (a.bounds.y1 - a.bounds.y0))
                    .total_cmp(&((b.bounds.x1 - b.bounds.x0) * (b.bounds.y1 - b.bounds.y0)))
            })?;
        if self.fills.iter().any(|other| {
            other.end > candidate.end
                && ((other.color != Some(rgb) && overlaps(other.bounds, candidate.bounds))
                    || (other.end > position && overlaps(other.bounds, bounds)))
        }) {
            return None;
        }
        Some(ParsedTextBackground {
            bounds: candidate.bounds,
            rgb,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParseLimits;

    /// 塗り配列の最小容量を確保する前に予算を予約し、境界の不足を公開エラーへ残す。
    #[test]
    fn background_result_capacity_is_reserved_before_allocation() {
        let total = (4 * size_of::<Fill>()) as u64;
        for limit in [total - 1, total, total + 1] {
            let budget = DecodeBudget::new(&ParseLimits {
                max_total_metadata_workspace_bytes: limit,
                ..Default::default()
            });
            let result = scan(&budget, b".2 .4 .6 rg 0 0 20 20 re f");
            assert_eq!(result.is_some(), limit >= total);
            assert_eq!(budget.is_stopped(), limit < total);
            if let Some(result) = result {
                assert_eq!(
                    result
                        .behind(
                            100,
                            ParsedBounds {
                                x0: 1.,
                                y0: 1.,
                                x1: 2.,
                                y1: 2.
                            }
                        )
                        .unwrap()
                        .rgb,
                    [51, 102, 153]
                );
                assert!(budget.check().is_ok());
            } else {
                assert!(budget.check().is_err());
            }
        }
    }

    /// 通常の黒文字やcontainer内の偽色・矩形では追加配列を予約しない。
    #[test]
    fn no_real_color_candidate_keeps_zero_background_workspace() {
        for content in [
            b"0 g BT (text) Tj ET".as_slice(),
            b"BT (.2 .4 .6 rg 0 0 20 20 re f) Tj ET",
            b"[.2 .4 .6 rg 0 0 20 20 re f]",
        ] {
            let budget = DecodeBudget::new(&ParseLimits {
                max_total_metadata_workspace_bytes: 0,
                ..Default::default()
            });
            assert!(scan(&budget, content).is_none());
            assert!(!budget.is_stopped());
            assert!(budget.check().is_ok());
        }
    }
}
