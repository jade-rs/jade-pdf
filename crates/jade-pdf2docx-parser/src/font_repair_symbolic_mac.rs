//! 単一Mac cmap・symbolic PDF fontの対応だけを、明示ToUnicodeで合成する。
use super::*;
use crate::{metadata, syntax, text::ToUnicodeCMap};

/// 未確認encodingや共有descriptorを除き、原本字形と字幅を検証してcmapだけを復元する。
pub(crate) fn repair(
    budget: &DecodeBudget,
    source: &[u8],
    descriptor: u32,
    bytes: &[u8],
    mapping: &ToUnicodeCMap,
) -> Option<Vec<u8>> {
    if budget.is_stopped() || !mapping.is_complete_byte_bmp_mapping() {
        return None;
    }
    let font = context(budget, source, descriptor)?;
    let codes = mac_codes(bytes)?;
    widths_agree(font, bytes, &codes, mapping)?;
    let mut glyphs = budget.font_vector::<Glyph>(mapping.bmp_scalar_iter().count())?;
    for (source, unicode) in mapping.bmp_scalar_iter() {
        let id = codes.glyph(source)?;
        if glyphs
            .iter()
            .any(|glyph| glyph.unicode == unicode && glyph.id != id)
        {
            return None;
        }
        glyphs.push(Glyph {
            unicode,
            id,
            order: 0,
        });
    }
    glyphs.sort_unstable_by_key(|glyph| glyph.unicode);
    glyphs.dedup_by_key(|glyph| glyph.unicode);
    glyphs.retain(|glyph| glyph.id != 0);
    if glyphs.is_empty() || budget.is_stopped() {
        return None;
    }
    let cmap = build_unicode_bmp_cmap(budget, &glyphs)?;
    let output = rebuild_sfnt_with_table(budget, bytes, *b"cmap", &cmap)?;
    crate::font_validation::subset_unicode_cmap_is_complete(&output).then_some(output)
}

/// 単一世代0のTrueType resourceとsymbolic descriptorだけを結び付ける。
fn context<'a>(budget: &DecodeBudget, source: &'a [u8], descriptor: u32) -> Option<&'a [u8]> {
    let mut found_descriptor = false;
    let mut font = None;
    for object in metadata::objects(source, None) {
        if budget.is_stopped() {
            return None;
        }
        if object.number == descriptor {
            if found_descriptor
                || object.generation != 0
                || !unique(object.body, &[b"Type", b"Flags", b"FontFile2", b"FontName"])
                || !metadata::has_type(object.body, b"FontDescriptor")
            {
                return None;
            }
            let flags = metadata::value(object.body, b"Flags").and_then(syntax::integer)?;
            let (file, generation) = metadata::value(object.body, b"FontFile2")
                .and_then(metadata::reference_with_generation)?;
            if flags & 36 != 4 || file == 0 || generation != 0 {
                return None;
            }
            found_descriptor = true;
        }
        let reference = metadata::value(object.body, b"FontDescriptor")
            .and_then(metadata::reference_with_generation);
        if reference.is_some_and(|(number, _)| number == descriptor) {
            if font.is_some()
                || object.generation != 0
                || reference != Some((descriptor, 0))
                || !unique(
                    object.body,
                    &[
                        b"Type",
                        b"Subtype",
                        b"FontDescriptor",
                        b"Encoding",
                        b"ToUnicode",
                        b"FirstChar",
                        b"LastChar",
                        b"Widths",
                    ],
                )
                || !metadata::has_type(object.body, b"Font")
                || !name_value(object.body, b"Subtype", b"TrueType")
                || metadata::value(object.body, b"Encoding").is_some()
            {
                return None;
            }
            let (cmap, generation) = metadata::value(object.body, b"ToUnicode")
                .and_then(metadata::reference_with_generation)?;
            if cmap == 0 || generation != 0 {
                return None;
            }
            font = Some(object.body);
        }
    }
    found_descriptor.then_some(font).flatten()
}

/// 完成した辞書の重要属性に重複を許さない。
fn unique(dictionary: &[u8], keys: &[&[u8]]) -> bool {
    metadata::dictionary_entries_complete(dictionary)
        && keys.iter().all(|key| {
            metadata::entries(dictionary).is_some_and(|entries| {
                entries
                    .filter(|(name, _)| syntax::name_is(name, key))
                    .count()
                    <= 1
            })
        })
}

/// 値全体が指定nameであることを、escapeを解釈して確認する。
fn name_value(dictionary: &[u8], key: &[u8], expected: &[u8]) -> bool {
    metadata::value(dictionary, key)
        .and_then(|raw| raw.strip_prefix(b"/"))
        .is_some_and(|name| syntax::name_is(name, expected))
}

