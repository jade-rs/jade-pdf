//! PDF subset が Unicode cmap に存在しない glyph を宣言していないか検査する。

/// Unicode BMP cmap の非空白文字に実際の TrueType outline がある subset だけを採用する。
/// 未対応の cmap・outline 形式は外部フォントへの fallback に任せる。
pub(crate) fn subset_unicode_cmap_is_complete(bytes: &[u8]) -> bool {
    complete(bytes).unwrap_or(false)
}

/// directory と format 4 の境界を検査し、欠落 outline を検出する。
fn complete(bytes: &[u8]) -> Option<bool> {
    let cmap = table(bytes, b"cmap")?;
    let head = table(bytes, b"head")?;
    let loca = table(bytes, b"loca")?;
    let glyf = table(bytes, b"glyf")?;
    let glyph_count = word(table(bytes, b"maxp")?, 4)?;
    let long = match word(head, 50)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    for record in 0..usize::from(word(cmap, 2)?) {
        let start = 4 + record * 8;
        if word(cmap, start)? != 3 || word(cmap, start + 2)? != 1 {
            continue;
        }
        let offset = dword(cmap, start + 4)?;
        let sub = cmap.get(offset..)?;
        if word(sub, 0)? != 4 {
            continue;
        }
        let sub = sub.get(..usize::from(word(sub, 2)?))?;
        let segment_count_x2 = usize::from(word(sub, 6)?);
        if segment_count_x2 == 0 || segment_count_x2 % 2 != 0 {
            return None;
        }
        let segments = segment_count_x2 / 2;
        let mut mapped = false;
        let mut previous_end = None;
        for index in 0..segments {
            let end = word(sub, 14 + index * 2)?;
            let first = word(sub, 16 + segments * 2 + index * 2)?;
            if first > end || previous_end.is_some_and(|previous| previous >= first) {
                return None;
            }
            previous_end = Some(end);
            let delta = word(sub, 16 + segments * 4 + index * 2)?;
            let range_start = 16 + segments * 6 + index * 2;
            let range = usize::from(word(sub, range_start)?);
            for code in first..=end {
                if code == u16::MAX {
                    continue;
                }
                let glyph = if range == 0 {
                    code.wrapping_add(delta)
                } else {
                    let glyph = word(sub, range_start + range + usize::from(code - first) * 2)?;
                    if glyph == 0 {
                        0
                    } else {
                        glyph.wrapping_add(delta)
                    }
                };
                if glyph == 0 {
                    continue;
                }
                if glyph >= glyph_count {
                    return Some(false);
                }
                let offset = |glyph: u16| -> Option<usize> {
                    if long {
                        dword(loca, usize::from(glyph) * 4)
                    } else {
                        Some(usize::from(word(loca, usize::from(glyph) * 2)?) * 2)
                    }
                };
                let start = offset(glyph)?;
                let end = offset(glyph.checked_add(1)?)?;
                if start > end
                    || end > glyf.len()
                    || (start == end && !char::from_u32(u32::from(code))?.is_whitespace())
                {
                    return Some(false);
                }
                mapped = true;
            }
        }
        return Some(mapped);
    }
    Some(false)
}

/// sfnt directory の指定 table を境界検査して読む。
fn table<'a>(bytes: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    for index in 0..usize::from(word(bytes, 4)?) {
        let record = 12 + index * 16;
        if bytes.get(record..record + 4)? == tag {
            let offset = dword(bytes, record + 8)?;
            let length = dword(bytes, record + 12)?;
            return bytes.get(offset..offset.checked_add(length)?);
        }
    }
    None
}

/// big endian の16 bit値を境界検査して読む。
fn word(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// big endian の32 bit値を host の offset へ変換する。
fn dword(bytes: &[u8], offset: usize) -> Option<usize> {
    usize::try_from(u32::from_be_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
    .ok()
}
