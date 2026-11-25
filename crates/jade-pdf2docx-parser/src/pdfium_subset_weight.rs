//! SDK専用の識別候補を、元のsubset Type1/CFF辞書から保守的に確認する。
use crate::{decode::DecodeBudget, metadata, resource_resolver::ResourceResolver, syntax};

#[cfg(test)]
#[path = "pdfium_subset_weight_tests.rs"]
mod tests;

/// 辞書から確認した候補。実際の採用にはSDKの埋め込み確認を別途必要とする。
pub(crate) struct Candidate<'a> {
    /// 埋め込みが成立した場合にSDKが返す、subset prefixを除いたASCII名。
    pub(crate) native_name: &'a str,
    /// 明示された整数のFontDescriptor weight。
    pub(crate) weight: u16,
}

/// Optionを返す内部処理では、元の失敗を消費せず停止だけを確認する。
fn active(budget: &DecodeBudget) -> Option<()> {
    (!budget.is_stopped()).then_some(())
}

/// 完全な辞書の直下で、一度だけ現れた属性を借用する。
fn unique<'a>(dictionary: &'a [u8], key: &[u8], budget: &DecodeBudget) -> Option<&'a [u8]> {
    if !metadata::dictionary_entries_complete(dictionary) {
        return None;
    }
    let mut result = None;
    for (name, value) in metadata::entries(dictionary)? {
        active(budget)?;
        if syntax::name_is(name, key) {
            if result.is_some() {
                return None;
            }
            result = Some(value);
        }
    }
    result
}

/// 一意な属性の直接値と間接値を、既存resolverで同じ元のbyte列へ解決する。
fn field<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    dictionary: &'a [u8],
    key: &[u8],
    budget: &DecodeBudget,
) -> Option<&'a [u8]> {
    resolver
        .resolve(unique(dictionary, key, budget)?)
        .map(|(v, _)| v)
}

/// 標準の6大文字subset prefixと未escapeのASCII名だけを借用し、特殊encoding名を除く。
fn native_name<'a>(name: &'a [u8], budget: &DecodeBudget) -> Option<&'a str> {
    let name = name.strip_prefix(b"/")?;
    if name.len() < 8 || !name[..6].iter().all(u8::is_ascii_uppercase) || name[6] != b'+' {
        return None;
    }
    for (index, byte) in name.iter().copied().enumerate() {
        if index % 256 == 0 {
            active(budget)?;
        }
        if !byte.is_ascii_alphanumeric() && !matches!(byte, b'+' | b'-' | b'_' | b'.') {
            return None;
        }
    }
    let native = std::str::from_utf8(&name[7..]).ok()?;
    if native.eq_ignore_ascii_case("Symbol") || native.eq_ignore_ascii_case("ZapfDingbats") {
        return None;
    }
    Some(native)
}

/// stream keywordを文字列やPDF名から作らず、最初の辞書とEOLを確認する。
#[cfg(test)]
fn stream_header(body: &[u8]) -> Option<&[u8]> {
    let mut lexer = syntax::Lexer {
        bytes: body,
        cursor: 0,
    };
    let (token, span) = lexer.next_spanned()?;
    if !matches!(token, syntax::Token::Open(true)) {
        return None;
    }
    let header = metadata::dictionary_iter(&body[span.start..]).next()?;
    let after = body.get(span.start + header.len()..)?;
    let mut lexer = syntax::Lexer {
        bytes: after,
        cursor: 0,
    };
    let (token, span) = lexer.next_spanned()?;
    if !matches!(token, syntax::Token::Word(b"stream")) {
        return None;
    }
    let suffix = after.get(span.end..)?;
    (suffix.starts_with(b"\n") || suffix.starts_with(b"\r\n")).then_some(header)
}

/// sourceの一意なsubset名・descriptor・Type1C streamだけを候補として読む。
#[cfg(test)]
pub(crate) fn read<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    font: &'a [u8],
    budget: &DecodeBudget,
) -> Option<Candidate<'a>> {
    read_with_header(resolver, font, budget, |_, body| stream_header(body))
}

/// 合成metadataで失われるEOLを推測せず、同じ原本の検査済みstream辞書を使う。
pub(crate) fn read_indexed<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    font: &'a [u8],
    budget: &DecodeBudget,
    bytes: &'a [u8],
    index: &crate::stream_index::StreamIndex,
) -> Option<Candidate<'a>> {
    if !index.matches(bytes) {
        return None;
    }
    read_with_header(resolver, font, budget, |number, _| {
        index.for_object(bytes, number?).map(|(header, _)| header)
    })
}

/// 辞書の条件を共有し、streamの根拠だけを呼出し側の検査方法から受け取る。
fn read_with_header<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    font: &'a [u8],
    budget: &DecodeBudget,
    header_for: impl FnOnce(Option<u32>, &'a [u8]) -> Option<&'a [u8]>,
) -> Option<Candidate<'a>> {
    active(budget)?;
    if !metadata::is_name(field(resolver, font, b"Type", budget)?, b"Font")
        || !metadata::is_name(field(resolver, font, b"Subtype", budget)?, b"Type1")
    {
        return None;
    }
    let name = field(resolver, font, b"BaseFont", budget)?.trim_ascii();
    let native_name = native_name(name, budget)?;
    let descriptor = field(resolver, font, b"FontDescriptor", budget)?;
    if field(resolver, descriptor, b"FontName", budget)?.trim_ascii() != name {
        return None;
    }
    for (name, _) in metadata::entries(descriptor)? {
        active(budget)?;
        if syntax::name_is(name, b"FontFile") || syntax::name_is(name, b"FontFile2") {
            return None;
        }
    }
    let flags = field(resolver, descriptor, b"Flags", budget)?;
    let flags = std::str::from_utf8(flags.trim_ascii())
        .ok()?
        .parse::<u32>()
        .ok()?;
    if flags & 32 == 0 || flags & 4 != 0 {
        return None;
    }
    let weight = crate::font_weight::read(resolver, font, false, budget)?;
    let (file, number) = resolver.resolve(unique(descriptor, b"FontFile3", budget)?)?;
    let header = header_for(number, file)?;
    if !metadata::is_name(field(resolver, header, b"Subtype", budget)?, b"Type1C") {
        return None;
    }
    let length = field(resolver, header, b"Length", budget)?;
    let length = std::str::from_utf8(length.trim_ascii())
        .ok()?
        .parse::<u64>()
        .ok()?;
    active(budget)?;
    (length > 0).then_some(Candidate {
        native_name,
        weight,
    })
}
