//! 元fontとPDF resourceのEncoding・Widthsを、文字codeごとに対応付ける。
use super::file::Font;
use crate::{decode::DecodeBudget, resource_resolver::ResourceResolver};
#[path = "type1_outline_resource_encoding.rs"]
mod encoding;
#[path = "type1_outline_resource_values.rs"]
mod values;

/// 一つのPDF文字codeに対応する元glyphと、PDFが宣言した幅。
#[derive(Clone, Copy)]
pub(super) struct Slot {
    /// 元fontのGID。Some(0)は.notdef、Noneは対応する元glyphが存在しない。
    pub(super) glyph: Option<u16>,
    /// 1000単位系の元PDF幅。未定義範囲はMissingWidth、未指定ならPDF既定の0。
    pub(super) width: f64,
    /// FirstCharからLastCharまでのWidthsに明示された値か。
    pub(super) explicit_width: bool,
}

/// PDFのEncoding上書きを優先し、小数幅をf32へ縮めずに原本の256codeを読む。
pub(super) fn read<'bytes>(
    budget: &DecodeBudget,
    font: &Font<'_>,
    dictionary: &'bytes [u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<Vec<Slot>> {
    let subtype = values::field(budget, dictionary, b"Subtype", resolver)??;
    if !crate::metadata::is_name(subtype, b"Type1") {
        return None;
    }
    let first = values::code(values::field(budget, dictionary, b"FirstChar", resolver)??)?;
    let last = values::code(values::field(budget, dictionary, b"LastChar", resolver)??)?;
    if first > last {
        return None;
    }
    let array = values::field(budget, dictionary, b"Widths", resolver)??;
    let count = last - first + 1;
    let mut values = crate::metadata::ArrayValues::new(array, count)?;
    let mut widths = [0.; 256];
    let descriptor = values::field(budget, dictionary, b"FontDescriptor", resolver)?;
    let missing = match descriptor {
        None => 0.,
        Some(descriptor) => match values::field(budget, descriptor, b"MissingWidth", resolver)? {
            Some(value) => values::number(value)?,
            None => 0.,
        },
    };
    widths.fill(missing);
    for width in &mut widths[first..=last] {
        if budget.is_stopped() {
            return None;
        }
        let raw = values.next_value()??;
        let (value, _) = resolver.resolve(raw)?;
        *width = values::number(value)?;
    }
    if values.next_value()?.is_some() {
        return None;
    }
    let encoding = values::field(budget, dictionary, b"Encoding", resolver)?;
    let glyphs = encoding::bind(budget, font, encoding, resolver)?;
    let mut slots = budget.font_vector(256)?;
    for code in 0..256 {
        slots.push(Slot {
            glyph: glyphs[code],
            width: widths[code],
            explicit_width: (first..=last).contains(&code),
        });
    }
    (!budget.is_stopped()).then_some(slots)
}

#[cfg(test)]
#[path = "type1_outline_resource_tests.rs"]
mod tests;
