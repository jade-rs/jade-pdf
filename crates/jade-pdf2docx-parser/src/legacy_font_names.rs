//! 非UTF-8の表示Font名を、同じ参照先のsfnt名前テーブルだけから復元する。
use crate::{decode::DecodeBudget, metadata, metadata_names, resource_resolver::ResourceResolver};

/// PDF name valueを借用し、escape・末尾・単一tokenを検査する。
pub(crate) fn raw_name(value: &[u8]) -> Option<&[u8]> {
    let mut lexer = crate::syntax::Lexer {
        bytes: value,
        cursor: 0,
    };
    let crate::syntax::Token::Name(raw) = lexer.next()? else {
        return None;
    };
    (lexer.next().is_none() && metadata_names::name_bytes(raw).all(|b| b.is_some())).then_some(raw)
}

/// 同じbyte名のFontとDescriptorだけを結び、有効な世代のFontFile2から表示名を得る。
pub(crate) fn for_font<'bytes>(
    budget: &DecodeBudget,
    dictionary: &'bytes [u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
    mut payload: impl FnMut(u32) -> Option<Vec<u8>>,
) -> Option<String> {
    if budget.is_stopped() {
        return None;
    }
    let (subtype, _) = resolver.resolve(metadata::value(dictionary, b"Subtype")?)?;
    let font = if metadata::is_name(subtype, b"Type0") {
        let (array, _) = resolver.resolve(metadata::value(dictionary, b"DescendantFonts")?)?;
        let mut entries = metadata::array_entries(array, 1)?;
        let value = entries.next()??;
        if entries.next().is_some() {
            return None;
        }
        let (descendant, _) = resolver.resolve(value)?;
        let (subtype, _) = resolver.resolve(metadata::value(descendant, b"Subtype")?)?;
        if !metadata::is_name(subtype, b"CIDFontType2") {
            return None;
        }
        descendant
    } else if metadata::is_name(subtype, b"TrueType") || metadata::is_name(subtype, b"CIDFontType2")
    {
        dictionary
    } else {
        return None;
    };
    let base = raw_name(metadata::value(font, b"BaseFont")?)?;
    let (descriptor, _) = resolver.resolve(metadata::value(font, b"FontDescriptor")?)?;
    let declared = raw_name(metadata::value(descriptor, b"FontName")?)?;
    if !metadata_names::same_resource_name(base, declared) {
        return None;
    }
    descriptor_payload(budget, descriptor, resolver, &mut payload).map(|(name, _)| name)
}

/// 元のname byteを推測復号せず、正しいFile参照と一致したPostScript名を保持する。
pub(crate) fn descriptor_payload<'bytes>(
    budget: &DecodeBudget,
    descriptor: &'bytes [u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
    mut payload: impl FnMut(u32) -> Option<Vec<u8>>,
) -> Option<(String, Vec<u8>)> {
    if budget.is_stopped() {
        return None;
    }
    let raw = raw_name(metadata::value(descriptor, b"FontName")?)?;
    let (_, number) = resolver.resolve(metadata::value(descriptor, b"FontFile2")?)?;
    let bytes = payload(number?)?;
    if bytes.len() > crate::parser::MAX_EMBEDDED_FONT_BYTES {
        return None;
    }
    let name = postscript_name(&bytes)?;
    let mut prefix = [0; 7];
    let mut prefix_length = 0;
    for (index, byte) in metadata_names::name_bytes(raw).take(7).enumerate() {
        prefix[index] = byte?;
        prefix_length += 1;
    }
    let prefix = if prefix_length == 7
        && prefix[..6].iter().all(u8::is_ascii_uppercase)
        && prefix[6] == b'+'
    {
        &prefix[..]
    } else {
        &[]
    };
    let mut output = budget.metadata_vector(prefix.len() + name.len)?;
    output.extend_from_slice(prefix);
    output.extend_from_slice(&name.bytes[..name.len]);
    Some((String::from_utf8(output).ok()?, bytes))
}

/// 名前テーブルの最大63byteのASCII PostScript名。本文の解釈にheapを使わない。
#[derive(PartialEq, Eq)]
struct Name {
    /// 初期化済みの固定容量。
    bytes: [u8; 63],
    /// 有効な文字数。
    len: usize,
}

/// 仕様に沿ったUnicode/Mac Romanのname ID6だけを採用し、不一致を拒否する。
fn postscript_name(bytes: &[u8]) -> Option<Name> {
    if !matches!(bytes.get(..4), Some(b"\0\x01\0\0" | b"true" | b"OTTO")) {
        return None;
    }
    let count = usize::from(word(bytes, 4)?);
    let directory_end = 12usize.checked_add(count.checked_mul(16)?)?;
    bytes.get(..directory_end)?;
    let mut table = None;
    for index in 0..count {
        let at = 12 + index * 16;
        if bytes.get(at..at + 4)? != b"name" {
            continue;
        }
        if table.is_some() {
            return None;
        }
        let offset = dword(bytes, at + 8)?;
        let length = dword(bytes, at + 12)?;
        if offset < directory_end {
            return None;
        }
        table = Some(bytes.get(offset..offset.checked_add(length)?)?);
    }
    let table = table?;
    if word(table, 0)? > 1 {
        return None;
    }
    let count = usize::from(word(table, 2)?);
    let strings = usize::from(word(table, 4)?);
    if strings < 6usize.checked_add(count.checked_mul(12)?)? {
        return None;
    }
    let mut selected = None;
    for index in 0..count {
        let at = 6 + index * 12;
        let platform = word(table, at)?;
        let encoding = word(table, at + 2)?;
        if word(table, at + 6)? != 6 {
            continue;
        }
        let unicode = match (platform, encoding) {
            (0, 0..=4) | (3, 1 | 10) => true,
            (1, 0) => false,
            _ => continue,
        };
        let length = usize::from(word(table, at + 8)?);
        let start = strings.checked_add(usize::from(word(table, at + 10)?))?;
        let value = table.get(start..start.checked_add(length)?)?;
        let stride = if unicode { 2 } else { 1 };
        if length == 0 || length % stride != 0 || length / stride > 63 {
            return None;
        }
        let mut name = Name {
            bytes: [0; 63],
            len: length / stride,
        };
        for (index, chunk) in value.chunks_exact(stride).enumerate() {
            if unicode && chunk[0] != 0 {
                return None;
            }
            let byte = chunk[stride - 1];
            if !(33..=126).contains(&byte) || b"[](){}<>/%".contains(&byte) {
                return None;
            }
            name.bytes[index] = byte;
        }
        if selected.as_ref().is_some_and(|prior| *prior != name) {
            return None;
        }
        selected = Some(name);
    }
    selected
}

/// 境界検査した16bit big-endian値を読む。
fn word(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// 境界検査した32bit offsetを読む。
fn dword(bytes: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_be_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
    .ok()
}

#[cfg(test)]
#[path = "legacy_font_names_tests.rs"]
mod tests;
