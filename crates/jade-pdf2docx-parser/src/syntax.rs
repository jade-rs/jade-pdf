/// stream 本文・コメント・文字列を除外するための最小字句。
#[derive(Clone, Copy)]
pub(crate) enum Token<'a> {
    /// PDF 名の未展開 byte 列。
    Name(&'a [u8]),
    /// 数字または keyword。
    Word(&'a [u8]),
    /// dictionary（true）または array（false）の開始。
    Open(bool),
    /// dictionary（true）または array（false）の終了。
    Close(bool),
    /// 内容を読み飛ばした文字列など。
    Other,
}

/// バイト列を追加コピーせずに読む字句走査器。
#[derive(Clone)]
pub(crate) struct Lexer<'a> {
    /// 解析対象。
    pub(crate) bytes: &'a [u8],
    /// 次に読む位置。
    pub(crate) cursor: usize,
}

impl<'a> Lexer<'a> {
    /// コメントと空白を除き、次の字句を読む。
    pub(crate) fn next(&mut self) -> Option<Token<'a>> {
        self.skip_trivia();
        let start = self.cursor;
        self.bytes.get(start)?;
        self.cursor += 1;
        match self.bytes[start] {
            b'(' => {
                let mut depth = 1usize;
                while let Some(&byte) = self.bytes.get(self.cursor) {
                    self.cursor += 1;
                    match byte {
                        b'\\' => self.cursor = (self.cursor + 1).min(self.bytes.len()),
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                Some(Token::Other)
            }
            b'<' if self.bytes.get(self.cursor) == Some(&b'<') => {
                self.cursor += 1;
                Some(Token::Open(true))
            }
            b'>' if self.bytes.get(self.cursor) == Some(&b'>') => {
                self.cursor += 1;
                Some(Token::Close(true))
            }
            b'<' => {
                while self.bytes.get(self.cursor).is_some_and(|b| *b != b'>') {
                    self.cursor += 1;
                }
                self.cursor = (self.cursor + 1).min(self.bytes.len());
                Some(Token::Other)
            }
            b'[' => Some(Token::Open(false)),
            b']' => Some(Token::Close(false)),
            b'/' => {
                while self.bytes.get(self.cursor).is_some_and(|b| !delimiter(*b)) {
                    self.cursor += 1;
                }
                Some(Token::Name(&self.bytes[start + 1..self.cursor]))
            }
            byte if delimiter(byte) => Some(Token::Other),
            _ => {
                while self.bytes.get(self.cursor).is_some_and(|b| !delimiter(*b)) {
                    self.cursor += 1;
                }
                Some(Token::Word(&self.bytes[start..self.cursor]))
            }
        }
    }

    /// 空白・コメントを含まない字句と、その元入力内の範囲を返す。
    pub(crate) fn next_spanned(&mut self) -> Option<(Token<'a>, std::ops::Range<usize>)> {
        self.skip_trivia();
        let start = self.cursor;
        let token = self.next()?;
        Some((token, start..self.cursor))
    }

    /// 字句前の空白とコメントを読み飛ばす。
    fn skip_trivia(&mut self) {
        while let Some(&byte) = self.bytes.get(self.cursor) {
            if whitespace(byte) {
                self.cursor += 1;
            } else if byte == b'%' {
                while self
                    .bytes
                    .get(self.cursor)
                    .is_some_and(|b| !matches!(b, b'\r' | b'\n'))
                {
                    self.cursor += 1;
                }
            } else {
                break;
            }
        }
    }

    /// stream keyword に続く CR/LF を除いた本文開始位置を返す。
    pub(crate) fn stream_data_start(&self) -> usize {
        let mut start = self.cursor;
        if self.bytes.get(start) == Some(&b'\r') {
            start += 1;
        }
        if self.bytes.get(start) == Some(&b'\n') {
            start += 1;
        }
        start
    }

    /// length があればそれを使い、正しい境界を確認できたか返す。未解決なら marker を探す。
    pub(crate) fn skip_stream(&mut self, length: Option<usize>) -> bool {
        self.cursor = self.stream_data_start();
        if let Some(end) = length
            .and_then(|n| self.cursor.checked_add(n))
            .filter(|end| *end <= self.bytes.len())
        {
            let mut marker = end;
            while self.bytes.get(marker).is_some_and(|b| whitespace(*b)) {
                marker += 1;
            }
            if self.bytes[marker..].starts_with(b"endstream")
                && self
                    .bytes
                    .get(marker + 9)
                    .is_none_or(|byte| delimiter(*byte))
            {
                self.cursor = marker + b"endstream".len();
                return true;
            }
        }
        self.cursor = self.bytes[self.cursor..]
            .windows(9)
            .position(|w| w == b"endstream")
            .map_or(self.bytes.len(), |offset| self.cursor + offset + 9);
        false
    }
}

/// PDF の空白（NUL を含む）かを判定する。
pub(crate) fn whitespace(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 12 | b'\r' | b' ')
}

/// PDF token を区切る byte かを判定する。
fn delimiter(byte: u8) -> bool {
    whitespace(byte) || b"()<>[]{}/%".contains(&byte)
}

/// 非負整数を追加 allocation なしで読む。
pub(crate) fn integer(word: &[u8]) -> Option<usize> {
    if word.is_empty() || !word.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(word).ok()?.parse().ok()
}

/// #xx escape を復元しながら PDF 名を比較する。
pub(crate) fn name_is(raw: &[u8], expected: &[u8]) -> bool {
    let mut cursor = 0;
    for &byte in expected {
        let Some(&actual) = raw.get(cursor) else {
            return false;
        };
        let decoded = if actual == b'#' {
            let Some(hex) = raw.get(cursor + 1..cursor + 3) else {
                return false;
            };
            let Ok(hex) = std::str::from_utf8(hex) else {
                return false;
            };
            let Ok(value) = u8::from_str_radix(hex, 16) else {
                return false;
            };
            cursor += 3;
            value
        } else {
            cursor += 1;
            actual
        };
        if decoded != byte {
            return false;
        }
    }
    cursor == raw.len()
}
