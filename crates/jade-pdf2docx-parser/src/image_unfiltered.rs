//! filterのない8 bit Device RGB/Grayを、既存の色写像・PNG・資源予算で復元する。
use super::{DecodeBudget, image_attribute, image_pixels, image_sample_decode::SampleDecode};
use crate::metadata;

/// 対応範囲を直接宣言した画像だけを選び、未知のfilterや色空間を無圧縮と推測しない。
pub(super) fn is_candidate(dictionary: &[u8]) -> bool {
    metadata::dictionary_entries_complete(dictionary)
        && metadata::value(dictionary, b"Filter").is_none()
        && metadata::value(dictionary, b"BitsPerComponent") == Some(b"8")
        && metadata::value(dictionary, b"ColorSpace").is_some_and(|color| {
            metadata::is_name(color, b"DeviceRGB") || metadata::is_name(color, b"DeviceGray")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, ParseLimits};

    /// 二画素RGBの有限なsource属性。
    const RGB: &[u8] = b"<< /BitsPerComponent 8 /ColorSpace /DeviceRGB >>";

    /// 新しいsource配列も、確保前に画像保存・単一展開・累積確保上限へ数える。
    #[test]
    fn raw_samples_obey_all_buffer_limits() {
        for (limits, resource) in [
            (
                ParseLimits {
                    max_decoded_stream_bytes: 5,
                    ..Default::default()
                },
                "decoded stream bytes",
            ),
            (
                ParseLimits {
                    max_total_image_storage_bytes: 5,
                    ..Default::default()
                },
                "image storage bytes",
            ),
            (
                ParseLimits {
                    max_total_allocation_bytes: 0,
                    ..Default::default()
                },
                "total allocation bytes",
            ),
        ] {
            let budget = DecodeBudget::new(&limits);
            assert!(decode(&budget, b"", RGB, &[1; 6], 2, 1).is_none());
            assert!(
                matches!(budget.check(), Err(ParseError::ResourceLimit { resource:r, .. }) if r==resource)
            );
            assert!(decode(&budget, b"", RGB, &[1; 6], 2, 1).is_none());
        }
    }

    /// 解放済み画像も累積へ含め、上限ちょうどの二枚と次の一枚を区別する。
    #[test]
    fn decoded_sample_budget_is_cumulative_and_inclusive() {
        let budget = DecodeBudget::new(&ParseLimits {
            max_decoded_stream_bytes: 6,
            max_total_decoded_bytes: 12,
            ..Default::default()
        });
        assert!(decode(&budget, b"", RGB, &[1; 6], 2, 1).is_some());
        assert!(decode(&budget, b"", RGB, &[2; 6], 2, 1).is_some());
        assert!(decode(&budget, b"", RGB, &[3; 6], 2, 1).is_none());
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "total decoded bytes",
                limit: 12,
                actual: 18
            })
        ));
    }

    /// 重複・不完全辞書・未知filterを採用せず、nestedの名前を外側属性と誤認しない。
    #[test]
    fn ambiguous_metadata_never_becomes_successful_raw_pixels() {
        for dict in [
            b"<< /BitsPerComponent 8 /ColorSpace /DeviceRGB /ColorSpace /DeviceGray >>".as_slice(),
            b"<< /BitsPerComponent 8 /ColorSpace /DeviceRGB /Decode [0 1 0 1 0 1] /Decode [1 0 1 0 1 0] >>",
            b"<< /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter null >>",
            b"<< /BitsPerComponent 8 /ColorSpace /DeviceRGB /ImageMask true >>",
            b"<< /BitsPerComponent 8 /ColorSpace /DeviceRGB /Decode [0 1",
        ] {
            assert!(decode(&DecodeBudget::new(&ParseLimits::default()),b"",dict,&[1;6],2,1).is_none());
        }
        let nested =
            b"<< /BitsPerComponent 8 /ColorSpace /DeviceRGB /Other << /Filter /JPXDecode >> >>";
        assert!(
            decode(
                &DecodeBudget::new(&ParseLimits::default()),
                b"",
                nested,
                &[1; 6],
                2,
                1
            )
            .is_some()
        );
    }
}

/// maskやfilterパラメータを黙って省かず、正確なsample長を検証してから確保する。
pub(super) fn decode(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    if !is_candidate(dictionary) || width == 0 || height == 0 || budget.is_stopped() {
        return None;
    }
    budget.check_image_pixels(width, height)?;
    // 重複属性は最初の値を採って画像を成功させない。
    let mut keys = 0_u16;
    for (key, _) in metadata::entries(dictionary)? {
        for (index, expected) in [
            b"Filter".as_slice(),
            b"ColorSpace",
            b"BitsPerComponent",
            b"Decode",
            b"DecodeParms",
            b"Mask",
            b"SMask",
            b"ImageMask",
        ]
        .into_iter()
        .enumerate()
        {
            if crate::syntax::name_is(key, expected) {
                let bit = 1_u16 << index;
                if keys & bit != 0 {
                    return None;
                }
                keys |= bit;
            }
        }
    }
    for key in [b"Mask".as_slice(), b"SMask", b"DecodeParms"] {
        if metadata::value(dictionary, key).is_some() {
            let value = image_attribute(budget, pdf, dictionary, key)?;
            if !super::image_icc::is_null(&value) {
                return None;
            }
        }
    }
    if super::image_stencil::is_mask(budget, pdf, dictionary)? {
        return None;
    }
    let color = metadata::value(dictionary, b"ColorSpace")?;
    let gray = metadata::is_name(color, b"DeviceGray");
    let count = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    let expected = count.checked_mul(if gray { 1 } else { 3 })?;
    if data.len() != expected {
        return None;
    }
    let mapping = SampleDecode::read(budget, pdf, dictionary)?;
    budget.charge_form_bytes(expected)?;
    let mut samples = budget.image_vector(expected)?;
    samples.extend_from_slice(data);
    mapping.apply(budget, &mut samples)?;
    if gray {
        let rgb = image_pixels::gray(budget, &samples)?;
        super::encode_rgb8_png(budget, width, height, &rgb)
    } else {
        super::encode_rgb8_png(budget, width, height, &samples)
    }
}
