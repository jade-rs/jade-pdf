//! 有効なRoot CatalogのVersionをheaderと比較し、文書の実効versionを求める。
use crate::{
    ParseError, ParseLimits, PdfVersion,
    decode::DecodeBudget,
    metadata,
    page_attributes::{self, Objects},
    syntax::{Lexer, Token},
};
use std::path::Path;

/// 間接値の予算・深度・循環を検査し、headerより上位のVersionだけを採用する。
pub(crate) fn read(
    path: &Path,
    objects: &Objects<'_>,
    catalog: &[u8],
    header: &PdfVersion,
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<PdfVersion, ParseError> {
    let Some(value) = metadata::value(catalog, b"Version") else {
        return Ok(header.clone());
    };
    let (value, _) = page_attributes::resolve_value(path, objects, value, limits, budget)?;
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    if matches!(lexer.next(), Some(Token::Word(b"null"))) && lexer.next().is_none() {
        return Ok(header.clone());
    }
    let catalog = parse(value).ok_or_else(|| ParseError::InvalidPdfStructure {
        path: path.to_path_buf(),
        reason: "Catalog Version is not a major.minor name",
    })?;
    Ok(
        if (catalog.major, catalog.minor) > (header.major, header.minor) {
            catalog
        } else {
            header.clone()
        },
    )
}

/// #xxを固定長で借用復号し、headerと同じ一桁のmajor.minor形式を読む。
fn parse(value: &[u8]) -> Option<PdfVersion> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    let Token::Name(name) = lexer.next()? else {
        return None;
    };
    if lexer.next().is_some() {
        return None;
    }
    let mut decoded = crate::metadata_names::name_bytes(name);
    let major = decoded.next()??;
    let dot = decoded.next()??;
    let minor = decoded.next()??;
    if !major.is_ascii_digit() || dot != b'.' || !minor.is_ascii_digit() || decoded.next().is_some()
    {
        return None;
    }
    Some(PdfVersion {
        major: major - b'0',
        minor: minor - b'0',
    })
}
