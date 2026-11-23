//! 元FontFileの宣言された区間を使い、headerと全glyphの外形をまとめて読む。
use super::{charstring::Glyph, header::Header};
use crate::decode::DecodeBudget;

/// 元の座標系を保つType1 font。UnicodeとPDF幅の適用は後段で行う。
pub(super) struct Font<'a> {
    /// 元FontFileを借用するcleartext情報。
    pub(super) header: Header<'a>,
    /// .notdefを先頭にし、残りを名前順に並べた全glyph。
    pub(super) glyphs: Vec<NamedGlyph>,
}

/// 復号bufferの寿命から独立して保つglyph。
pub(super) struct NamedGlyph {
    /// 原本のglyph名。予算へ必要byteだけを予約する。
    pub(super) name: Vec<u8>,
    /// 元programのadvanceと外形。
    pub(super) outline: Glyph,
}

/// 未変更のFontFileを二層復号し、未対応glyphや停止時は部分fontを返さない。
pub(super) fn read<'a>(
    budget: &DecodeBudget,
    bytes: &'a [u8],
    length1: usize,
    length2: usize,
) -> Option<Font<'a>> {
    if bytes.len() > crate::parser::MAX_EMBEDDED_FONT_BYTES
        || length1 == 0
        || length2 < 4
        || budget.is_stopped()
    {
        return None;
    }
    let end = length1.checked_add(length2)?;
    let clear = bytes.get(..length1)?;
    let cipher = bytes.get(length1..end)?;
    let header = super::header::parse(budget, clear)?;
    let decrypted = super::crypt::decrypt(budget, cipher, 55665, 4)?;
    let mut private = super::private::parse(budget, &decrypted)?;
    let mut subrs = budget.font_vector(private.subrs.len())?;
    for slot in &private.subrs {
        let program = match slot {
            Some(bytes) => program(budget, bytes, private.prefix)?,
            None => Vec::new(),
        };
        subrs.push(program);
    }
    private.glyphs.sort_unstable_by(|a, b| {
        (a.name != b".notdef")
            .cmp(&(b.name != b".notdef"))
            .then_with(|| a.name.cmp(b.name))
    });
    let mut glyphs = budget.font_vector(private.glyphs.len())?;
    for entry in private.glyphs {
        if budget.is_stopped() {
            return None;
        }
        let bytes = program(budget, entry.bytes, private.prefix)?;
        let outline = match private.standard {
            Some(verified) => super::charstring::decode_verified(budget, &bytes, &subrs, verified)?,
            None => super::charstring::decode(budget, &bytes, &subrs)?,
        };
        let mut name = budget.font_vector(entry.name.len())?;
        name.extend_from_slice(entry.name);
        glyphs.push(NamedGlyph { name, outline });
    }
    if budget.is_stopped() {
        None
    } else {
        Some(Font { header, glyphs })
    }
}

/// lenIVが指定されなければ元byteを保ち、指定時はprefixを捨てて復号する。
fn program(budget: &DecodeBudget, bytes: &[u8], prefix: Option<usize>) -> Option<Vec<u8>> {
    match prefix {
        Some(skip) => super::crypt::decrypt(budget, bytes, 4330, skip),
        None => {
            let mut copy = budget.font_vector(bytes.len())?;
            copy.extend_from_slice(bytes);
            (!budget.is_stopped()).then_some(copy)
        }
    }
}

#[cfg(test)]
#[path = "type1_outline_file_tests.rs"]
mod tests;
