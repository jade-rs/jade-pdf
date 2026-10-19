//! PDFのliteral/hex文字列をbyte単位で借用走査する。
use crate::syntax::{Lexer, Token};

/// 借用文字列を検証し、復号した各byteを確保なしで通知する。
pub(crate) fn visit_string(value: &[u8], mut emit: impl FnMut(u8) -> Option<()>) -> Option<()> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    let (Token::Other, span) = lexer.next_spanned()? else {
        return None;
    };
    if lexer.next().is_some() {
        return None;
    }
    let value = &value[span];
    if value.first() == Some(&b'<') && value.last() == Some(&b'>') {
        let mut high = None;
        for &byte in &value[1..value.len() - 1] {
            if matches!(byte, 0 | 9 | 10 | 12 | 13 | 32) {
                continue;
            }
            let nibble = hex_nibble(byte)?;
            if let Some(high) = high.take() {
                emit(high << 4 | nibble)?;
            } else {
                high = Some(nibble);
            }
        }
        if let Some(high) = high {
            emit(high << 4)?;
        }
        return Some(());
    }
    if value.first() != Some(&b'(') {
        return None;
    }
    let mut cursor = 1;
    let mut depth = 1usize;
    while let Some(&byte) = value.get(cursor) {
        cursor += 1;
        match byte {
            b'\\' => {
                let &escaped = value.get(cursor)?;
                cursor += 1;
                match escaped {
                    b'0'..=b'7' => {
                        let mut octal = u16::from(escaped - b'0');
                        for _ in 0..2 {
                            let Some(&next @ b'0'..=b'7') = value.get(cursor) else {
                                break;
                            };
                            octal = octal * 8 + u16::from(next - b'0');
                            cursor += 1;
                        }
                        emit(octal as u8)?;
                    }
                    b'\r' | b'\n' => {
                        if escaped == b'\r' && value.get(cursor) == Some(&b'\n') {
                            cursor += 1;
                        }
                    }
                    _ => emit(match escaped {
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'b' => 8,
                        b'f' => 12,
                        other => other,
                    })?,
                }
            }
            b'(' => {
                depth += 1;
                emit(byte)?;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return (cursor == value.len()).then_some(());
                }
                emit(byte)?;
            }
            b'\r' => {
                if value.get(cursor) == Some(&b'\n') {
                    cursor += 1;
                }
                emit(b'\n')?;
            }
            _ => emit(byte)?,
        }
    }
    None
}

/// hex 1 桁を nibble に変換する。
fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
