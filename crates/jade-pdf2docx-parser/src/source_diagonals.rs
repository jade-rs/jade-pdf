//! 軸平行罫線と混ぜず、実際にstrokeされた黒い対角線の端点を保存する。
use super::{
    ParsedPageViewport,
    stroked_rectangles::{drawing_operand, joined, paint::Paint, resources},
};
use crate::{
    ParsedSourceDiagonal,
    decode::DecodeBudget,
    metadata_workspace::try_push,
    resource_resolver::ResourceResolver,
    syntax::{Lexer, Token},
    text_draws::Operand,
};

/// q/Qで戻す座標変換とstrokeの根拠。
#[derive(Clone, Copy)]
struct State {
    /// 現在の座標変換。
    matrix: [f32; 6],
    /// 現在の線幅。
    width: f32,
    /// 単純線分の端を定義できるPDF line cap。未知値はNone。
    cap: Option<u8>,
    /// 後続の黒塗りが元の黒strokeを消さないことの、strokeと独立な根拠。
    fill: Paint,
    /// 不透明な黒い実線の根拠。
    paint: Paint,
}

/// 同一ページの連続Contentsでpath・q保存・描画順を維持する。
fn scan<'a>(
    budget: &DecodeBudget,
    page: usize,
    content: &[u8],
    page_resources: Option<&'a [u8]>,
    resolver: &mut ResourceResolver<'_, 'a>,
    output: &mut Vec<ParsedSourceDiagonal>,
) -> Option<()> {
    let clips = crate::content_clip::scan(budget, content, [1., 0., 0., 1., 0., 0.]);
    if budget.is_stopped() {
        return None;
    }
    if clips.unsupported {
        return Some(());
    }
    let Some(media) = budget.page_media_box(page) else {
        return Some(());
    };
    let page_clip =
        crate::form_clip::Clip::page(budget, Some(media), budget.page_crop_box(page).flatten())?;
    let mut state = State {
        matrix: [1., 0., 0., 1., 0., 0.],
        width: 1.,
        cap: Some(0),
        fill: Default::default(),
        paint: Default::default(),
    };
    let mut saved = Vec::new();
    let mut segments = Vec::new();
    let mut current = None;
    let mut first = None;
    let mut curved = false;
    let mut has_segment = false;
    let mut bounds = None;
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
            let Some(value) = drawing_operand(&mut lexer, token, span.start) else {
                break;
            };
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
        let fill_word: Option<&[u8]> = match word {
            b"g" => Some(b"G"),
            b"rg" => Some(b"RG"),
            b"k" => Some(b"K"),
            b"cs" => Some(b"CS"),
            b"sc" => Some(b"SC"),
            b"scn" => Some(b"SCN"),
            _ => None,
        };
        if let Some(fill_word) = fill_word {
            state.fill.apply(fill_word, &args[..count]);
        }

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
            ) => state.matrix = super::concat_matrix(state.matrix, [*a, *b, *c, *d, *e, *f]),
            (b"cm", _) => state.matrix = [f32::NAN; 6],
            (b"J", [Operand::Number(v)]) => {
                state.cap = match *v {
                    0. => Some(0),
                    1. => Some(1),
                    2. => Some(2),
                    _ => None,
                }
            }
            (b"J", _) => state.cap = None,
            (b"w", [Operand::Number(w)]) => state.width = *w,
            (b"w", _) => state.width = f32::NAN,
            (b"gs", [Operand::Name(name)]) => {
                // stroke用CAの解決をfill用caの根拠へ使わない。Qまでは未確認を保つ。
                state.fill.state(None);
                state
                    .paint
                    .state(resources::resolve(budget, resolver, page_resources, name))
            }
            (b"gs", _) => {
                state.paint.state(None);
                state.fill.state(None);
            }
            (b"m" | b"l", [Operand::Number(x), Operand::Number(y)]) => {
                let next = point(state.matrix, *x, *y);
                if let Some(p) = next {
                    grow(&mut bounds, p);
                    if word == b"l" {
                        // 複数連結線のjoin/miter範囲を単純線分のclip根拠へ縮めない。
                        if has_segment {
                            curved = true;
                        }
                        has_segment = true;
                        if let Some(old) = current {
                            try_push(budget, &mut segments, (old, p))?;
                        }
                    } else {
                        has_segment = false;
                        first = Some(p);
                    }
                } else {
                    curved = true;
                }
                current = next;
            }
            (b"h", []) => {
                curved = true;
                if let (Some(a), Some(b)) = (current, first) {
                    try_push(budget, &mut segments, (a, b))?;
                    current = first;
                }
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
                for (x, y) in [(*x, *y), (*x + *w, *y), (*x, *y + *h), (*x + *w, *y + *h)] {
                    if let Some(p) = point(state.matrix, x, y) {
                        grow(&mut bounds, p);
                    } else {
                        curved = true;
                    }
                }
                current = None;
                first = None;
            }
            (b"m" | b"l" | b"re" | b"h", _) => {
                curved = true;
                current = None;
            }
            (b"c" | b"v" | b"y", _) => {
                curved = true;
                current = None;
            }
            (b"S", []) => {
                if !curved
                    && state.paint.supported()
                    && resources::native_device(resolver, page_resources, state.paint.components())
                {
                    if let Some(w) = width(state) {
                        for segment in segments.drain(..) {
                            emit(
                                budget,
                                output,
                                page,
                                segment,
                                w,
                                clips.get(span.start),
                                &page_clip,
                            )?;
                        }
                    }
                }
                segments.clear();
                current = None;
                first = None;
                curved = false;
                has_segment = false;
                bounds = None;
            }
            (b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*", []) => {
                if !(state.fill.supported()
                    && resources::native_device(resolver, page_resources, state.fill.components()))
                {
                    occlude(output, page, if curved { None } else { bounds });
                }
                segments.clear();
                current = None;
                first = None;
                curved = false;
                has_segment = false;
                bounds = None;
            }
            (b"n" | b"s", []) => {
                segments.clear();
                current = None;
                first = None;
                curved = false;
                has_segment = false;
                bounds = None;
            }
            (b"Do" | b"sh", _) => occlude(output, page, None),
            // Optional Contentの可視性は未確認なので、このページの対角線を推測しない。
            (b"BDC", [Operand::Name(name), _])
                if crate::metadata_names::resource_name_order(name, "OC").is_eq() =>
            {
                occlude(output, page, None);
                break;
            }
            (b"BI", []) => {
                occlude(output, page, None);
                break;
            }
            _ => {}
        }
        count = 0;
    }
    Some(())
}

