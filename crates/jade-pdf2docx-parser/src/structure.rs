use std::path::Path;

use crate::{
    ParseError, ParseLimits,
    limits::check_limit,
    syntax::{Lexer, Token, integer, name_is},
};

/// 構造走査で確認した stream の辞書と本文。入力 byte 列を借用する。
pub(crate) struct StreamSpan<'a> {
    /// 最外 dictionary の byte 列。
    pub(crate) dictionary: &'a [u8],
    /// 解決した Length に従う本文。未解決入力では従来の marker 境界を使用する。
    pub(crate) payload: &'a [u8],
    /// indirect object header の入力内位置。
    pub(crate) object_offset: Option<usize>,
    /// stream を所有する indirect object 番号。
    pub(crate) object_number: Option<u32>,
}

/// 軽量な object/暗号化検査を行い、stream 範囲を返す。xref 全体の整合性検査ではない。
pub(crate) fn validate_structure<'a>(
    path: &Path,
    bytes: &'a [u8],
    header_end: usize,
    limits: &ParseLimits,
    decode_budget: &crate::decode::DecodeBudget,
) -> Result<Vec<StreamSpan<'a>>, ParseError> {
    crate::metadata_workspace::ensure_active(decode_budget)?;
    let mut lexer = Lexer {
        bytes,
        cursor: header_end,
    };
    let mut frames: Vec<Frame> = Vec::new();
    let mut trailer = false;
    let mut integers = 0usize;
    let mut object_integers = [None; 2];
    let mut object_number = None;
    let mut object_generation = None;
    let mut object_offset = None;
    let mut integer_offsets = [None; 2];
    let mut in_object = false;
    let mut complete_objects = 0usize;
    let mut stream_length = None;
    let mut stream_dictionary = None;
    let mut streams = Vec::new();
    let mut xref = None;
    let mut xref_loaded = false;
    while let Some(token) = lexer.next() {
        if frames.is_empty() {
            match token {
                Token::Word(b"trailer") => trailer = true,
                Token::Word(b"obj") if integers == 2 => {
                    in_object = true;
                    object_offset = integer_offsets[0];
                    object_generation = object_integers[1];
                    object_number =
                        object_integers[0].and_then(|number| u32::try_from(number).ok());
                }
                Token::Word(b"endobj") if in_object => {
                    complete_objects += 1;
                    in_object = false;
                    object_number = None;
                    object_generation = None;
                    object_offset = None;
                    stream_dictionary = None;
                    stream_length = None;
                }
                Token::Word(b"stream") => {
                    let stream_length = stream_length.take();
                    let indirect = matches!(stream_length, Some(StreamLength::Reference { .. }));
                    let length = match stream_length {
                        Some(StreamLength::Direct(length)) => Some(length),
                        Some(StreamLength::Reference { object, generation }) => {
                            if !xref_loaded {
                                xref = crate::xref::XrefIndex::read(
                                    path,
                                    bytes,
                                    limits,
                                    decode_budget,
                                )?;
                                xref_loaded = true;
                            }
                            xref.as_ref()
                                .map(|index| {
                                    let revision = object_number
                                        .zip(object_generation)
                                        .zip(object_offset)
                                        .map_or(0, |((number, generation), offset)| {
                                            index.stream_revision(number, generation, offset)
                                        });
                                    index.length(
                                        path,
                                        bytes,
                                        (object, generation),
                                        revision,
                                        limits,
                                        decode_budget,
                                    )
                                })
                                .transpose()?
                        }
                        None => None,
                    };
                    let start = lexer.stream_data_start();
                    let exact = lexer.skip_stream(length);
                    if !exact && indirect && length.is_some() {
                        return Err(ParseError::InvalidPdfStructure {
                            path: path.into(),
                            reason: "stream Length does not match endstream boundary",
                        });
                    }
                    if let Some(dictionary) = stream_dictionary.take()
                        && lexer.cursor >= start + 9
                        && bytes[lexer.cursor - 9..lexer.cursor] == *b"endstream"
                    {
                        let mut end = if exact {
                            start + length.expect("validated stream length")
                        } else {
                            lexer.cursor - 9
                        };
                        if !exact {
                            while end > start && matches!(bytes[end - 1], b'\r' | b'\n') {
                                end -= 1;
                            }
                        }
                        crate::metadata_workspace::push(
                            decode_budget,
                            &mut streams,
                            StreamSpan {
                                dictionary,
                                payload: &bytes[start..end],
                                object_number,
                                object_offset,
                            },
                        )?;
                    }
                }
                _ => {}
            }
            integers = match token {
                Token::Word(word) if integer(word).is_some() => {
                    object_integers = [object_integers[1], integer(word)];
                    integer_offsets = [integer_offsets[1], Some(lexer.cursor - word.len())];
                    (integers + 1).min(2)
                }
                _ => {
                    object_integers = [None; 2];
                    integer_offsets = [None; 2];
                    0
                }
            };
        }
        if let Some(frame) = frames.last_mut() {
            frame.consume(token, &lexer);
        }
        match token {
            Token::Open(dictionary) => {
                check_limit(
                    "container nesting",
                    frames.len() as u64 + 1,
                    limits.max_container_nesting as u64,
                )?;
                let start = lexer.cursor - if dictionary { 2 } else { 1 };
                let frame = Frame::new(dictionary, frames.is_empty(), trailer, start);
                crate::metadata_workspace::push(decode_budget, &mut frames, frame)?;
                trailer = false;
            }
            Token::Close(dictionary) => {
                if let Some(frame) = frames.pop_if(|frame| frame.dictionary == dictionary)
                    && frame.top_level
                    && frame.dictionary
                {
                    if frame.encrypted && (frame.trailer || frame.xref) {
                        return Err(ParseError::UnsupportedEncryption { path: path.into() });
                    }
                    stream_length = frame.length;
                    stream_dictionary = Some(&bytes[frame.start..lexer.cursor]);
                }
            }
            _ => {}
        }
    }
    if complete_objects == 0 {
        return Err(ParseError::InvalidPdfStructure {
            path: path.into(),
            reason: "no complete indirect object",
        });
    }
    if !xref_loaded {
        xref = crate::xref::XrefIndex::read(path, bytes, limits, decode_budget)?;
    }
    decode_budget.cache_xref(xref);
    Ok(streams)
}

