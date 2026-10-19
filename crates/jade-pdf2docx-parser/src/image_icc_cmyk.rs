//! CMYK→三成分PCSのLUT16だけを検証してCMSへ渡す。
use super::super::{DecodeBudget, ImageFilter, image_attribute, image_sample_decode::number};
use moxcms::{ColorProfile, DataColorSpace, ParsingOptions};

/// 範囲を検査してbig endian整数を読む。
fn integer(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// 四入力・三出力LUT16の次元と全table長をCMSより先に検査する。
fn valid_lut(bytes: &[u8]) -> Option<()> {
    if bytes.len() < 52
        || &bytes[..4] != b"mft2"
        || bytes[8..10] != [4, 3]
        || !(2..=17).contains(&bytes[10])
    {
        return None;
    }
    let input = usize::from(u16::from_be_bytes(bytes[48..50].try_into().ok()?));
    let output = usize::from(u16::from_be_bytes(bytes[50..52].try_into().ok()?));
    if !(2..=4096).contains(&input) || !(2..=4096).contains(&output) {
        return None;
    }
    let nodes = usize::from(bytes[10]).checked_pow(4)?;
    let entries = input
        .checked_mul(4)?
        .checked_add(nodes.checked_mul(3)?)?
        .checked_add(output.checked_mul(3)?)?;
    (bytes.len() == 52usize.checked_add(entries.checked_mul(2)?)?).then_some(())
}

/// ICCBased alternateのN・Range・filterと、profile内の有界A2B/白色点を検証する。
pub(in super::super) fn read(
    budget: &DecodeBudget,
    pdf: &[u8],
    reference: &[u8],
) -> Option<ColorProfile> {
    let color = if let Some(object) = budget.reference(pdf, reference) {
        super::super::object_body_for_number(budget, pdf, object)?
    } else {
        super::super::ObjectBytes::Borrowed(reference)
    };
    let mut entries = crate::metadata::array_entries(&color, 2)?;
    if !crate::metadata::is_name(entries.next()??, b"ICCBased") {
        return None;
    }
    let object = budget.reference(pdf, entries.next()??)?;
    if entries.next().is_some() {
        return None;
    }
    let (dict, data) = super::super::extract_stream_for_object(budget, pdf, object)?;
    if number(budget, pdf, &image_attribute(budget, pdf, dict, b"N")?)? != 4.0 {
        return None;
    }
    if crate::metadata::value(dict, b"Range").is_some() {
        let value = image_attribute(budget, pdf, dict, b"Range")?;
        let mut entries = crate::metadata::array_entries(&value, 8)?;
        for expected in [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0] {
            if number(budget, pdf, entries.next()??)? != expected {
                return None;
            }
        }
        if entries.next().is_some() {
            return None;
        }
    }
    if crate::metadata::value(dict, b"DecodeParms").is_some()
        && !super::is_null(&image_attribute(budget, pdf, dict, b"DecodeParms")?)
    {
        return None;
    }
    let decoded;
    let bytes = if crate::metadata::value(dict, b"Filter").is_none() {
        data
    } else {
        if ImageFilter::read(dict)? != ImageFilter::Flate {
            return None;
        }
        decoded = super::super::decode_zlib(data, budget)?;
        &decoded
    };
    if !(132..=1_048_576).contains(&bytes.len())
        || integer(bytes, 0)? as usize != bytes.len()
        || bytes.get(16..20)? != b"CMYK"
        || !matches!(bytes.get(20..24)?, b"Lab " | b"XYZ ")
    {
        return None;
    }
    let count = integer(bytes, 128)? as usize;
    if count > 32 || 132usize.checked_add(count.checked_mul(12)?)? > bytes.len() {
        return None;
    }
    let mut selected = [None; 4];
    for index in 0..count {
        let entry = 132 + index * 12;
        let signature = &bytes[entry..entry + 4];
        // より新しい多次元tagを落として別の変換へ黙ってfallbackしない。
        if matches!(&signature[..3], b"D2B" | b"B2D") {
            return None;
        }
        if let Some(slot) = [b"A2B0", b"A2B1", b"A2B2", b"wtpt"]
            .iter()
            .position(|name| signature == *name)
        {
            if selected[slot].is_some() {
                return None;
            }
            let offset = integer(bytes, entry + 4)? as usize;
            let length = integer(bytes, entry + 8)? as usize;
            if offset < 132 + count * 12 {
                return None;
            }
            let value = bytes.get(offset..offset.checked_add(length)?)?;
            if slot < 3 {
                valid_lut(value)?;
            } else if value.len() != 20 || &value[..4] != b"XYZ " {
                return None;
            }
            selected[slot] = Some(entry);
        }
    }
    if selected[0].is_none() || selected[3].is_none() {
        return None;
    }
    let mut normalized = budget.copy_stream_bytes(bytes)?;
    let mut count = 0;
    for entry in selected.into_iter().flatten() {
        normalized[132 + count * 12..144 + count * 12].copy_from_slice(&bytes[entry..entry + 12]);
        count += 1;
    }
    normalized[128..132].copy_from_slice(&(count as u32).to_be_bytes());
    // 有界入力に対するCMS内部LUT/変換table分の保守的予約。実RSSの強制上限ではない。
    budget.reserve_allocation::<u8>(32 * 1024 * 1024)?;
    let profile = ColorProfile::new_from_slice_with_options(
        &normalized,
        ParsingOptions {
            max_profile_size: 1_048_576,
            max_allowed_clut_size: 83_521,
            max_allowed_trc_size: 4096,
        },
    )
    .ok()?;
    if profile.color_space != DataColorSpace::Cmyk
        || !matches!(profile.pcs, DataColorSpace::Lab | DataColorSpace::Xyz)
    {
        return None;
    }
    (!budget.is_stopped()).then_some(profile)
}
