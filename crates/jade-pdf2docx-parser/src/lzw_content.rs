//! PDFのMSB-first LZW本文を、共通の展開量・確保・キャンセル予算へ接続する。
use crate::{
    decode::DecodeBudget,
    metadata,
    syntax::{Lexer, Token},
};
use std::io::{self, Read};

/// PDF LZWの最大12bit辞書長。入力に応じて拡張しない。
const CAPACITY: usize = 4096;

/// 先行codeと末尾byteで表す、一つの辞書entry。
#[derive(Clone, Copy)]
struct Entry {
    /// 既存の短い列を参照するcode。
    prefix: u16,
    /// 既存列へ追加する一byte。
    suffix: u8,
}

/// 一つのcode列だけを逆順に保持し、共通readerへ少量ずつ返す。
pub(crate) struct Decoder<'a> {
    /// コピーしない圧縮元本文。
    data: &'a [u8],
    /// 次に読むMSB-first bit位置。
    bit: usize,
    /// 現在のcode幅、9から12bit。
    width: u8,
    /// 幅変更を一code早めるか。PDFの既定値は1。
    early: usize,
    /// 次に割り当てる辞書code。
    next: usize,
    /// 直前に復元したcode。clear後は未指定。
    previous: Option<u16>,
    /// 共通確保予算へ予約済みの固定長辞書。
    table: Vec<Entry>,
    /// 共通確保予算へ予約済みの、未消費の逆順byte列。
    pending: Vec<u8>,
    /// EOD257を確認したか。
    finished: bool,
    /// 辞書を辿る間も確認する解析ローカルの停止状態。
    budget: &'a DecodeBudget,
}

impl<'a> Decoder<'a> {
    /// 二つの固定長作業配列を確保前に予約し、未対応parameterを拒否する。
    pub(crate) fn new(data: &'a [u8], early: u8, budget: &'a DecodeBudget) -> Option<Self> {
        if early > 1 {
            return None;
        }
        let mut table = budget.try_vector_capacity(CAPACITY)?;
        table.resize(
            CAPACITY,
            Entry {
                prefix: 0,
                suffix: 0,
            },
        );
        let pending = budget.try_vector_capacity(CAPACITY)?;
        Some(Self {
            data,
            bit: 0,
            width: 9,
            early: usize::from(early),
            next: 258,
            previous: None,
            table,
            pending,
            finished: false,
            budget,
        })
    }

    /// 一codeの有限なbitだけを読み、途中で切れた本文を検出する。
    fn code(&mut self) -> io::Result<u16> {
        let mut code = 0u16;
        for _ in 0..self.width {
            let byte = self
                .data
                .get(self.bit / 8)
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
            code = (code << 1) | u16::from((byte >> (7 - self.bit % 8)) & 1);
            self.bit = self.bit.checked_add(1).ok_or_else(invalid)?;
        }
        Ok(code)
    }

    /// clear・EOD・通常参照・KwKwKを処理し、一つの表示列を復元する。
    fn fill(&mut self) -> io::Result<()> {
        while self.pending.is_empty() && !self.finished {
            if self.budget.is_stopped() {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let code = self.code()?;
            if code == 256 {
                self.width = 9;
                self.next = 258;
                self.previous = None;
                continue;
            }
            if code == 257 {
                self.finished = true;
                return Ok(());
            }
            let special = usize::from(code) == self.next;
            if usize::from(code) > self.next || usize::from(code) >= CAPACITY {
                return Err(invalid());
            }
            let mut current = if special {
                self.previous.ok_or_else(invalid)?
            } else {
                code
            };
            while current >= 256 {
                if self.budget.is_stopped() {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                if usize::from(current) >= self.next || self.pending.len() >= CAPACITY - 1 {
                    return Err(invalid());
                }
                let entry = self.table[usize::from(current)];
                if entry.prefix >= current {
                    return Err(invalid());
                }
                self.pending.push(entry.suffix);
                current = entry.prefix;
            }
            self.pending.push(current as u8);
            let first = *self.pending.last().ok_or_else(invalid)?;
            if special {
                if self.pending.len() == CAPACITY {
                    return Err(invalid());
                }
                self.pending.insert(0, first);
            }
            if let Some(previous) = self.previous {
                if self.next < CAPACITY {
                    self.table[self.next] = Entry {
                        prefix: previous,
                        suffix: first,
                    };
                    self.next += 1;
                    if self.width < 12 && self.next + self.early == 1usize << self.width {
                        self.width += 1;
                    }
                }
            }
            self.previous = Some(code);
        }
        Ok(())
    }
}

impl Read for Decoder<'_> {
    /// 一つの辞書列を小分けに返し、展開bufferの上限は共通readerへ委ねる。
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        self.fill()?;
        let count = output.len().min(self.pending.len());
        for byte in &mut output[..count] {
            *byte = self.pending.pop().unwrap();
        }
        Ok(count)
    }
}

/// 未定義参照や循環に見えるcode列を、部分的な成功として返さない。
fn invalid() -> io::Error {
    io::ErrorKind::InvalidData.into()
}

/// 直接DecodeParmsのEarlyChange0/1を読み、未対応の予測変換を適用せず拒否する。
pub(crate) fn early_change(dictionary: &[u8]) -> Option<u8> {
    let Some(parameters) = metadata::value(dictionary, b"DecodeParms") else {
        return Some(1);
    };
    if parameters.trim_ascii() == b"null" {
        return Some(1);
    }
    metadata::dictionary_iter(parameters).next()?;
    if let Some(predictor) = metadata::value(parameters, b"Predictor") {
        if integer(predictor)? != 1 {
            return None;
        }
    }
    let early = match metadata::value(parameters, b"EarlyChange") {
        Some(value) => integer(value)?,
        None => 1,
    };
    (early <= 1).then_some(early)
}

/// 空白・コメントを除く単一の小整数だけを追加確保せずに読む。
fn integer(value: &[u8]) -> Option<u8> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    let Token::Word(word) = lexer.next()? else {
        return None;
    };
    let value = std::str::from_utf8(word).ok()?.parse().ok()?;
    lexer.next().is_none().then_some(value)
}
