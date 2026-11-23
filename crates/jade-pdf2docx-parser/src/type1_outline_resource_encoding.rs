//! 元の静的EncodingとPDFのbase/Differencesから、確認したglyphだけを選ぶ。
use super::{DecodeBudget, Font, ResourceResolver, values};
use crate::{metadata, type1_program::Base};
#[path = "type1_outline_pdf_encodings.rs"]
mod predefined;
use crate::type1_standard_encoding as standard;

/// 既知のPDF encoding名を、OS側の文字コード表へ置き換えずに選ぶ。
fn predefined(name: &[u8]) -> Option<&'static [&'static [u8]; 256]> {
    match name {
        b"StandardEncoding" => Some(&standard::STANDARD),
        b"WinAnsiEncoding" => Some(&predefined::WIN_ANSI),
        b"MacRomanEncoding" => Some(&predefined::MAC_ROMAN),
        b"MacExpertEncoding" => Some(&predefined::MAC_EXPERT),
        _ => None,
    }
}

/// 元fontの名前を参照する256codeを、PDF側の宣言で上書きする。
pub(super) fn bind<'bytes>(
    budget: &DecodeBudget,
    font: &Font<'_>,
    encoding: Option<&'bytes [u8]>,
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<[Option<u16>; 256]> {
    if font.header.encoding.slots.len() != 256 || font.glyphs.is_empty() || font.glyphs.len() > 4096
    {
        return None;
    }
    let mut base = None;
    let mut differences = [None; 256];
    if let Some(encoding) = encoding {
        if let Some(name) = values::name(encoding) {
            base = Some(named_base(name)?);
        } else {
            if let Some(value) = values::field(budget, encoding, b"BaseEncoding", resolver)? {
                base = Some(named_base(values::name(value)?)?);
            }
            if let Some(array) = values::field(budget, encoding, b"Differences", resolver)? {
                let mut values = metadata::ArrayValues::new(array, 4096)?;
                let mut code = None;
                loop {
                    if budget.is_stopped() {
                        return None;
                    }
                    let raw = match values.next_value()? {
                        Some(raw) => raw,
                        None => break,
                    };
                    let (value, _) = resolver.resolve(raw)?;
                    if let Some(index) = values::code(value) {
                        code = Some(index);
                    } else {
                        let name = values::name(value)?;
                        let index = code?;
                        *differences.get_mut(index)? = Some(name);
                        code = Some(index + 1);
                    }
                }
            }
        }
    }
    let mut output = [None; 256];
    for code in 0..256 {
        if budget.is_stopped() {
            return None;
        }
        let (name, escaped) = if let Some(name) = differences[code] {
            (name, true)
        } else if let Some(base) = base {
            (base[code], false)
        } else {
            let name = match font.header.encoding.slots[code] {
                Some(name) => name,
                None => match font.header.encoding.base {
                    Base::Custom => b".notdef",
                    Base::Standard => standard::STANDARD[code],
                    Base::Latin1 => return None,
                },
            };
            (name, false)
        };
        output[code] = font
            .glyphs
            .iter()
            .position(|glyph| {
                if escaped {
                    crate::metadata_names::name_bytes(name).eq(glyph.name.iter().copied().map(Some))
                } else {
                    name == glyph.name
                }
            })
            .and_then(|index| u16::try_from(index).ok());
    }
    Some(output)
}

/// 固定されたencoding名と、PDF escapeを確保なしに照合する。
fn named_base(raw: &[u8]) -> Option<&'static [&'static [u8]; 256]> {
    for candidate in [
        b"StandardEncoding".as_slice(),
        b"WinAnsiEncoding",
        b"MacRomanEncoding",
        b"MacExpertEncoding",
    ] {
        if crate::metadata_names::name_bytes(raw).eq(candidate.iter().copied().map(Some)) {
            return predefined(candidate);
        }
    }
    None
}
