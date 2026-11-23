//! CFF DICTの固定容量writer。offsetは一定長とし、配置計算を循環させない。
use super::Font;
use std::fmt::{self, Write};

/// 全glyphで共有するnominalWidthX=32768。他のPrivate項目は既定値を使う。
pub(super) const PRIVATE: &[u8] = &[29, 0, 0, 128, 0, 21];

/// Top DICT専用の固定長buffer。可変長のnoticeはSIDで参照する。
pub(super) struct Dict {
    /// 動的確保しない出力本体。
    bytes: [u8; 256],
    /// 書込み済みbyte数。
    len: usize,
}
impl Dict {
    /// 初期状態の辞書を作る。
    fn new() -> Self {
        Self {
            bytes: [0; 256],
            len: 0,
        }
    }
    /// 容量を先に検査してbyte列を追加する。
    fn push(&mut self, bytes: &[u8]) -> Option<()> {
        let end = self.len.checked_add(bytes.len())?;
        self.bytes.get_mut(self.len..end)?.copy_from_slice(bytes);
        self.len = end;
        Some(())
    }
    /// CFF DICTの32bit整数表現は、Type2の16.16と区別する。
    fn integer(&mut self, value: i32) -> Option<()> {
        self.push(&[29])?;
        self.push(&value.to_be_bytes())
    }
    /// 絶対offsetも32bitの符号付き上限内で格納する。
    fn offset(&mut self, value: usize) -> Option<()> {
        self.integer(i32::try_from(value).ok()?)
    }
    /// 完成した辞書だけを借用する。
    pub(super) fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
    /// 有限小数を固定容量で指数表記へ変え、DICTのnibble列で格納する。
    fn real(&mut self, value: f64) -> Option<()> {
        if !value.is_finite() {
            return None;
        }
        let mut text = Text {
            bytes: [0; 64],
            len: 0,
        };
        write!(&mut text, "{value:e}").ok()?;
        let mut nibbles = [15_u8; 66];
        let mut count = 0;
        let mut index = 0;
        while index < text.len {
            let nibble = match text.bytes[index] {
                b'0'..=b'9' => text.bytes[index] - b'0',
                b'.' => 10,
                b'-' => 14,
                b'e' | b'E' => {
                    if text.bytes.get(index + 1) == Some(&b'-') {
                        index += 1;
                        12
                    } else {
                        if text.bytes.get(index + 1) == Some(&b'+') {
                            index += 1;
                        }
                        11
                    }
                }
                _ => return None,
            };
            *nibbles.get_mut(count)? = nibble;
            count += 1;
            index += 1;
        }
        count += 1; // 末尾の15は数値の終了。半byteの余りも15で埋める。
        self.push(&[30])?;
        for pair in nibbles[..count.next_multiple_of(2)].chunks_exact(2) {
            self.push(&[pair[0] * 16 + pair[1]])?;
        }
        Some(())
    }
}

/// 浮動小数点の表示でもheap上の一時Stringを作らない。
struct Text {
    /// 指数表記の最大長を収める固定本体。
    bytes: [u8; 64],
    /// 書込み済みの長さ。
    len: usize,
}
impl Write for Text {
    /// formatterから受け取る文字列も、容量確認後だけコピーする。
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        self.bytes
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// 大きさがoffsetの値に依存しないTop DICTを作る。
pub(super) fn top(
    font: &Font<'_>,
    notice: u16,
    name: u16,
    charset: usize,
    charstrings: usize,
    private: usize,
) -> Option<Dict> {
    let mut dict = Dict::new();
    for value in font.bbox {
        dict.integer(i32::from(value))?;
    }
    dict.push(&[5])?;
    dict.integer(i32::from(notice))?;
    dict.push(&[1])?;
    for op in [2, 3] {
        dict.integer(i32::from(name))?;
        dict.push(&[op])?;
    }
    dict.integer(i32::from(font.is_fixed_pitch))?;
    dict.push(&[12, 1])?;
    dict.real(font.italic_angle)?;
    dict.push(&[12, 2])?;
    dict.offset(charset)?;
    dict.push(&[15])?;
    dict.offset(charstrings)?;
    dict.push(&[17])?;
    dict.offset(PRIVATE.len())?;
    dict.offset(private)?;
    dict.push(&[18])?;
    Some(dict)
}