/// 検査に必要な dictionary key。
enum Key {
    /// 暗号化設定。
    Encrypt,
    /// object の種別。
    Type,
    /// stream の直接指定された長さ。
    Length,
    /// 検査対象外の key。
    Other,
}

/// 入れ子 dictionary/array の検査状態。
struct Frame {
    /// container の開始 byte 位置。
    start: usize,
    /// dictionary なら true、array なら false。
    dictionary: bool,
    /// 外側に container がない dictionary か。
    top_level: bool,
    /// trailer の dictionary か。
    trailer: bool,
    /// 次の値を待つ key。
    pending: Option<Key>,
    /// null 以外の暗号化設定があるか。
    encrypted: bool,
    /// xref stream dictionary か。
    xref: bool,
    /// stream の直接長または間接参照。
    length: Option<StreamLength>,
}

impl Frame {
    /// 空の container 状態を作る。
    fn new(dictionary: bool, top_level: bool, trailer: bool, start: usize) -> Self {
        Self {
            start,
            dictionary,
            top_level,
            trailer,
            pending: None,
            encrypted: false,
            xref: false,
            length: None,
        }
    }

    /// dictionary の key/value を消費し、入れ子 container 内の key と混同しない。
    fn consume(&mut self, token: Token<'_>, lexer: &Lexer<'_>) {
        if !self.dictionary || matches!(token, Token::Close(_)) {
            return;
        }
        if let Some(key) = self.pending.take() {
            match key {
                Key::Encrypt => self.encrypted = !matches!(token, Token::Word(b"null")),
                Key::Type => {
                    self.xref = matches!(token, Token::Name(name) if name_is(name, b"XRef"))
                }
                Key::Length => {
                    self.length = match token {
                        Token::Word(word) => {
                            integer(word).map(|value| StreamLength::read(value, lexer))
                        }
                        _ => None,
                    };
                }
                Key::Other => {}
            }
        } else if let Token::Name(name) = token {
            self.pending = Some(if name_is(name, b"Encrypt") {
                Key::Encrypt
            } else if name_is(name, b"Type") {
                Key::Type
            } else if name_is(name, b"Length") {
                Key::Length
            } else {
                Key::Other
            });
        }
    }
}

/// stream 長の直接指定または間接 object 参照。
enum StreamLength {
    /// 直接指定された byte 数。
    Direct(usize),
    /// xref で解決する object。
    Reference {
        /// object 番号。
        object: usize,
        /// generation 番号。
        generation: usize,
    },
}

impl StreamLength {
    /// 整数の直後が generation と R なら間接参照として扱う。
    fn read(value: usize, lexer: &Lexer<'_>) -> Self {
        let mut lookahead = lexer.clone();
        if let Some(Token::Word(word)) = lookahead.next()
            && let Some(generation) = integer(word)
            && matches!(lookahead.next(), Some(Token::Word(b"R")))
        {
            return Self::Reference {
                object: value,
                generation,
            };
        }
        Self::Direct(value)
    }
}

#[cfg(test)]
#[path = "structure_budget_tests.rs"]
mod budget_tests;