/// 同じ展開済みstreamと共有予算から、別配列の対角線を抽出する。
pub(super) fn append(
    budget: &DecodeBudget,
    source: &[u8],
    streams: &[(usize, Vec<u8>)],
    output: &mut Vec<ParsedSourceDiagonal>,
) -> Option<()> {
    let mut index = 0;
    let mut first = None;
    while index < streams.len() {
        let (page, content, end) = joined(budget, streams, index)?;
        index = end;
        if probe::has_candidate(budget, &content) {
            first = Some((page, content));
            break;
        }
    }
    let Some((page, content)) = first else {
        return Some(());
    };
    let objects = crate::object_index::build(budget, source)?;
    let mut resolver = ResourceResolver::new(&objects, budget);
    let page_resources = match budget.page_resources(source) {
        Some(p) => p,
        None => {
            let dictionaries = budget.page_dictionaries(source);
            let mut pages = budget.metadata_vector(dictionaries.len())?;
            for p in dictionaries {
                pages.push(resources::inherited(budget, &mut resolver, p)?);
            }
            pages
        }
    };
    scan(
        budget,
        page,
        &content,
        page_resources.get(page).copied().flatten(),
        &mut resolver,
        output,
    )?;
    while index < streams.len() {
        let (page, content, end) = joined(budget, streams, index)?;
        index = end;
        if probe::has_candidate(budget, &content) {
            scan(
                budget,
                page,
                &content,
                page_resources.get(page).copied().flatten(),
                &mut resolver,
                output,
            )?;
        }
    }
    Some(())
}

#[path = "source_diagonal_geometry.rs"]
mod geometry;
pub(super) use geometry::in_viewports;
use geometry::{emit, grow, occlude, point, width};

#[path = "source_diagonal_probe.rs"]
mod probe;
