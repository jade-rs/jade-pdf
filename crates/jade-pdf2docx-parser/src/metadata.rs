//! stream を除いた入力から、最外辞書の型・属性を字句単位で読む。
use crate::syntax::{Lexer, Token, integer, name_is};

/// metadata の検索対象からコメント・文字列を同じ長さの空白へ置き換える。
pub(crate) fn mask_ignored(bytes: &mut [u8]) {
    let mut cursor = 0;
    loop {
        let next = {
            let mut lexer = Lexer { bytes, cursor };
            lexer
                .next_spanned()
                .map(|(token, span)| (matches!(token, Token::Other), span))
        };
        let Some((ignored, span)) = next else {
            bytes[cursor..].fill(b' ');
            break;
        };
        bytes[cursor..span.start].fill(b' ');
        if ignored {
            bytes[span.clone()].fill(b' ');
            // delimiter を保ち、隣接する name と値を結合させずに文字列を隠す。
            if span.len() >= 2 {
                bytes[span.start..span.start + 2].copy_from_slice(b"[]");
            } else {
                bytes[span.start] = b'0';
            }
        }
        cursor = span.end;
    }
}

/// 最外 dictionary の範囲を配列へ保存せず、一件ずつ借用する。
pub(crate) fn dictionary_iter(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut lexer = Lexer { bytes, cursor: 0 };
    let mut depth = 0usize;
    let mut start = None;
    std::iter::from_fn(move || {
        while let Some((token, span)) = lexer.next_spanned() {
            match token {
                Token::Open(dictionary) => {
                    if depth == 0 && dictionary {
                        start = Some(span.start);
                    }
                    depth += 1;
                }
                Token::Close(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0
                        && let Some(start) = start.take()
                    {
                        return Some(&bytes[start..span.end]);
                    }
                }
                _ => {}
            }
        }
        None
    })
}

/// 最外辞書直下のキーと値を借用して列挙する。
pub(crate) struct DictionaryEntries<'a> {
    /// 値の範囲を返す元の辞書。
    dictionary: &'a [u8],
    /// 入れ子の値を一度に読み飛ばす字句解析器。
    lexer: Lexer<'a>,
}

impl<'a> Iterator for DictionaryEntries<'a> {
    type Item = (&'a [u8], &'a [u8]);

    /// 辞書直下の name と、その値全体を返す。
    fn next(&mut self) -> Option<Self::Item> {
        let Token::Name(name) = self.lexer.next()? else {
            return None;
        };
        let span = value_span(&mut self.lexer)?;
        Some((name, &self.dictionary[span]))
    }
}

/// 最外辞書の値を、入れ子・文字列・間接参照を保って列挙する。
pub(crate) fn entries(dictionary: &[u8]) -> Option<DictionaryEntries<'_>> {
    let mut lexer = Lexer {
        bytes: dictionary,
        cursor: 0,
    };
    if !matches!(lexer.next()?, Token::Open(true)) {
        return None;
    }
    Some(DictionaryEntries { dictionary, lexer })
}

