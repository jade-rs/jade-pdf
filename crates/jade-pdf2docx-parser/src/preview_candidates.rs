//! 借用字句からpreview候補を容量制限付きで復号し、候補配列を作らず通知する。
use super::{ToUnicodeCMap, operand};
use crate::{
    decode::DecodeBudget,
    metadata_map::Map,
    syntax::{Lexer, Token},
};

/// 既存previewの候補優先順位を保持する走査方法。
#[derive(Clone, Copy)]
pub(super) enum Mode {
    /// BT/ETの内側を先に読む。
    TextObjects,
    /// font encodingを適用して全体を読む。
    Font,
    /// encoding未指定の表示operandを最後に読む。
    Shown,
}

/// 候補を逐次復号し、容量不足はNone、文字数上限はfalseで通知する。
pub(super) fn visit(
    budget: &DecodeBudget,
    bytes: &[u8],
    cmap: &ToUnicodeCMap,
    encodings: &Map<String, String>,
    mode: Mode,
    mut emit: impl FnMut(String) -> Option<bool>,
) -> Option<bool> {
    if budget.is_stopped() {
        return None;
    }
    scan(budget, bytes, cmap, encodings, mode, &mut emit)
}

/// 借用範囲を走査し、不正文字列の後から次の表示operandを再探索する。
fn scan(
    budget: &DecodeBudget,
    bytes: &[u8],
    cmap: &ToUnicodeCMap,
    encodings: &Map<String, String>,
    mode: Mode,
    emit: &mut impl FnMut(String) -> Option<bool>,
) -> Option<bool> {
    let mut lexer = Lexer { bytes, cursor: 0 };
    let mut range_lexer = lexer.clone();
    let mut malformed_container = false;
    let mut range = matches!(mode, Mode::TextObjects)
        .then(|| next_text_object(&mut range_lexer, &mut malformed_container))
        .flatten();
    let mut rendering = super::preview_rendering::State::default();
    let mut font = None;
    while let Some((token, span)) = lexer.next_spanned() {
        if budget.is_stopped() {
            return None;
        }
        while range.as_ref().is_some_and(|range| span.start >= range.end) {
            range = next_text_object(&mut range_lexer, &mut malformed_container);
        }
        let eligible = !matches!(mode, Mode::TextObjects)
            || range
                .as_ref()
                .is_some_and(|range| range.contains(&span.start));
        if let Token::Word(word) = token {
            rendering.word(budget, word)?;
        } else {
            rendering.clear_operands();
        }
        match token {
            Token::Name(name) if matches!(mode, Mode::Font) => {
                let mut ahead = lexer.clone();
                if let Some(Token::Word(size)) = ahead.next()
                    && crate::page_attributes::number_value(size).is_some()
                    && matches!(ahead.next(), Some(Token::Word(b"Tf")))
                {
                    font = Some(crate::metadata_names::resource_name(budget, name)?);
                    lexer.cursor = ahead.cursor;
                }
            }
            Token::Open(dictionary) => {
                let start = lexer.cursor;
                let mut container = lexer.clone();
                if matches!(mode, Mode::TextObjects) && eligible {
                    container.bytes = &bytes[..range.as_ref()?.end];
                }
                let end = array_end(&mut container);
                lexer.cursor = container.cursor;
                let Some(end) = end else {
                    if matches!(mode, Mode::TextObjects) {
                        lexer.cursor = start;
                        recover_text_object(&mut lexer);
                    }
                    continue;
                };
                if !dictionary && eligible && rendering.paints() && shown_after(&lexer, b"TJ") {
                    let encoding = font
                        .as_deref()
                        .and_then(|name| encodings.get(name))
                        .map(String::as_str);
                    if let Some(text) = join_array(budget, &bytes[start..end], cmap, encoding) {
                        if !emit(text)? {
                            return Some(false);
                        }
                    } else if budget.is_stopped() {
                        return None;
                    }
                }
            }
            Token::Other if matches!(bytes[span.start], b'(' | b'<') => {
                let raw = &bytes[span.clone()];
                if crate::pdf_binary_string::visit_string(raw, |_| Some(())).is_none() {
                    lexer.cursor = span.start + 1;
                    continue;
                }
                if eligible && rendering.paints() && shown_after(&lexer, b"Tj") {
                    let encoding = font
                        .as_deref()
                        .and_then(|name| encodings.get(name))
                        .map(String::as_str);
                    let text = operand::decode(budget, raw, cmap, encoding)?;
                    if !emit(text)? {
                        return Some(false);
                    }
                }
            }
            _ => {}
        }
    }
    Some(true)
}

/// 未完container内の候補を捨て、後続のBT境界から互換復旧する。
fn recover_text_object(lexer: &mut Lexer<'_>) {
    while let Some((token, span)) = lexer.next_spanned() {
        if matches!(token, Token::Word(b"BT")) {
            lexer.cursor = span.start;
            return;
        }
    }
}

/// 完結したBT/ET範囲を返し、未完container後は再度EOFまで先読みしない。
fn next_text_object(
    lexer: &mut Lexer<'_>,
    malformed_container: &mut bool,
) -> Option<std::ops::Range<usize>> {
    let mut start = None;
    while let Some((token, span)) = lexer.next_spanned() {
        match token {
            Token::Word(b"BT") => start = Some(span.end),
            Token::Word(b"ET") => {
                if let Some(begin) = start.take() {
                    return Some(begin..span.start);
                }
            }
            Token::Open(_) if !*malformed_container => {
                let mut ahead = lexer.clone();
                if array_end(&mut ahead).is_some() {
                    lexer.cursor = ahead.cursor;
                } else {
                    *malformed_container = true;
                }
            }
            _ => {}
        }
    }
    None
}

/// コメント・空白を読み飛ばし、完全一致する表示operatorだけを認める。
fn shown_after(lexer: &Lexer<'_>, expected: &[u8]) -> bool {
    matches!(lexer.clone().next(), Some(Token::Word(word)) if word == expected)
}

/// 配列終端を追加保存領域なしで探し、未完配列は残りの範囲を消費する。
fn array_end(lexer: &mut Lexer<'_>) -> Option<usize> {
    let mut depth = 1usize;
    while let Some((token, span)) = lexer.next_spanned() {
        match token {
            Token::Open(_) => depth = depth.checked_add(1)?,
            Token::Close(_) => {
                depth -= 1;
                if depth == 0 {
                    return Some(span.start);
                }
            }
            _ => {}
        }
    }
    None
}

/// TJの直接文字列だけを結合し、初回fragmentを移動して追加確保を避ける。
fn join_array(
    budget: &DecodeBudget,
    bytes: &[u8],
    cmap: &ToUnicodeCMap,
    encoding: Option<&str>,
) -> Option<String> {
    let mut lexer = Lexer { bytes, cursor: 0 };
    let mut depth = 0usize;
    let mut joined: Option<String> = None;
    while let Some((token, span)) = lexer.next_spanned() {
        match token {
            Token::Open(_) => depth = depth.checked_add(1)?,
            Token::Close(_) => depth = depth.checked_sub(1)?,
            Token::Other if depth == 0 && matches!(bytes[span.start], b'(' | b'<') => {
                let text = operand::decode(budget, &bytes[span], cmap, encoding)?;
                if let Some(output) = &mut joined {
                    crate::text_result_storage::extend(budget, output, &text)?;
                } else {
                    joined = Some(text);
                }
            }
            _ => {}
        }
    }
    joined
}