/// 境界・重複・checksumを検証した単一Mac Roman format6だけを採用する。
fn mac_codes(bytes: &[u8]) -> Option<Format6<'_>> {
    if bytes.get(..4)? != [0, 1, 0, 0] || sfnt_checksum(bytes) != 0xb1b0afba {
        return None;
    }
    let count = usize::from(read_be_u16(bytes, 4)?);
    if !(1..=128).contains(&count) {
        return None;
    }
    let directory_end = 12 + count * 16;
    let mut previous_tag = None;
    for index in 0..count {
        let entry = 12 + index * 16;
        let tag = bytes.get(entry..entry + 4)?;
        // Unicodeを組み直すと別の字形・色・字幅を選び得る表や、無効になる署名は扱わない。
        if [
            b"GSUB", b"GPOS", b"GDEF", b"kern", b"kerx", b"morx", b"mort", b"feat", b"fvar",
            b"gvar", b"HVAR", b"MVAR", b"VVAR", b"avar", b"cvar", b"DSIG", b"CFF ", b"CFF2",
            b"COLR", b"CPAL", b"CBDT", b"CBLC", b"EBDT", b"EBLC", b"sbix", b"SVG ",
        ]
        .iter()
        .any(|unsupported| tag == *unsupported)
        {
            return None;
        }
        if previous_tag.is_some_and(|previous| previous >= tag) {
            return None;
        }
        previous_tag = Some(tag);
        let offset = read_be_u32(bytes, entry + 8)? as usize;
        let length = read_be_u32(bytes, entry + 12)? as usize;
        let end = offset.checked_add(length)?;
        let table = bytes.get(offset..end)?;
        if offset < directory_end || offset % 4 != 0 {
            return None;
        }
        let checksum = if tag == b"head" {
            sfnt_checksum(table.get(..8)?).wrapping_add(sfnt_checksum(table.get(12..)?))
        } else {
            sfnt_checksum(table)
        };
        if checksum != read_be_u32(bytes, entry + 4)? {
            return None;
        }
        for prior in 0..index {
            let at = 12 + prior * 16;
            let start = read_be_u32(bytes, at + 8)? as usize;
            let prior_end = start.checked_add(read_be_u32(bytes, at + 12)? as usize)?;
            if offset < prior_end && start < end {
                return None;
            }
        }
    }
    let cmap = sfnt_table(bytes, b"cmap")?;
    if read_be_u16(cmap, 0)? != 0
        || read_be_u16(cmap, 2)? != 1
        || read_be_u16(cmap, 4)? != 1
        || read_be_u16(cmap, 6)? != 0
    {
        return None;
    }
    let sub = cmap.get(read_be_u32(cmap, 8)? as usize..)?;
    let length = usize::from(read_be_u16(sub, 2)?);
    let first = read_be_u16(sub, 6)?;
    let count = usize::from(read_be_u16(sub, 8)?);
    if read_be_u16(sub, 0)? != 6
        || read_be_u16(sub, 4)? != 0
        || length != 10 + count * 2
        || count == 0
        || usize::from(first) + count > 256
    {
        return None;
    }
    Some(Format6 {
        first,
        glyphs: sub.get(10..length)?,
    })
}

/// 明示した単byte幅配列が、再利用する字形のhmtxと0.001/1000em以内で一致するか調べる。
fn widths_agree(
    font: &[u8],
    bytes: &[u8],
    codes: &Format6<'_>,
    mapping: &ToUnicodeCMap,
) -> Option<()> {
    let first = metadata::value(font, b"FirstChar").and_then(syntax::integer)?;
    let last = metadata::value(font, b"LastChar").and_then(syntax::integer)?;
    if first > last || last > 255 {
        return None;
    }
    let units = read_be_u16(sfnt_table(bytes, b"head")?, 18)?;
    let glyph_count = read_be_u16(sfnt_table(bytes, b"maxp")?, 4)?;
    let metrics = read_be_u16(sfnt_table(bytes, b"hhea")?, 34)?;
    if !(16..=16384).contains(&units) || metrics == 0 || metrics > glyph_count {
        return None;
    }
    let hmtx = sfnt_table(bytes, b"hmtx")?;
    hmtx.get(..usize::from(metrics) * 4 + usize::from(glyph_count - metrics) * 2)?;
    let widths = metadata::value(font, b"Widths")?;
    let mut lexer = syntax::Lexer {
        bytes: widths,
        cursor: 0,
    };
    if !matches!(lexer.next(), Some(syntax::Token::Open(false))) {
        return None;
    }
    for code in first..=last {
        let syntax::Token::Word(raw) = lexer.next()? else {
            return None;
        };
        let width = pdf_width(raw)?;
        if !width.is_finite() || width < 0. {
            return None;
        }
        if mapping.lookup_byte(code as u8).is_some() {
            let id = codes.glyph(code as u16)?;
            if id != 0 {
                if id >= glyph_count {
                    return None;
                }
                let advance = read_be_u16(hmtx, usize::from(id.min(metrics - 1)) * 4)?;
                if (width - f64::from(advance) * 1000. / f64::from(units)).abs() > 0.001 {
                    return None;
                }
            }
        }
    }
    if !matches!(lexer.next(), Some(syntax::Token::Close(false))) || lexer.next().is_some() {
        return None;
    }
    for (code, _) in mapping.bmp_scalar_iter() {
        if codes.glyph(code)? != 0 && !(first..=last).contains(&usize::from(code)) {
            return None;
        }
    }
    Some(())
}

/// PDF realの符号・十進小数だけを読み、指数・非有限値・空の数を拒否する。
fn pdf_width(raw: &[u8]) -> Option<f64> {
    let digits = if matches!(raw.first(), Some(b'+' | b'-')) {
        &raw[1..]
    } else {
        raw
    };
    if !digits.iter().any(u8::is_ascii_digit)
        || digits.iter().filter(|&&byte| byte == b'.').count() > 1
        || !digits
            .iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b'.')
    {
        return None;
    }
    let width: f64 = std::str::from_utf8(raw).ok()?.parse().ok()?;
    width.is_finite().then_some(width)
}
