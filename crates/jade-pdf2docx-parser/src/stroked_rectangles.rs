//! 元のre pathの辺を、構築時の座標と描画時のstroke/clipに基づいて保存する。
use crate::{
    ParsedBounds, ParsedGraphicElement,
    decode::DecodeBudget,
    metadata_workspace::try_push,
    parser,
    resource_resolver::ResourceResolver,
    syntax::{Lexer, Token},
    text_draws::{Operand, operand},
};
use std::borrow::Cow;
#[path = "rectangle_paint.rs"]
mod paint;
#[path = "rectangle_resources.rs"]
mod resources;

/// q/Qで復元する長方形の描画状態。path自体は退避しない。
#[derive(Clone, Copy, Default)]
struct State {
    /// 現在のCTM。
    matrix: [f32; 6],
    /// ユーザー空間の線幅。
    width: f32,
    /// 確認済みのstroke色と合成状態。
    paint: paint::Paint,
}

/// byte列内の真のre命令だけを検出し、Resourceを使わないページの追加解決を避ける。
fn has_rectangle(content: &[u8]) -> bool {
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    while let Some((token, span)) = lexer.next_spanned() {
        if matches!(token, Token::Word(b"re")) {
            return true;
        }
        if !matches!(token, Token::Word(_)) {
            operand(&mut lexer, token, span.start);
        }
    }
    false
}

/// 任意の軸平行変換・反転・90度回転でも、外接矩形ではなく元の四辺を使う。
fn rectangle(matrix: [f32; 6], values: [f32; 4]) -> Option<ParsedBounds> {
    if !matrix.iter().chain(&values).all(|v| v.is_finite())
        || !((matrix[1] == 0. && matrix[2] == 0. && matrix[0] != 0. && matrix[3] != 0.)
            || (matrix[0] == 0. && matrix[3] == 0. && matrix[1] != 0. && matrix[2] != 0.))
    {
        return None;
    }
    let b = parser::transform_rect_bounds(matrix, values[0], values[1], values[2], values[3]);
    ([b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite()) && b.x0 < b.x1 && b.y0 < b.y1)
        .then_some(b)
}

/// 元の辺とstrokeの外周が両方のclip内にあるときだけ保存する。
#[allow(clippy::too_many_arguments)]
fn emit(
    budget: &DecodeBudget,
    output: &mut Vec<ParsedGraphicElement>,
    page: usize,
    b: ParsedBounds,
    state: State,
    clip: Option<&crate::form_clip::Clip>,
    page_clip: &crate::form_clip::Clip,
) -> Option<()> {
    let horizontal = state.width * state.matrix[1].hypot(state.matrix[3]);
    let vertical = state.width * state.matrix[0].hypot(state.matrix[2]);
    if ![horizontal, vertical]
        .iter()
        .all(|v| v.is_finite() && *v > 0.)
    {
        return Some(());
    }
    for (bounds, width) in [
        (
            ParsedBounds {
                x0: b.x0,
                y0: b.y0,
                x1: b.x1,
                y1: b.y0,
            },
            horizontal,
        ),
        (
            ParsedBounds {
                x0: b.x0,
                y0: b.y1,
                x1: b.x1,
                y1: b.y1,
            },
            horizontal,
        ),
        (
            ParsedBounds {
                x0: b.x0,
                y0: b.y0,
                x1: b.x0,
                y1: b.y1,
            },
            vertical,
        ),
        (
            ParsedBounds {
                x0: b.x1,
                y0: b.y0,
                x1: b.x1,
                y1: b.y1,
            },
            vertical,
        ),
    ] {
        let padded = if bounds.y0 == bounds.y1 {
            [
                bounds.x0 - vertical / 2.,
                bounds.y0 - horizontal / 2.,
                bounds.x1 + vertical / 2.,
                bounds.y1 + horizontal / 2.,
            ]
        } else {
            [
                bounds.x0 - vertical / 2.,
                bounds.y0 - horizontal / 2.,
                bounds.x1 + vertical / 2.,
                bounds.y1 + horizontal / 2.,
            ]
        };
        if page_clip.contains_rect(padded, [1., 0., 0., 1., 0., 0.])
            && clip.is_none_or(|clip| clip.contains_rect(padded, [1., 0., 0., 1., 0., 0.]))
        {
            try_push(
                budget,
                output,
                ParsedGraphicElement {
                    page,
                    bounds,
                    stroke_width: width,
                },
            )?;
        }
    }
    Some(())
}

