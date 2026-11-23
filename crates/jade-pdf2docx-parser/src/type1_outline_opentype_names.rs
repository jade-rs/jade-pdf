//! name tableをUTF16BEで作り、元noticeとlicenseをplatform間で共有する。
use super::{Info, cff};
use crate::decode::DecodeBudget;

/// source Type1のbyte列と、追加のUnicode許諾文を区別して格納する。
#[derive(Clone, Copy)]
enum Text<'a> {
    /// Type1 noticeのbyte値を失わずUnicodeへ対応させる。
    Latin1(&'a [u8]),
    /// 呼出元が確認したUnicode文字列。
    Unicode(&'a str),
}

impl Text<'_> {
    /// 一時Stringなしで、UTF16の個数計測と書込みに同じ順序を使う。
    fn units(self, mut emit: impl FnMut(u16) -> Option<()>) -> Option<()> {
        match self {
            Self::Latin1(bytes) => {
                for byte in bytes {
                    emit(u16::from(*byte))?;
                }
            }
            Self::Unicode(text) => {
                for value in text.encode_utf16() {
                    emit(value)?;
                }
            }
        }
        Some(())
    }
}

/// Unicode/Windows name recordを共有し、長さとoffsetを検査してから確保する。
pub(super) fn build(
    budget: &DecodeBudget,
    font: &cff::Font<'_>,
    info: &Info<'_>,
) -> Option<Vec<u8>> {
    let style = match (info.bold, info.italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        _ => "Regular",
    };
    let names = [
        (0, Text::Latin1(font.notice)),
        (1, Text::Latin1(font.name)),
        (2, Text::Unicode(style)),
        (3, Text::Latin1(font.name)),
        (4, Text::Latin1(font.name)),
        (5, Text::Unicode("Version 1.000; Jade PDF")),
        (6, Text::Latin1(font.name)),
        (13, Text::Unicode(info.license)),
        (14, Text::Unicode(info.license_url)),
        (16, Text::Latin1(font.name)),
        (17, Text::Unicode(style)),
    ];
    let mut lengths = [0_u16; 11];
    let mut offsets = [0_u16; 11];
    let mut length = 0_usize;
    for (index, (_, text)) in names.iter().enumerate() {
        if budget.is_stopped() {
            return None;
        }
        offsets[index] = u16::try_from(length).ok()?;
        let mut bytes = 0_usize;
        text.units(|_| {
            bytes = bytes.checked_add(2)?;
            Some(())
        })?;
        lengths[index] = u16::try_from(bytes).ok()?;
        length = length.checked_add(bytes)?;
        if length > 65535 {
            return None;
        }
    }
    let storage = 6 + 24 * names.len();
    let mut output = budget.font_vector(storage.checked_add(length)?)?;
    for value in [0, 22, u16::try_from(storage).ok()?] {
        output.extend_from_slice(&value.to_be_bytes());
    }
    for (platform, encoding, language) in [(0, 4, 0), (3, 1, 0x409)] {
        for (index, (id, _)) in names.iter().enumerate() {
            if budget.is_stopped() {
                return None;
            }
            for value in [
                platform,
                encoding,
                language,
                *id,
                lengths[index],
                offsets[index],
            ] {
                output.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
    for (_, text) in names {
        text.units(|unit| {
            if budget.is_stopped() {
                return None;
            }
            output.extend_from_slice(&unit.to_be_bytes());
            Some(())
        })?;
    }
    (!budget.is_stopped()).then_some(output)
}
