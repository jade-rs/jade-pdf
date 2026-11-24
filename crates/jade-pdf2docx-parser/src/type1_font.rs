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
    let generation = number.and_then(|n| resolver.generation(n)).unwrap_or(0);
    if let Some(cached) = number.and_then(|n| budget.cached_type1(n, generation)) {
        return Some(cached);
    }
    if let Some(mapping) = crate::named_font_encoding::read(budget, font, resolver) {
        if let Some(number) = number {
            budget.save_type1(number, generation, mapping.clone())?;
        }
        return Some(mapping);
    }
    let raw_encoding = metadata::value(font, b"Encoding");
    let pdf_encoding = raw_encoding
        .and_then(|v| resolver.resolve(v))
        .map(|v| v.0)
        .filter(|value| value.trim_ascii() != b"null");
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
        .map(|v| v.0)
        .filter(|value| metadata::entries(value).is_some());
    let raw_program = descriptor.and_then(|dictionary| metadata::value(dictionary, b"FontFile"));
    // 標準14書体以外の組み込み表を、欠落したfont programから推測しない。
    if raw_program.is_none() && standard_font(font, resolver) {
        return None;
    }
    let stream = raw_program.and_then(|v| resolver.resolve(v));
    let program = stream
        .and_then(|(_, number)| number)
        .and_then(|number| parser::form_stream_payload(budget, bytes, number, offsets));
    let length = stream
        .and_then(|(dictionary, _)| metadata::value(dictionary, b"Length1"))
        .and_then(|v| resolver.resolve(v))
        .and_then(|v| crate::syntax::integer(v.0));
    let cleartext = length.and_then(|length| program.as_ref()?.get(..length));
    let table = cleartext.and_then(|program| crate::type1_program::parse(budget, program));
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
    let mapping = if table.is_none() && differences.iter().all(Option::is_none) {
        Type1Mapping::unverified(budget, &original)?
    } else {
        crate::text::type1_mapping(budget, table.as_ref(), &differences, &original)?
    };
    if let Some(number) = number {
        budget.save_type1(number, generation, mapping.clone())?;
    }
    Some(mapping)
}

/// 標準14書体の非埋め込み経路を、任意の独自font名へのASCII推測から区別する。
fn standard_font<'bytes>(font: &'bytes [u8], resolver: &mut ResourceResolver<'_, 'bytes>) -> bool {
    let Some((base, _)) =
        metadata::value(font, b"BaseFont").and_then(|value| resolver.resolve(value))
    else {
        return false;
    };
    [
        b"Courier".as_slice(),
        b"Courier-Bold",
        b"Courier-Oblique",
        b"Courier-BoldOblique",
        b"Helvetica",
        b"Helvetica-Bold",
        b"Helvetica-Oblique",
        b"Helvetica-BoldOblique",
        b"Times-Roman",
        b"Times-Bold",
        b"Times-Italic",
        b"Times-BoldItalic",
        b"Symbol",
        b"ZapfDingbats",
    ]
    .iter()
    .any(|name| metadata::is_name(base, name))
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
