//! Form の局所 Resources と描画ごとの CTM を有限な stack で辿る。
use crate::{
    QualityReport,
    decode::DecodeBudget,
    font_diagnostics::{Selection, State},
    form_clip::Clip,
    form_operands::Operands,
    metadata,
    metadata_workspace::try_push,
    parser,
    resource_resolver::ResourceResolver,
    syntax::{Lexer, Token},
};
use std::{ops::Range, rc::Rc};

pub(crate) use crate::form_matrices::text_matrices;

/// 座標変換の初期値。
const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// q/Qで退避する行列・選択Font・文字状態。
struct Saved<'a> {
    /// 呼び出し元の座標変換。
    matrix: [f32; 6],
    /// 未対応Fontの診断用選択。
    selected: Option<Selection>,
    /// 文字と塗りの保存状態。
    text: crate::text_draws::State,
    /// 保存時のFont選択元辞書。
    resources: &'a [u8],
}

/// 一つのページまたは Form の非再帰走査状態。
struct Frame<'a> {
    /// Form の有効 object 番号。ページ本文には番号を持たせない。
    number: Option<u32>,
    /// decode 済み本文を描画間で共有する。
    content: Rc<Vec<u8>>,
    /// inline binary の直前までの走査可能な本文長。
    end: usize,
    /// この呼び出しで有効な局所 Resources。
    resources: &'a [u8],
    /// 次に読む字句の byte 位置。
    cursor: usize,
    /// 現在の CTM。
    matrix: [f32; 6],
    /// 呼び出し時の BBox と親領域の交差。
    clip: Clip,
    /// 本文の通常path clipの変更点。
    clips: crate::content_clip::Clips,
    /// q/Qの退避行列・診断Font・文字状態・選択時Resources。
    saved: Vec<Saved<'a>>,
    /// 呼び出し元から継承する文字状態。
    text: crate::text_draws::State,
    /// 選択中のFontが定義されたResources。
    font_resources: &'a [u8],
    /// 文字状態更新のための固定長operand列。
    text_operands: crate::form_text_state::Operands,
    /// 選択 Font と文字描画の診断状態。
    fonts: State,
    /// cm の有限な数値 operand。
    operands: Operands,
    /// Doの直前の名前を本文内の範囲で保持し、復号名を確保しない。
    name: Option<Range<usize>>,
    /// container 内の偽 operator を除外する深度。
    depth: usize,
}

impl<'a> Frame<'a> {
    /// 本文と局所辞書から空の字句状態を作る。
    fn new(
        budget: &DecodeBudget,
        number: Option<u32>,
        content: Rc<Vec<u8>>,
        resources: &'a [u8],
        matrix: [f32; 6],
        clip: Clip,
    ) -> Self {
        let clips = crate::content_clip::scan(budget, &content, matrix);
        Self {
            number,
            end: safe_prefix(&content).len(),
            content,
            resources,
            cursor: 0,
            matrix,
            clip,
            clips,
            saved: Vec::new(),
            text: crate::text_draws::State::initial(Default::default()),
            font_resources: resources,
            text_operands: Default::default(),
            fonts: State::default(),
            operands: Operands::default(),
            name: None,
            depth: 0,
        }
    }
}

/// inline binary を本文として解釈しないため最初のトップレベル BI までに制限する。
fn safe_prefix(content: &[u8]) -> &[u8] {
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    let mut depth = 0usize;
    while let Some((token, span)) = lexer.next_spanned() {
        match token {
            Token::Open(_) => depth += 1,
            Token::Close(_) => depth = depth.saturating_sub(1),
            Token::Word(b"BI") if depth == 0 => return &content[..span.start],
            _ => {}
        }
    }
    content
}

