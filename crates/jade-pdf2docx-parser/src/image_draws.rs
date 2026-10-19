//! 画像Doの借用字句走査とページ別CTMを累積metadata予算で制限する。
use crate::{
    decode::DecodeBudget,
    form_operands::Operands,
    metadata_names::valid_resource_name,
    metadata_workspace::try_push,
    syntax::{Lexer, Token},
};

#[path = "image_paint.rs"]
mod paint;
#[path = "image_paint_resources.rs"]
mod resources;
pub(super) use resources::visit as visit_in_source;

/// Do時の非stroke色と透明度。通常画像は色を使わない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ImagePaint {
    /// 対応済みDevice色。未対応色空間ではNone。
    pub(super) color: Option<[u8; 3]>,
    /// 非stroke alpha。未解決・未対応の描画状態ではNone。
    pub(super) opacity: Option<f64>,
}

impl Default for ImagePaint {
    /// PDFの既定である黒・不透明を返す。
    fn default() -> Self {
        Self {
            color: Some([0; 3]),
            opacity: Some(1.0),
        }
    }
}

impl ImagePaint {
    /// stencilは色とalpha、通常画像はalphaが既知の場合だけ再現する。
    pub(super) fn supported(self, stencil: bool) -> bool {
        self.opacity.is_some() && (!stencil || self.color.is_some())
    }
}

/// q/Qで同時に保存する行列と非stroke色。
#[derive(Clone, Copy)]
struct GraphicsState {
    /// 現在の描画行列。
    matrix: [f32; 6],
    /// stencilの塗り色と透明状態。
    paint: paint::Paint,
}

/// Contents境界をまたいで保持するページの描画状態。
struct State<'a> {
    /// 現在の単位正方形の変換行列。
    graphics: GraphicsState,
    /// qで退避した行列と塗り色。拡張前に全slot容量を予約する。
    saved: Vec<GraphicsState>,
    /// cm直前の数値を無確保で保持する。
    numbers: Operands,
    /// Do直前のresource名。Contents本文を借用する。
    name: Option<&'a [u8]>,
    /// 配列・辞書内部の偽operatorを除くための深さ。
    depth: usize,
    /// inline image以降の不確定なbinary境界を走査しない。
    stopped: bool,
}

impl Default for State<'_> {
    /// 描画行列を単位行列とし、作業配列は初回qまで確保しない。
    fn default() -> Self {
        Self {
            graphics: GraphicsState {
                matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                paint: paint::Paint::default(),
            },
            saved: Vec::new(),
            numbers: Operands::default(),
            name: None,
            depth: 0,
            stopped: false,
        }
    }
}

/// 字句順のDoを訪問し、状態・q保存列の容量不足と停止を呼出側へ伝える。
pub(super) fn visit(
    budget: &DecodeBudget,
    contents: &[(usize, Vec<u8>)],
    mut draw: impl FnMut(usize, &[u8], [f32; 6]) -> Option<()>,
) -> Option<()> {
    visit_painted(budget, contents, |page, name, matrix, _| {
        draw(page, name, matrix)
    })
}

/// 画像描画の行列と塗り色を、同じページ状態から同時に返す。
pub(super) fn visit_painted(
    budget: &DecodeBudget,
    contents: &[(usize, Vec<u8>)],
    draw: impl FnMut(usize, &[u8], [f32; 6], ImagePaint) -> Option<()>,
) -> Option<()> {
    visit_resolved(budget, contents, |_, _| None, draw)
}

/// gsのページ別解決を呼出側へ委ね、未知状態を描画色へ伝播する。
fn visit_resolved(
    budget: &DecodeBudget,
    contents: &[(usize, Vec<u8>)],
    mut resolve_state: impl FnMut(usize, &[u8]) -> Option<paint::Parameters>,
    mut draw: impl FnMut(usize, &[u8], [f32; 6], ImagePaint) -> Option<()>,
) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    let mut states: Vec<(usize, State<'_>)> = Vec::new();
    for (page, content) in contents {
        let index = match states.binary_search_by_key(page, |entry| entry.0) {
            Ok(index) => index,
            Err(index) => {
                try_push(budget, &mut states, (*page, State::default()))?;
                states[index..].rotate_right(1);
                index
            }
        };
        scan(
            budget,
            &mut states[index].1,
            content,
            |name| resolve_state(*page, name),
            |name, matrix, color| draw(*page, name, matrix, color),
        )?;
    }
    Some(())
}

/// 名前は復号Stringを作らず借用し、数値・容器・演算子を字句として区別する。
fn scan<'a>(
    budget: &DecodeBudget,
    state: &mut State<'a>,
    content: &'a [u8],
    mut resolve_state: impl FnMut(&[u8]) -> Option<paint::Parameters>,
    mut draw: impl FnMut(&[u8], [f32; 6], ImagePaint) -> Option<()>,
) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    if state.stopped {
        return Some(());
    }
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    while let Some(token) = lexer.next() {
        if budget.is_stopped() {
            return None;
        }
        match token {
            Token::Open(_) => {
                state.depth += 1;
                state.name = None;
                state.numbers.clear();
            }
            Token::Close(_) => {
                state.depth = state.depth.saturating_sub(1);
                state.name = None;
                state.numbers.clear();
            }
            _ if state.depth != 0 => {}
            Token::Name(name) => {
                state.name = valid_resource_name(name).then_some(name);
                state.numbers.clear();
            }
            Token::Word(word) => {
                if let Some(value) = std::str::from_utf8(word)
                    .ok()
                    .and_then(|word| word.parse::<f32>().ok())
                    .filter(|value| value.is_finite())
                {
                    state.numbers.push(value);
                    state.name = None;
                    continue;
                }
                if word == b"gs" {
                    let parameters = state.name.and_then(&mut resolve_state);
                    state.graphics.paint.apply_state(parameters);
                } else {
                    state
                        .graphics
                        .paint
                        .apply(word, state.numbers.as_slice(), state.name);
                }
                match word {
                    b"q" => {
                        budget.check_form_graphics_depth(state.saved.len() + 1)?;
                        try_push(budget, &mut state.saved, state.graphics)?;
                    }
                    b"Q" => {
                        if let Some(previous) = state.saved.pop() {
                            state.graphics = previous;
                        }
                    }
                    b"cm" if state.numbers.len() == 6 => {
                        state.graphics.matrix = super::concat_matrix(
                            state.graphics.matrix,
                            state.numbers.as_slice().try_into().unwrap(),
                        );
                    }
                    b"Do" => {
                        if let Some(name) = state.name {
                            draw(name, state.graphics.matrix, state.graphics.paint.snapshot())?;
                        }
                    }
                    b"BI" => {
                        state.stopped = true;
                        break;
                    }
                    _ => {}
                }
                state.name = None;
                state.numbers.clear();
            }
            _ => {
                state.name = None;
                state.numbers.clear();
            }
        }
    }
    Some(())
}

#[cfg(test)]
#[path = "image_draws_tests.rs"]
mod tests;
