//! 軸平行罫線と混ぜず、実際にstrokeされた黒い対角線の端点を保存する。
use super::{
    ParsedPageViewport,
    stroked_rectangles::{drawing_operand, joined, paint::Paint, resources},
};
use crate::{
    ParsedBounds, ParsedSourceDiagonal,
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
    /// 不透明な黒い実線の根拠。
    paint: Paint,
}

/// 一様な変換だけで、方向に依存しない実効線幅を求める。
fn width(state: State) -> Option<f32> {
    let [a, b, c, d, _, _] = state.matrix.map(f64::from);
    let x = a * a + b * b;
    let y = c * c + d * d;
    let w = f64::from(state.width) * x.sqrt();
    (state.matrix.iter().all(|v| v.is_finite())
        && x > 0.
        && x == y
        && a * c + b * d == 0.
        && w.is_finite()
        && w > 0.
        && w <= f64::from(f32::MAX))
    .then_some(w as f32)
}

/// 点の方向を保ったままf64で変換し、非有限・表現不能の結果を使わない。
fn point(matrix: [f32; 6], x: f32, y: f32) -> Option<[f32; 2]> {
    let p = crate::content_clip::transform([f64::from(x), f64::from(y)], matrix);
    p.iter()
        .all(|v| v.is_finite() && v.abs() <= f64::from(f32::MAX))
        .then(|| p.map(|v| v as f32))
}

/// 複数subpathの後続fillでも、隠し得る全領域の外接矩形を追跡する。
fn grow(bounds: &mut Option<ParsedBounds>, p: [f32; 2]) {
    if let Some(b) = bounds {
        b.x0 = b.x0.min(p[0]);
        b.x1 = b.x1.max(p[0]);
        b.y0 = b.y0.min(p[1]);
        b.y1 = b.y1.max(p[1]);
    } else {
        *bounds = Some(ParsedBounds {
            x0: p[0],
            y0: p[1],
            x1: p[0],
            y1: p[1],
        });
    }
}

/// 後続の塗りが重なる候補を除き、画像や未知の領域では既存候補を保守的に捨てる。
fn occlude(output: &mut Vec<ParsedSourceDiagonal>, page: usize, bounds: Option<ParsedBounds>) {
    output.retain(|line| {
        if line.page != page {
            return true;
        }
        let Some(b) = bounds else {
            return false;
        };
        let pad = line.stroke_width / 2.;
        line.start[0].max(line.end[0]) + pad < b.x0
            || line.start[0].min(line.end[0]) - pad > b.x1
            || line.start[1].max(line.end[1]) + pad < b.y0
            || line.start[1].min(line.end[1]) - pad > b.y1
    });
}

/// 描画前のclipとページ内へstroke全体が収まる、非軸平行の線分だけを保存する。
#[allow(clippy::too_many_arguments)]
fn emit(
    budget: &DecodeBudget,
    output: &mut Vec<ParsedSourceDiagonal>,
    page: usize,
    segment: ([f32; 2], [f32; 2]),
    stroke_width: f32,
    clip: Option<&crate::form_clip::Clip>,
    page_clip: &crate::form_clip::Clip,
) -> Option<()> {
    let (start, end) = segment;
    if start[0] == end[0] || start[1] == end[1] {
        return Some(());
    }
    let pad = stroke_width / 2.;
    let padded = [
        start[0].min(end[0]) - pad,
        start[1].min(end[1]) - pad,
        start[0].max(end[0]) + pad,
        start[1].max(end[1]) + pad,
    ];
    if padded.iter().all(|v| v.is_finite())
        && page_clip.contains_rect(padded, [1., 0., 0., 1., 0., 0.])
        && clip.is_none_or(|c| c.contains_rect(padded, [1., 0., 0., 1., 0., 0.]))
    {
        try_push(
            budget,
            output,
            ParsedSourceDiagonal {
                page,
                start,
                end,
                stroke_width,
            },
        )?;
    }
    Some(())
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
        paint: Default::default(),
    };
    let mut saved = Vec::new();
    let mut segments = Vec::new();
    let mut current = None;
    let mut first = None;
    let mut curved = false;
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
            (b"w", [Operand::Number(w)]) => state.width = *w,
            (b"w", _) => state.width = f32::NAN,
            (b"gs", [Operand::Name(name)]) => {
                state
                    .paint
                    .state(resources::resolve(budget, resolver, page_resources, name))
            }
            (b"gs", _) => state.paint.state(None),
            (b"m" | b"l", [Operand::Number(x), Operand::Number(y)]) => {
                let next = point(state.matrix, *x, *y);
                if let Some(p) = next {
                    grow(&mut bounds, p);
                    if word == b"l" {
                        if let Some(old) = current {
                            try_push(budget, &mut segments, (old, p))?;
                        }
                    } else {
                        first = Some(p);
                    }
                } else {
                    curved = true;
                }
                current = next;
            }
            (b"h", []) => {
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
                bounds = None;
            }
            (b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*", []) => {
                occlude(output, page, if curved { None } else { bounds });
                segments.clear();
                current = None;
                first = None;
                curved = false;
                bounds = None;
            }
            (b"n" | b"s", []) => {
                segments.clear();
                current = None;
                first = None;
                curved = false;
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
    // lのない文書ではResource索引を追加しない。実際の字句・描画判定はscanが行う。
    if !streams.iter().any(|(_, s)| s.contains(&b'l')) {
        return Some(());
    }
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
    let mut index = 0;
    while index < streams.len() {
        let (page, content, end) = joined(budget, streams, index)?;
        index = end;
        scan(
            budget,
            page,
            &content,
            page_resources.get(page).copied().flatten(),
            &mut resolver,
            output,
        )?;
    }
    Some(())
}

/// 原点移動とcardinal page rotationで、外接矩形へ向きを潰さず二端点を変換する。
pub(super) fn in_viewports(
    lines: &mut Vec<ParsedSourceDiagonal>,
    viewports: &[ParsedPageViewport],
) {
    lines.retain_mut(|line| {
        let Some(v) = viewports.iter().find(|v| v.page.page_number == line.page) else {
            return false;
        };
        for p in [&mut line.start, &mut line.end] {
            let x = p[0] - v.origin_x;
            let y = p[1] - v.origin_y;
            *p = match v.page.rotation.rem_euclid(360) {
                90 => [y, v.page.width - x],
                180 => [v.page.width - x, v.page.height - y],
                270 => [v.page.height - y, x],
                _ => [x, y],
            };
        }
        line.start.iter().chain(&line.end).all(|v| v.is_finite())
    });
}
