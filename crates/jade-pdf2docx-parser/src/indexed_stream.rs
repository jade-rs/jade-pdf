use std::{borrow::Cow, path::Path};

use crate::{
    ParseError, ParseLimits,
    decode::{DecodeBudget, decode_zlib},
    limits::check_limit,
    syntax::{Lexer, Token, integer, name_is},
};

/// xref/object stream の解決に必要な直接 dictionary 値だけを保持する。
#[derive(Default)]
pub(crate) struct Dictionary {
    /// xref stream か。
    pub(crate) xref: bool,
    /// object stream か。
    pub(crate) object_stream: bool,
    /// stream 本文の byte 数。
    pub(crate) length: Option<usize>,
    /// xref の object 番号の範囲。
    pub(crate) size: Option<usize>,
    /// xref entry の field 幅。
    pub(crate) widths: Option<Vec<usize>>,
    /// xref subsection の開始番号・件数。
    pub(crate) index: Option<Vec<usize>>,
    /// 過去 revision の xref offset。
    pub(crate) prev: Option<usize>,
    /// xref stream が指定する catalog 参照。
    pub(crate) root: Option<(u32, u16)>,
    /// object stream の member 数。
    pub(crate) count: Option<usize>,
    /// object stream の最初の body offset。
    pub(crate) first: Option<usize>,
    /// FlateDecode を使用するか。
    flate: bool,
    /// 展開後の行差分復元。
    predictor: crate::indexed_predictor::Predictor,
}

/// 正式 offset と直接 Length で切り出した stream object。
pub(crate) struct IndexedStream<'a> {
    /// object 番号。
    pub(crate) number: usize,
    /// generation 番号。
    pub(crate) generation: usize,
    /// 必要な dictionary 値。
    pub(crate) dictionary: Dictionary,
    /// 無圧縮なら借用し、圧縮済みなら予算内で展開した本文。
    pub(crate) payload: Cow<'a, [u8]>,
}

/// 正式な object offset から stream を読み、本文中の marker を検索しない。
pub(crate) fn read<'a>(
    path: &Path,
    bytes: &'a [u8],
    offset: usize,
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<IndexedStream<'a>, ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let mut lexer = Lexer {
        bytes,
        cursor: offset,
    };
    let number = next_integer(path, &mut lexer)?;
    let generation = next_integer(path, &mut lexer)?;
    if !matches!(lexer.next(), Some(Token::Word(b"obj"))) {
        return Err(invalid(path, "invalid indexed stream header"));
    }
    let dictionary = read_dictionary(path, &mut lexer, limits, budget)?;
    if !matches!(lexer.next(), Some(Token::Word(b"stream"))) {
        return Err(invalid(path, "missing indexed stream body"));
    }
    if lexer.bytes.get(lexer.cursor) == Some(&b'\r') {
        lexer.cursor += 1;
    }
    if lexer.bytes.get(lexer.cursor) == Some(&b'\n') {
        lexer.cursor += 1;
    } else if lexer.bytes.get(lexer.cursor.wrapping_sub(1)) != Some(&b'\r') {
        return Err(invalid(path, "missing stream line ending"));
    }
    let start = lexer.cursor;
    let length = dictionary
        .length
        .ok_or_else(|| invalid(path, "missing direct indexed stream Length"))?;
    let end = start
        .checked_add(length)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| invalid(path, "indexed stream Length outside input"))?;
    lexer.cursor = end;
    if !matches!(lexer.next(), Some(Token::Word(b"endstream")))
        || !matches!(lexer.next(), Some(Token::Word(b"endobj")))
    {
        return Err(invalid(path, "indexed stream Length boundary mismatch"));
    }
    let payload = if dictionary.flate {
        let decoded = decode_zlib(&bytes[start..end], budget);
        budget.check()?;
        let mut decoded = decoded.ok_or_else(|| invalid(path, "invalid indexed Flate stream"))?;
        dictionary.predictor.apply(path, &mut decoded)?;
        Cow::Owned(decoded)
    } else {
        Cow::Borrowed(&bytes[start..end])
    };
    Ok(IndexedStream {
        number,
        generation,
        dictionary,
        payload,
    })
}