/// 最外辞書直下のキー・値の組と終端を検査し、余剰tokenを黙って捨てない。
pub(crate) fn dictionary_entries_complete(dictionary: &[u8]) -> bool {
    let Some(mut entries) = entries(dictionary) else {
        return false;
    };
    loop {
        let mut next = entries.lexer.clone();
        match next.next() {
            Some(Token::Close(true)) => return next.next().is_none(),
            Some(Token::Name(_)) => {
                if entries.next().is_none() {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// 最外辞書の指定キーに対応する値を、入れ子と間接参照を保って返す。
pub(crate) fn value<'a>(dictionary: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    entries(dictionary)?.find_map(|(name, value)| name_is(name, key).then_some(value))
}

/// 最外辞書の Type が指定 PDF name と一致するか判定する。
pub(crate) fn has_type(dictionary: &[u8], expected: &[u8]) -> bool {
    let Some(value) = value(dictionary, b"Type") else {
        return false;
    };
    matches!(Lexer { bytes: value, cursor: 0 }.next(), Some(Token::Name(name)) if name_is(name, expected))
}

/// 最外 Page 辞書だけを入力順で返す。
pub(crate) fn page_iter(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    dictionary_iter(bytes).filter(|dictionary| has_type(dictionary, b"Page"))
}

/// 値が間接参照なら、generation を字句検査して object 番号を返す。
pub(crate) fn reference(value: &[u8]) -> Option<u32> {
    reference_with_generation(value).map(|(number, _)| number)
}

/// 間接参照の object 番号と世代番号を、後続 token も含めて検査する。
pub(crate) fn reference_with_generation(value: &[u8]) -> Option<(u32, u16)> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    let Token::Word(number) = lexer.next()? else {
        return None;
    };
    let number = u32::try_from(integer(number)?).ok()?;
    let Token::Word(generation) = lexer.next()? else {
        return None;
    };
    let generation = u16::try_from(integer(generation)?).ok()?;
    if !matches!(lexer.next(), Some(Token::Word(b"R"))) || lexer.next().is_some() {
        return None;
    }
    Some((number, generation))
}

/// 実際の間接 object の header と本文。
pub(crate) struct Object<'a> {
    /// header の元入力内位置。
    pub(crate) offset: usize,
    /// endobj の直後の元入力内位置。
    pub(crate) end: usize,
    /// header の object 番号。
    pub(crate) number: u32,
    /// header の generation 番号。
    pub(crate) generation: u16,
    /// header と endobj の間の元入力。
    pub(crate) body: &'a [u8],
}

/// 入れ子・文字列・コメント・stream 本文を除いて object を走査する。
pub(crate) struct Objects<'a, 'i> {
    /// 元入力を読む字句解析器。
    lexer: Lexer<'a>,
    /// 検査済み stream の本文境界。
    index: Option<&'i crate::stream_index::StreamIndex>,
}

/// 元入力を複製せず、間接 object の走査器を作る。
pub(crate) fn objects<'a, 'i>(
    bytes: &'a [u8],
    index: Option<&'i crate::stream_index::StreamIndex>,
) -> Objects<'a, 'i> {
    Objects {
        lexer: Lexer { bytes, cursor: 0 },
        index,
    }
}

impl<'a> Iterator for Objects<'a, '_> {
    type Item = Object<'a>;

    /// 最外にある header から次の object 本文を返す。
    fn next(&mut self) -> Option<Self::Item> {
        let mut depth = 0usize;
        loop {
            let (token, header_span) = self.lexer.next_spanned()?;
            if depth == 0
                && let Token::Word(number) = token
                && let Some(number) = integer(number).and_then(|n| u32::try_from(n).ok())
            {
                let mut header = self.lexer.clone();
                if let Some(Token::Word(generation)) = header.next()
                    && let Some(generation) =
                        integer(generation).and_then(|n| u16::try_from(n).ok())
                    && matches!(header.next(), Some(Token::Word(b"obj")))
                {
                    self.lexer = header;
                    let start = self.lexer.cursor;
                    let mut body_depth = 0usize;
                    while let Some((token, span)) = self.lexer.next_spanned() {
                        match token {
                            Token::Open(_) => body_depth += 1,
                            Token::Close(_) => body_depth = body_depth.saturating_sub(1),
                            Token::Word(b"endobj") if body_depth == 0 => {
                                return Some(Object {
                                    offset: header_span.start,
                                    end: span.end,
                                    number,
                                    generation,
                                    body: &self.lexer.bytes[start..span.start],
                                });
                            }
                            Token::Word(b"stream") if body_depth == 0 => {
                                let start = self.lexer.stream_data_start();
                                if let Some(end) =
                                    self.index.and_then(|index| index.payload_end_at(start))
                                {
                                    self.lexer.cursor = end;
                                } else {
                                    // 本文除外済みの合成入力と旧 unit fixture の fallback。
                                    self.lexer.skip_stream(None);
                                }
                            }
                            _ => {}
                        }
                    }
                    return None;
                }
            }
            match token {
                Token::Open(_) => depth += 1,
                Token::Close(_) => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
}

/// 値が指定 PDF name か、escape を展開して比較する。
pub(crate) fn is_name(bytes: &[u8], expected: &[u8]) -> bool {
    let mut lexer = Lexer { bytes, cursor: 0 };
    matches!(lexer.next(), Some(Token::Name(name)) if name_is(name, expected))
        && lexer.next().is_none()
}

/// resource 名の各 byte を一意な文字へ写し、非 UTF-8 名も照合できる内部キーを返す。
#[cfg(test)]
pub(crate) fn resource_name(raw: &[u8]) -> Option<String> {
    Some(
        decoded_name_bytes(raw)?
            .into_iter()
            .map(char::from)
            .collect(),
    )
}

/// PDF name の #xx escape を byte 列へ復号する。
#[cfg(test)]
fn decoded_name_bytes(raw: &[u8]) -> Option<Vec<u8>> {
    let mut decoded = Vec::with_capacity(raw.len());
    let mut cursor = 0;
    while let Some(&byte) = raw.get(cursor) {
        if byte == b'#' {
            let hex = raw.get(cursor + 1..cursor + 3)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            let hex = std::str::from_utf8(hex).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            cursor += 3;
        } else {
            decoded.push(byte);
            cursor += 1;
        }
    }
    Some(decoded)
}

/// 次の値の範囲を、入れ子・間接参照を含めて読む。
fn value_span(lexer: &mut Lexer<'_>) -> Option<std::ops::Range<usize>> {
    let (token, mut span) = lexer.next_spanned()?;
    if matches!(token, Token::Open(_)) {
        let mut depth = 1usize;
        while depth > 0 {
            let (next, next_span) = lexer.next_spanned()?;
            match next {
                Token::Open(_) => depth += 1,
                Token::Close(_) => depth -= 1,
                _ => {}
            }
            span.end = next_span.end;
        }
    } else if matches!(token, Token::Word(word) if integer(word).is_some()) {
        let mut reference = lexer.clone();
        if matches!(reference.next(), Some(Token::Word(word)) if integer(word).is_some())
            && matches!(reference.next(), Some(Token::Word(b"R")))
        {
            span.end = reference.cursor;
            *lexer = reference;
        }
    } else if matches!(token, Token::Close(_)) {
        return None;
    }
    Some(span)
}

/// 配列要素をコピーせず、要素数と終端を検査して順に借用する。
pub(crate) struct ArrayValues<'a> {
    /// 入力を借用する字句走査器。
    lexer: Lexer<'a>,
    /// 許可する最大要素数。
    maximum: usize,
    /// 既に返した要素数。
    count: usize,
    /// 正常な配列終端を確認済みか。
    finished: bool,
}

impl<'a> ArrayValues<'a> {
    /// 配列の開始を検査し、heap を確保せずに走査器を作る。
    pub(crate) fn new(bytes: &'a [u8], maximum: usize) -> Option<Self> {
        let mut lexer = Lexer { bytes, cursor: 0 };
        if !matches!(lexer.next()?, Token::Open(false)) {
            return None;
        }
        Some(Self {
            lexer,
            maximum,
            count: 0,
            finished: false,
        })
    }

    /// 次の値、正常終端、解析失敗をそれぞれ Some(Some)、Some(None)、None で返す。
    pub(crate) fn next_value(&mut self) -> Option<Option<&'a [u8]>> {
        if self.finished {
            return Some(None);
        }
        let mut closing = self.lexer.clone();
        if matches!(closing.next(), Some(Token::Close(false))) {
            if closing.next().is_some() {
                return None;
            }
            self.finished = true;
            return Some(None);
        }
        if self.count >= self.maximum {
            return None;
        }
        let span = value_span(&mut self.lexer)?;
        self.count += 1;
        Some(Some(&self.lexer.bytes[span]))
    }
}

/// 小さい属性配列を読み、要素数の上限を超える値を拒否する。
pub(crate) fn array_values(bytes: &[u8], maximum: usize) -> Option<Vec<&[u8]>> {
    array_entries(bytes, maximum)?.collect()
}

/// 配列の値を保存せず借用し、閉じ括弧・上限・末尾の破損を一度だけ返す。
pub(crate) fn array_entries(
    bytes: &[u8],
    maximum: usize,
) -> Option<impl Iterator<Item = Option<&[u8]>>> {
    let mut lexer = Lexer { bytes, cursor: 0 };
    if !matches!(lexer.next()?, Token::Open(false)) {
        return None;
    }
    let mut count = 0usize;
    let mut finished = false;
    Some(std::iter::from_fn(move || {
        if finished {
            return None;
        }
        let mut closing = lexer.clone();
        if matches!(closing.next(), Some(Token::Close(false))) {
            finished = true;
            return closing.next().is_some().then_some(None);
        }
        if count >= maximum {
            finished = true;
            return Some(None);
        }
        match value_span(&mut lexer) {
            Some(span) => {
                count += 1;
                Some(Some(&bytes[span]))
            }
            None => {
                finished = true;
                Some(None)
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    /// 借用走査は間接参照・入れ子・コメントを一要素として保持する。
    #[test]
    fn borrowed_array_values_preserve_operand_boundaries() {
        let mut values =
            super::ArrayValues::new(b"[1 0 R % comment\n [500 600] << /X 7 >>]", 3).unwrap();
        assert_eq!(values.next_value(), Some(Some(b"1 0 R".as_slice())));
        assert_eq!(values.next_value(), Some(Some(b"[500 600]".as_slice())));
        assert_eq!(values.next_value(), Some(Some(b"<< /X 7 >>".as_slice())));
        assert_eq!(values.next_value(), Some(None));
        assert_eq!(values.next_value(), Some(None));
    }

    /// 空配列と inclusive 要素上限を許可し、過剰・未完・後続 operand を拒否する。
    #[test]
    fn borrowed_array_values_check_limit_and_termination() {
        assert_eq!(
            super::ArrayValues::new(b"[]", 0).unwrap().next_value(),
            Some(None)
        );
        for bytes in [b"[1 2]".as_slice(), b"[1", b"[1] junk"] {
            let mut values = super::ArrayValues::new(bytes, 1).unwrap();
            assert_eq!(values.next_value(), Some(Some(b"1".as_slice())));
            assert_eq!(values.next_value(), None);
        }
        assert!(super::ArrayValues::new(b"<<>>", 1).is_none());
    }

    /// #xx は符号付き数値ではなく二桁の十六進数だけを許す。
    #[test]
    fn resource_name_rejects_non_hex_escape() {
        for raw in [b"F#+F".as_slice(), b"F#-0", b"F#0g", b"F#F", b"F##"] {
            assert_eq!(super::resource_name(raw), None, "{raw:?}");
        }
    }
}
