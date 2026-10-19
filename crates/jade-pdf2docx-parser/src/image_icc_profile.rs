//! ICC RGB matrix/TRCとD50 Gray TRCを有界のCMS入力へ正規化する。
use super::super::{DecodeBudget, image_attribute};
use moxcms::{ColorProfile, DataColorSpace, ParsingOptions};

/// profile本体・tag table・curveの大きさを制限し、未対応LUTを拒否する。
pub(super) fn read(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    data: &[u8],
) -> Option<ColorProfile> {
    read_components(budget, pdf, dict, data, false)
}

/// 白色点変換を必要としないD50 Gray TRC profileだけを読む。
pub(super) fn read_gray(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    data: &[u8],
) -> Option<ColorProfile> {
    read_components(budget, pdf, dict, data, true)
}

/// 成分数に応じて必要なtagを検証し、共通のprofile予算内へ正規化する。
fn read_components(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    data: &[u8],
    gray: bool,
) -> Option<ColorProfile> {
    let components = if gray { 1 } else { 3 };
    if crate::metadata::value(dict, b"Range").is_some() {
        let range = image_attribute(budget, pdf, dict, b"Range")?;
        let mut entries = crate::metadata::array_entries(&range, components * 2)?;
        for expected in [0.0, 1.0].into_iter().cycle().take(components * 2) {
            if super::super::image_sample_decode::number(budget, pdf, entries.next()??)? != expected
            {
                return None;
            }
        }
        if entries.next().is_some() {
            return None;
        }
    }
    if crate::metadata::value(dict, b"DecodeParms").is_some() {
        let parameters = image_attribute(budget, pdf, dict, b"DecodeParms")?;
        let mut lexer = crate::syntax::Lexer {
            bytes: &parameters,
            cursor: 0,
        };
        if !matches!(lexer.next(), Some(crate::syntax::Token::Word(b"null")))
            || lexer.next().is_some()
        {
            return None;
        }
    }
    let decoded;
    let bytes = if crate::metadata::value(dict, b"Filter").is_none() {
        data
    } else {
        if super::super::ImageFilter::read(dict)? != super::super::ImageFilter::Flate {
            return None;
        }
        decoded = super::super::decode_zlib(data, budget)?;
        &decoded
    };
    if bytes.len() > 65535
        || bytes.len() < 132
        || integer(bytes, 0)? as usize != bytes.len()
        || bytes.get(16..20)? != if gray { b"GRAY" } else { b"RGB " }
        || bytes.get(20..24)? != b"XYZ "
    {
        return None;
    }
    let count = integer(bytes, 128)? as usize;
    if count > 32 || 132usize.checked_add(count.checked_mul(12)?)? > bytes.len() {
        return None;
    }
    // CMSにmetadata・未知の変換tagを渡さず、必要なmatrix/TRCだけを残す。
    let selected: &[&[u8; 4]] = if gray {
        &[b"kTRC", b"wtpt"]
    } else {
        &[
            b"rXYZ", b"gXYZ", b"bXYZ", b"rTRC", b"gTRC", b"bTRC", b"wtpt", b"chad",
        ]
    };
    let mut offsets = [None; 8];
    for index in 0..count {
        let entry = 132 + index * 12;
        let signature = bytes.get(entry..entry + 4)?;
        if matches!(&signature[..3], b"A2B" | b"B2A" | b"D2B" | b"B2D")
            || (gray && signature == b"chad")
        {
            return None;
        }
        if let Some(slot) = selected.iter().position(|name| signature == *name) {
            if offsets[slot].is_some() {
                return None;
            }
            let offset = integer(bytes, entry + 4)? as usize;
            let size = integer(bytes, entry + 8)? as usize;
            if offset < 132 + count * 12 || size < 12 {
                return None;
            }
            let value = bytes.get(offset..offset.checked_add(size)?)?;
            match signature {
                b"rXYZ" | b"gXYZ" | b"bXYZ" | b"wtpt" if &value[..4] == b"XYZ " && size == 20 => {}
                b"rTRC" | b"gTRC" | b"bTRC" | b"kTRC" if &value[..4] == b"para" && size <= 40 => {}
                b"rTRC" | b"gTRC" | b"bTRC" | b"kTRC"
                    if &value[..4] == b"curv" && integer(value, 8)? <= 4096 => {}
                b"chad" if &value[..4] == b"sf32" && size == 44 => {}
                _ => return None,
            }
            offsets[slot] = Some(entry);
        }
    }
    if offsets[..if gray { 2 } else { 7 }]
        .iter()
        .any(Option::is_none)
    {
        return None;
    }
    let mut normalized = budget.copy_stream_bytes(bytes)?;
    let mut count = 0usize;
    for entry in offsets.into_iter().flatten() {
        normalized[132 + count * 12..144 + count * 12].copy_from_slice(&bytes[entry..entry + 12]);
        count += 1;
    }
    normalized[128..132].copy_from_slice(&(count as u32).to_be_bytes());
    // 有界matrix/TRCのCMS内部table用に保守的容量を予約する。実RSS上限ではない。
    budget.reserve_allocation::<u8>(16 * 1024 * 1024)?;
    let profile = ColorProfile::new_from_slice_with_options(
        &normalized,
        ParsingOptions {
            max_profile_size: 65536,
            max_allowed_clut_size: 0,
            max_allowed_trc_size: 4096,
        },
    )
    .ok()?;
    if profile.color_space
        != if gray {
            DataColorSpace::Gray
        } else {
            DataColorSpace::Rgb
        }
        || profile.pcs != DataColorSpace::Xyz
    {
        return None;
    }
    // moxcmsのGray経路は白色点を反映しないため、D50以外を通さない。
    if gray && (!is_d50(profile.white_point) || !is_d50(profile.media_white_point?)) {
        return None;
    }
    (!budget.is_stopped()).then_some(profile)
}

/// 範囲を検査してICCのbig endian整数を読む。
fn integer(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// ICC固定小数点の丸め差だけを許し、D50白色点を検証する。
fn is_d50(point: moxcms::Xyzd) -> bool {
    [point.x - 0.9642, point.y - 1.0, point.z - 0.8249]
        .into_iter()
        .all(|delta| delta.abs() <= 2.0 / 65536.0)
}