/// 有効ページの Font 描画を診断し、呼ばれた Form を局所辞書と合成 CTM 付きで通知する。
pub(crate) fn visit(
    budget: &DecodeBudget,
    bytes: &[u8],
    report: &mut QualityReport,
    mut visitor: impl FnMut(usize, &[u8], &[u8], [f32; 6], &Clip, crate::form_text_state::Inherited<'_>),
) {
    let source = parser::parser_source_with_compressed_objects(budget, bytes);
    let Some(objects) = crate::object_index::build(budget, &source) else {
        return;
    };
    let mut resolver = ResourceResolver::new(&objects, budget);
    let mut seen_fonts = crate::form_diagnostics::Fonts::default();
    let resources = budget.page_resources(&source).unwrap_or_default();
    let offsets = parser::stream_object_offsets(budget, bytes);
    let mut payloads = crate::form_storage::Payloads::default();
    let Some(pages) = crate::form_storage::pages(
        budget,
        parser::extract_page_content_streams_with_page(budget, bytes),
    ) else {
        return;
    };
    for (page, content) in pages {
        let resources = resources.get(page).copied().flatten().unwrap_or(b"<< >>");
        let Some(clip) = Clip::page(
            budget,
            budget.page_media_box(page),
            budget.page_crop_box(page).flatten(),
        ) else {
            return;
        };
        let mut stack = Vec::new();
        if try_push(
            budget,
            &mut stack,
            Frame::new(budget, None, content, resources, IDENTITY, clip),
        )
        .is_none()
        {
            return;
        }
        while let Some(frame) = stack.last_mut() {
            if budget.is_stopped() {
                return;
            }
            if frame.clips.unsupported
                && !report.issues.iter().any(|issue| {
                    matches!(issue, crate::QualityIssue::UnsupportedClippingPath { page: previous } if *previous == page)
                })
            {
                if try_push(
                    budget,
                    &mut report.issues,
                    crate::QualityIssue::UnsupportedClippingPath { page },
                ).is_none() {
                    return;
                }
            }
            frame.clips.unsupported = false;
            let content = &frame.content[..frame.end];
            let mut lexer = Lexer {
                bytes: content,
                cursor: frame.cursor,
            };
            let token = lexer.next_spanned();
            frame.cursor = lexer.cursor;
            let Some((token, span)) = token else {
                stack.pop();
                continue;
            };
            if let Some(selection) =
                frame
                    .fonts
                    .observe(token, content, span.clone(), frame.resources, &mut resolver)
                && frame.text.rendering.uses_glyph_shape_for(
                    selection.reason == crate::UnsupportedFontReason::Type3,
                    budget.is_pdf20(),
                )
                && seen_fonts.record(budget, report, page, selection).is_none()
            {
                return;
            }
            if frame.text_operands.observe(
                budget,
                &mut frame.text,
                token,
                span.clone(),
                content,
                |name| crate::text_alpha::resolve(frame.resources, name, &mut resolver),
            ) {
                frame.font_resources = frame.resources;
            }
            match token {
                Token::Open(_) => {
                    frame.depth += 1;
                    frame.name = None;
                    frame.operands.clear();
                }
                Token::Close(_) => {
                    frame.depth = frame.depth.saturating_sub(1);
                }
                _ if frame.depth != 0 => {}
                Token::Name(raw) => {
                    frame.name = crate::metadata_names::valid_resource_name(raw)
                        .then_some(span.start + 1..span.end);
                    frame.operands.clear();
                }
                Token::Word(b"Do") => {
                    let Some(name) = frame.name.take() else {
                        continue;
                    };
                    let name = &content[name];
                    let inherited_resources = frame.resources;
                    let parent_matrix = frame.matrix;
                    let selected = frame.fonts.selected;
                    let font_resources = frame.font_resources;
                    let Some((xobjects, _)) = metadata::value(inherited_resources, b"XObject")
                        .and_then(|v| resolver.resolve(v))
                    else {
                        continue;
                    };
                    let Some(value) = metadata::entries(xobjects).into_iter().flatten().find_map(
                        |(key, value)| {
                            crate::metadata_names::same_resource_name(key, name).then_some(value)
                        },
                    ) else {
                        continue;
                    };
                    let Some((dictionary, Some(number))) = resolver.resolve(value) else {
                        continue;
                    };
                    if !metadata::value(dictionary, b"Subtype")
                        .and_then(|v| resolver.resolve(v))
                        .is_some_and(|(v, _)| metadata::is_name(v, b"Form"))
                        || stack.iter().any(|frame| frame.number == Some(number))
                    {
                        continue;
                    }
                    let matrix = match metadata::value(dictionary, b"Matrix") {
                        None => IDENTITY,
                        Some(value) => {
                            let Some(matrix) = matrix(value, &mut resolver) else {
                                continue;
                            };
                            matrix
                        }
                    };
                    let matrix = parser::concat_matrix(parent_matrix, matrix);
                    if !matrix.iter().all(|v| v.is_finite()) {
                        continue;
                    }
                    let Some(bbox) = metadata::value(dictionary, b"BBox")
                        .and_then(|value| numbers::<4>(value, &mut resolver))
                    else {
                        continue;
                    };
                    let parent = stack.last().unwrap();
                    let combined = match parent.clips.get(span.start) {
                        Some(path) => parent.clip.intersection(budget, path),
                        None => None,
                    };
                    if budget.is_stopped() {
                        return;
                    }
                    let Some(clip) = combined
                        .as_ref()
                        .unwrap_or(&parent.clip)
                        .child(budget, bbox, matrix)
                    else {
                        return;
                    };
                    let local_resources = match metadata::value(dictionary, b"Resources") {
                        Some(value) => {
                            let Some((v, _)) = resolver.resolve(value) else {
                                continue;
                            };
                            if crate::page_attributes::is_null(v) {
                                inherited_resources
                            } else {
                                v
                            }
                        }
                        None => inherited_resources,
                    };
                    let content = payloads.get_or_load(budget, number, || {
                        parser::form_stream_payload(budget, bytes, number, &offsets)
                    });
                    if budget.is_stopped() {
                        return;
                    }
                    let Some(content) = content else {
                        continue;
                    };
                    if budget.charge_form_bytes(content.len()).is_none()
                        || budget.check_form_depth(stack.len()).is_none()
                    {
                        return;
                    }
                    let mut child =
                        Frame::new(budget, Some(number), content, local_resources, matrix, clip);
                    child.fonts = State::inherited(selected);
                    let Some(text) = stack.last().unwrap().text.saved(budget) else {
                        return;
                    };
                    child.text = text;
                    child.font_resources = font_resources;
                    if try_push(budget, &mut stack, child).is_none() {
                        return;
                    }
                    let child = stack.last().unwrap();
                    visitor(
                        page,
                        &child.content[..child.end],
                        local_resources,
                        matrix,
                        &child.clip,
                        crate::form_text_state::Inherited {
                            state: &child.text,
                            resources: child.font_resources,
                        },
                    );
                }
                Token::Word(word) => {
                    if let Some(value) = std::str::from_utf8(word)
                        .ok()
                        .and_then(|w| w.parse::<f32>().ok())
                        .filter(|v| v.is_finite())
                    {
                        frame.operands.push(value);
                    } else {
                        match word {
                            b"q" => {
                                if budget
                                    .check_form_graphics_depth(frame.saved.len() + 1)
                                    .is_none()
                                {
                                    return;
                                }
                                let Some(text) = frame.text.saved(budget) else {
                                    return;
                                };
                                if try_push(
                                    budget,
                                    &mut frame.saved,
                                    Saved {
                                        matrix: frame.matrix,
                                        selected: frame.fonts.selected,
                                        text,
                                        resources: frame.font_resources,
                                    },
                                )
                                .is_none()
                                {
                                    return;
                                }
                            }
                            b"Q" => {
                                if let Some(saved) = frame.saved.pop() {
                                    frame.matrix = saved.matrix;
                                    frame.fonts.selected = saved.selected;
                                    frame.text = saved.text;
                                    frame.font_resources = saved.resources;
                                }
                            }
                            b"cm" if frame.operands.len() == 6 => {
                                frame.matrix = parser::concat_matrix(
                                    frame.matrix,
                                    frame.operands.as_slice().try_into().unwrap(),
                                )
                            }
                            _ => {}
                        }
                        frame.operands.clear();
                    }
                    frame.name = None;
                }
                _ => {
                    frame.name = None;
                    frame.operands.clear();
                }
            }
        }
    }
}

/// Matrix と各座標の参照を解決し、null は単位行列、数値は PDF 字句として読む。
fn matrix<'bytes>(
    value: &'bytes [u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<[f32; 6]> {
    let (value, _) = resolver.resolve(value)?;
    if crate::page_attributes::is_null(value) {
        return Some(IDENTITY);
    }
    numbers(value, resolver)
}

/// 検証済み BBox/Matrix の配列と各数値の参照を読み取る。
fn numbers<'bytes, const N: usize>(
    value: &'bytes [u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<[f32; N]> {
    let (value, _) = resolver.resolve(value)?;
    let values = metadata::array_values(value, N)?;
    let mut result = [0.0f32; N];
    if values.len() != N {
        return None;
    }
    for (target, value) in result.iter_mut().zip(values) {
        let (value, _) = resolver.resolve(value)?;
        *target = crate::page_attributes::number_value(value)?;
    }
    Some(result)
}

#[cfg(test)]
#[path = "form_storage_regression_tests.rs"]
mod storage_regression_tests;

#[cfg(test)]
#[path = "form_state_regression_tests.rs"]
mod state_regression_tests;

#[cfg(test)]
#[path = "form_diagnostic_regression_tests.rs"]
mod diagnostic_regression_tests;

#[cfg(test)]
#[path = "form_clip_regression_tests.rs"]
mod clip_regression_tests;