/// 一ページの連続本文を走査し、gsの所属ページとWの描画確定位置を照合する。
fn scan<'a>(
    budget: &DecodeBudget,
    page: usize,
    content: &[u8],
    page_resources: Option<&'a [u8]>,
    resolver: &mut ResourceResolver<'_, 'a>,
    output: &mut Vec<ParsedGraphicElement>,
) -> Option<()> {
    let clips = crate::content_clip::scan(budget, content, [1., 0., 0., 1., 0., 0.]);
    if budget.is_stopped() {
        return None;
    }
    if clips.unsupported {
        return Some(());
    }
    let media = budget.page_media_box(page)?;
    let page_clip =
        crate::form_clip::Clip::page(budget, Some(media), budget.page_crop_box(page).flatten())?;
    let mut state = State {
        matrix: [1., 0., 0., 1., 0., 0.],
        width: 1.,
        paint: Default::default(),
    };
    let mut saved = Vec::new();
    let mut path = Vec::new();
    let mut args = [Operand::Other; 6];
    let mut count = 0;
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    while let Some((token, span)) = lexer.next_spanned() {
        if budget.is_stopped() {
            return None;
        }
        let Token::Word(word) = token else {
            let value = operand(&mut lexer, token, span.start);
            if count < args.len() {
                args[count] = value;
                count += 1;
            } else {
                count = 0;
            }
            continue;
        };
        if let Some(value) = crate::page_attributes::number_value(word) {
            if count < args.len() {
                args[count] = Operand::Number(value);
                count += 1;
            } else {
                count = 0;
            }
            continue;
        }
        state.paint.apply(word, &args[..count]);
        match (word, &args[..count]) {
            (b"q", []) => {
                budget.check_form_graphics_depth(saved.len() + 1)?;
                try_push(budget, &mut saved, state)?;
            }
            (b"Q", []) => {
                if let Some(previous) = saved.pop() {
                    state = previous;
                }
            }
            (
                b"cm",
                [
                    Operand::Number(a),
                    Operand::Number(b),
                    Operand::Number(c),
                    Operand::Number(d),
                    Operand::Number(e),
                    Operand::Number(f),
                ],
            ) => state.matrix = parser::concat_matrix(state.matrix, [*a, *b, *c, *d, *e, *f]),
            (b"w", [Operand::Number(width)]) => state.width = *width,
            (b"gs", [Operand::Name(name)]) => {
                state
                    .paint
                    .state(resources::resolve(budget, resolver, page_resources, name))
            }
            (
                b"re",
                [
                    Operand::Number(x),
                    Operand::Number(y),
                    Operand::Number(w),
                    Operand::Number(h),
                ],
            ) => {
                if let Some(bounds) = rectangle(state.matrix, [*x, *y, *w, *h]) {
                    try_push(budget, &mut path, bounds)?;
                }
            }
            (b"S" | b"s" | b"B" | b"B*" | b"b" | b"b*", []) => {
                if state.paint.supported() {
                    for bounds in path.drain(..) {
                        emit(
                            budget,
                            output,
                            page,
                            bounds,
                            state,
                            clips.get(span.start),
                            &page_clip,
                        )?;
                    }
                }
                path.clear();
            }
            (b"n" | b"f" | b"F" | b"f*", []) => path.clear(),
            (b"BI", []) => break,
            (b"gs", _) => state.paint.state(None),
            _ => {}
        }
        count = 0;
    }
    Some(())
}

/// Contents配列を一つの本文として扱い、各ページのre描画だけを既存結果へ追加する。
pub(super) fn append(
    budget: &DecodeBudget,
    source: &[u8],
    streams: &[(usize, Vec<u8>)],
    output: &mut Vec<ParsedGraphicElement>,
) -> Option<()> {
    let mut start = 0;
    while start < streams.len() {
        let page = streams[start].0;
        let end = start
            + streams[start..]
                .iter()
                .take_while(|(p, _)| *p == page)
                .count();
        let group = &streams[start..end];
        let content = if group.len() == 1 {
            Cow::Borrowed(group[0].1.as_slice())
        } else {
            let total = group
                .iter()
                .try_fold(0_usize, |n, (_, s)| n.checked_add(s.len()))?;
            let mut bytes = budget.metadata_vector(total)?;
            for (_, chunk) in group {
                bytes.extend_from_slice(chunk);
            }
            Cow::Owned(bytes)
        };
        if has_rectangle(&content) {
            let objects = crate::object_index::build(budget, source)?;
            let mut resolver = ResourceResolver::new(&objects, budget);
            let page_resources = budget
                .page_resources(source)
                .and_then(|pages| pages.get(page).copied().flatten());
            scan(
                budget,
                page,
                &content,
                page_resources,
                &mut resolver,
                output,
            )?;
        }
        start = end;
    }
    Some(())
}
