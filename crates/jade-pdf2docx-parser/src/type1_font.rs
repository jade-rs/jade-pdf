//! 埋め込みType1の静的Encodingを有効世代の参照から読み、共有する。
use crate::{
    decode::DecodeBudget,
    metadata, parser,
    resource_resolver::ResourceResolver,
    syntax::{Lexer, Token},
    text::Type1Mapping,
};

/// Font辞書に組み込み表の適用が必要なら、完成表と未知codeを返す。
pub(crate) fn read<'bytes>(
    budget: &DecodeBudget,
    bytes: &[u8],
    font: &'bytes [u8],
    number: Option<u32>,
    resolver: &mut ResourceResolver<'_, 'bytes>,
    offsets: &parser::StreamOffsets,
) -> Option<Type1Mapping> {
    let subtype = metadata::value(font, b"Subtype")
        .and_then(|v| resolver.resolve(v))?
        .0;
    if !metadata::is_name(subtype, b"Type1") && !metadata::is_name(subtype, b"MMType1") {
        return None;
    }
    let raw_encoding = metadata::value(font, b"Encoding");
    let pdf_encoding = raw_encoding.and_then(|v| resolver.resolve(v)).map(|v| v.0);
    let unverified_encoding = raw_encoding.is_some() && pdf_encoding.is_none();
    // 既存の明示BaseEncoding経路はここで組み込み表に置き換えない。
    if pdf_encoding.is_some_and(|value| {
        matches!(
            Lexer {
                bytes: value,
                cursor: 0
            }
            .next(),
            Some(Token::Name(_))
        ) || metadata::value(value, b"BaseEncoding").is_some()
    }) {
        return None;
    }
    let descriptor = metadata::value(font, b"FontDescriptor")
        .and_then(|v| resolver.resolve(v))
        .map(|v| v.0);
    let raw_program = descriptor.and_then(|dictionary| metadata::value(dictionary, b"FontFile"));
    // FontDescriptor自体がない通常の非埋め込みFontと、明示した参照の失敗を区別する。
    if metadata::value(font, b"FontDescriptor").is_none()
        || descriptor.is_some() && raw_program.is_none()
    {
        return None;
    }
    let stream = raw_program.and_then(|v| resolver.resolve(v));
    let generation = number.and_then(|n| resolver.generation(n)).unwrap_or(0);
    if let Some(cached) = number.and_then(|n| budget.cached_type1(n, generation)) {
        return Some(cached);
    }
    let program = stream
        .and_then(|(_, number)| number)
        .and_then(|number| parser::form_stream_payload(budget, bytes, number, offsets));
    let length = stream
        .and_then(|(dictionary, _)| metadata::value(dictionary, b"Length1"))
        .and_then(|v| resolver.resolve(v))
        .and_then(|v| crate::syntax::integer(v.0));
    let cleartext = length.and_then(|length| program.as_ref()?.get(..length));
    let table = cleartext
        .filter(|_| !unverified_encoding)
        .and_then(|program| crate::type1_program::parse(budget, program));
    let mut differences = [None; 256];
    if let Some(raw) = pdf_encoding
        .and_then(|dictionary| metadata::value(dictionary, b"Differences"))
        .and_then(|v| resolver.resolve(v))
        .map(|v| v.0)
    {
        read_differences(raw, &mut differences);
    }
    let original = metadata::value(font, b"ToUnicode")
        .and_then(|v| resolver.resolve(v))
        .and_then(|v| v.1)
        .and_then(|n| parser::font_cmap_for_object_number(budget, bytes, n, offsets))
        .unwrap_or_default();
    let mapping = crate::text::type1_mapping(budget, table.as_ref(), &differences, &original)?;
    if let Some(number) = number {
        budget.save_type1(number, generation, mapping.clone())?;
    }
    Some(mapping)
}

/// Differencesの有効な単byte割当を、借用glyph名で保存する。
fn read_differences<'a>(raw: &'a [u8], slots: &mut [Option<&'a [u8]>; 256]) {
    let mut lexer = Lexer {
        bytes: raw,
        cursor: 0,
    };
    if !matches!(lexer.next(), Some(Token::Open(false))) {
        return;
    }
    let mut code = None;
    while let Some(token) = lexer.next() {
        match token {
            Token::Word(word) => code = crate::syntax::integer(word).filter(|n| *n < 256),
            Token::Name(name) => {
                if let Some(index) = code {
                    slots[index] = Some(name);
                    code = (index < 255).then_some(index + 1);
                }
            }
            Token::Close(false) => break,
            _ => {
                code = None;
            }
        }
    }
}
