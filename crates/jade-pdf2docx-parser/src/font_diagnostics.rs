//! 実際の文字描画に使われた未対応 Font を字句状態から識別する。
use crate::{UnsupportedFontReason, metadata, resource_resolver::ResourceResolver, syntax::Token};
use std::ops::Range;

/// 選択済み Font のうち現行の glyph 描画を再現できないもの。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Selection {
    /// alias 解決後の object 番号。直接辞書では未指定。
    pub(crate) object_number: Option<u32>,
    /// 任意長の Subtype 名を保存しない固定理由。
    pub(crate) reason: UnsupportedFontReason,
}

/// Font の選択と文字描画 operand を追う。q/Q 保存は呼び出し側で行う。
#[derive(Default)]
pub(crate) struct State {
    /// Font 自体を選択時に解決し、Form の同名 resource に取り違えない。
    pub(crate) selected: Option<Selection>,
    /// BT/ET の文字描画区間。
    in_text: bool,
    /// Tf operand のresource名を本文内の範囲として保持し、復号名を確保しない。
    name: Option<Range<usize>>,
    /// resource 名の後に有限なサイズが一つ続いたか。
    size: bool,
    /// 直前の文字列または TJ array が非空の文字 byte を持つか。
    text: bool,
    /// 偽命令を除外する container 深度。
    depth: usize,
    /// 最外 container が TJ operand の array か。
    array: bool,
}

impl State {
    /// Form への暗黙の graphics state 保存で Font だけ継承する。
    pub(crate) fn inherited(selected: Option<Selection>) -> Self {
        Self {
            selected,
            ..Default::default()
        }
    }

    /// 一つの字句を進め、非空の文字描画で使った未対応 Font を返す。
    pub(crate) fn observe<'bytes>(
        &mut self,
        token: Token<'_>,
        content: &[u8],
        span: Range<usize>,
        resources: &'bytes [u8],
        resolver: &mut ResourceResolver<'_, 'bytes>,
    ) -> Option<Selection> {
        match token {
            Token::Open(dictionary) => {
                if self.depth == 0 {
                    self.array = !dictionary;
                    self.text = false;
                }
                self.depth += 1;
                self.name = None;
                self.size = false;
            }
            Token::Close(_) => self.depth = self.depth.saturating_sub(1),
            Token::Other if self.depth == 0 || self.depth == 1 && self.array => {
                self.text |= has_string_bytes(&content[span]);
                self.name = None;
                self.size = false;
            }
            _ if self.depth != 0 => {}
            Token::Name(name) => {
                self.name = crate::metadata_names::valid_resource_name(name)
                    .then_some(span.start + 1..span.end);
                self.size = false;
                self.text = false;
            }
            Token::Word(word) => {
                if std::str::from_utf8(word)
                    .ok()
                    .and_then(|s| s.parse::<f32>().ok())
                    .is_some_and(f32::is_finite)
                {
                    if self.name.is_some() && !self.size {
                        self.size = true;
                    } else {
                        self.name = None;
                    }
                    self.text = false;
                    return None;
                }
                let used = match word {
                    b"Tf" => {
                        if self.size
                            && let Some(name) = self.name.as_ref()
                        {
                            self.selected = select(resources, &content[name.clone()], resolver);
                        }
                        None
                    }
                    b"BT" => {
                        self.in_text = true;
                        None
                    }
                    b"ET" => {
                        self.in_text = false;
                        None
                    }
                    b"Tj" | b"TJ" | b"'" | b"\"" if self.in_text && self.text => self.selected,
                    _ => None,
                };
                self.name = None;
                self.size = false;
                self.text = false;
                return used;
            }
            _ => {
                self.name = None;
                self.size = false;
                self.text = false;
            }
        }
        None
    }
}

/// 現在の局所 Font map から参照を解決し、未対応 Subtype のみを保持する。
fn select<'bytes>(
    resources: &'bytes [u8],
    name: &[u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<Selection> {
    let (fonts, _) = metadata::value(resources, b"Font").and_then(|v| resolver.resolve(v))?;
    let value = metadata::entries(fonts)?
        .into_iter()
        .find_map(|(key, value)| {
            crate::metadata_names::same_resource_name(key, name).then_some(value)
        })?;
    let (font, object_number) = resolver.resolve(value)?;
    let (subtype, _) = metadata::value(font, b"Subtype").and_then(|v| resolver.resolve(v))?;
    if [b"Type1".as_slice(), b"MMType1", b"TrueType", b"Type0"]
        .iter()
        .any(|name| metadata::is_name(subtype, name))
    {
        return None;
    }
    Some(Selection {
        object_number,
        reason: if metadata::is_name(subtype, b"Type3") {
            UnsupportedFontReason::Type3
        } else {
            UnsupportedFontReason::Subtype
        },
    })
}

/// 空文字と escape による改行継続だけの literal を描画と誤認しない。
fn has_string_bytes(raw: &[u8]) -> bool {
    if raw.first() == Some(&b'<') {
        return raw
            .get(1..raw.len().saturating_sub(1))
            .is_some_and(|s| s.iter().any(|b| b.is_ascii_hexdigit()));
    }
    if raw.first() != Some(&b'(') {
        return false;
    }
    let mut cursor = 1;
    let end = raw.len().saturating_sub(1);
    while cursor < end {
        if raw[cursor] == b'\\' && cursor + 1 < end && matches!(raw[cursor + 1], b'\r' | b'\n') {
            cursor += 2;
            if raw[cursor - 1] == b'\r' && raw.get(cursor) == Some(&b'\n') {
                cursor += 1;
            }
        } else {
            return true;
        }
    }
    false
}