/// 認識した key だけを保持し、未知の container/参照は上限付きで読み飛ばす。
fn read_dictionary(
    path: &Path,
    lexer: &mut Lexer<'_>,
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<Dictionary, ParseError> {
    if !matches!(lexer.next(), Some(Token::Open(true))) {
        return Err(invalid(path, "missing indexed stream dictionary"));
    }
    check_limit("container nesting", 1, limits.max_container_nesting as u64)?;
    let mut dictionary = Dictionary::default();
    loop {
        let key = match lexer.next() {
            Some(Token::Close(true)) => return Ok(dictionary),
            Some(Token::Name(key)) => key,
            _ => return Err(invalid(path, "invalid indexed stream dictionary key")),
        };
        let value_start = lexer.cursor;
        let value = lexer
            .next()
            .ok_or_else(|| invalid(path, "missing indexed stream dictionary value"))?;
        if name_is(key, b"Root") {
            lexer.cursor = value_start;
            dictionary.root = Some(crate::xref::read_root(path, lexer)?);
        } else if name_is(key, b"Type") {
            dictionary.xref = matches!(value, Token::Name(name) if name_is(name, b"XRef"));
            dictionary.object_stream =
                matches!(value, Token::Name(name) if name_is(name, b"ObjStm"));
        } else if name_is(key, b"W") || name_is(key, b"Index") {
            if !matches!(value, Token::Open(false)) {
                return Err(invalid(path, "invalid xref integer array"));
            }
            check_limit("container nesting", 2, limits.max_container_nesting as u64)?;
            let mut array = Vec::new();
            while let Some(token) = lexer.next() {
                if matches!(token, Token::Close(false)) {
                    break;
                }
                let Token::Word(word) = token else {
                    return Err(invalid(path, "invalid xref array member"));
                };
                let number =
                    integer(word).ok_or_else(|| invalid(path, "invalid xref array integer"))?;
                check_limit(
                    "xref entries",
                    array.len() as u64 / 2 + 1,
                    limits.max_xref_entries as u64,
                )?;
                crate::metadata_workspace::push(budget, &mut array, number)?;
            }
            if name_is(key, b"W") {
                dictionary.widths = Some(array);
            } else {
                dictionary.index = Some(array);
            }
        } else if name_is(key, b"DecodeParms") {
            dictionary.predictor =
                crate::indexed_predictor::Predictor::read(path, lexer, value, limits)?;
        } else if name_is(key, b"Filter") {
            let filter = if matches!(value, Token::Open(false)) {
                check_limit("container nesting", 2, limits.max_container_nesting as u64)?;
                let name = lexer.next();
                if !matches!(lexer.next(), Some(Token::Close(false))) {
                    return Err(invalid(path, "unsupported indexed stream filter chain"));
                }
                name
            } else {
                Some(value)
            };
            if !matches!(filter, Some(Token::Name(name)) if name_is(name, b"FlateDecode") || name_is(name, b"Fl"))
            {
                return Err(invalid(path, "unsupported indexed stream filter"));
            }
            dictionary.flate = true;
        } else if [b"Length".as_slice(), b"Size", b"Prev", b"N", b"First"]
            .iter()
            .any(|name| name_is(key, name))
        {
            let Token::Word(word) = value else {
                return Err(invalid(path, "invalid indexed stream integer"));
            };
            let number =
                integer(word).ok_or_else(|| invalid(path, "invalid indexed stream integer"))?;
            let mut lookahead = lexer.clone();
            if matches!(lookahead.next(), Some(Token::Word(word)) if integer(word).is_some())
                && matches!(lookahead.next(), Some(Token::Word(b"R")))
            {
                return Err(invalid(path, "indexed stream parameter must be direct"));
            }
            if name_is(key, b"Length") {
                dictionary.length = Some(number);
            } else if name_is(key, b"Size") {
                dictionary.size = Some(number);
            } else if name_is(key, b"Prev") {
                dictionary.prev = Some(number);
            } else if name_is(key, b"N") {
                dictionary.count = Some(number);
            } else {
                dictionary.first = Some(number);
            }
        } else if name_is(key, b"Encrypt") && !matches!(value, Token::Word(b"null")) {
            return Err(ParseError::UnsupportedEncryption { path: path.into() });
        } else {
            skip_value(path, lexer, value, limits)?;
        }
    }
}

/// 未使用の値を読み飛ばし、入れ子の key と最上位の key を混同しない。
fn skip_value(
    path: &Path,
    lexer: &mut Lexer<'_>,
    token: Token<'_>,
    limits: &ParseLimits,
) -> Result<(), ParseError> {
    if let Token::Open(dictionary) = token {
        let mut stack = vec![dictionary];
        check_limit("container nesting", 2, limits.max_container_nesting as u64)?;
        while let Some(token) = lexer.next() {
            match token {
                Token::Open(dictionary) => {
                    check_limit(
                        "container nesting",
                        stack.len() as u64 + 2,
                        limits.max_container_nesting as u64,
                    )?;
                    stack.push(dictionary);
                }
                Token::Close(dictionary) => {
                    if stack.pop() != Some(dictionary) {
                        return Err(invalid(path, "mismatched indexed dictionary container"));
                    }
                    if stack.is_empty() {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        return Err(invalid(path, "incomplete indexed dictionary container"));
    }
    if matches!(token, Token::Word(word) if integer(word).is_some()) {
        let mut lookahead = lexer.clone();
        if matches!(lookahead.next(), Some(Token::Word(word)) if integer(word).is_some())
            && matches!(lookahead.next(), Some(Token::Word(b"R")))
        {
            *lexer = lookahead;
        }
    }
    Ok(())
}

/// 必須の非負整数 token を読む。
fn next_integer(path: &Path, lexer: &mut Lexer<'_>) -> Result<usize, ParseError> {
    match lexer.next() {
        Some(Token::Word(word)) => {
            integer(word).ok_or_else(|| invalid(path, "invalid indexed object integer"))
        }
        _ => Err(invalid(path, "missing indexed object integer")),
    }
}

/// 構造不正を入力 path とともに返す。
fn invalid(path: &Path, reason: &'static str) -> ParseError {
    ParseError::InvalidPdfStructure {
        path: path.into(),
        reason,
    }
}
