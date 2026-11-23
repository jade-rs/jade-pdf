//! eexecとcharstringに共通するType1暗号を、保存前の共有予算で復号する。
use crate::decode::DecodeBudget;

/// 捨てるprefixを保存せず、完成結果だけを返す。
pub(super) fn decrypt(
    budget: &DecodeBudget,
    bytes: &[u8],
    mut seed: u16,
    skip: usize,
) -> Option<Vec<u8>> {
    let mut output = budget.font_vector(bytes.len().checked_sub(skip)?)?;
    for (index, &cipher) in bytes.iter().enumerate() {
        if index.is_multiple_of(1024) && budget.is_stopped() {
            return None;
        }
        let plain = cipher ^ (seed >> 8) as u8;
        seed = u16::from(cipher)
            .wrapping_add(seed)
            .wrapping_mul(52845)
            .wrapping_add(22719);
        if index >= skip {
            output.push(plain);
        }
    }
    (!budget.is_stopped()).then_some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, ParseLimits};
    use proptest::prelude::*;

    /// 出力保存だけの累積上限を指定する。
    fn budget(capacity: u64) -> DecodeBudget {
        DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: capacity,
            ..Default::default()
        })
    }

    /// 独立したfontTools4.60.1で得た二つのseedの固定vectorを検査する。
    #[test]
    fn independent_vectors_recover_both_encryption_layers() {
        for (seed, cipher) in [
            (
                4330,
                [
                    113, 148, 100, 197, 103, 232, 181, 1, 44, 75, 146, 253, 60, 8, 27,
                ],
            ),
            (
                55665,
                [
                    184, 88, 141, 165, 57, 159, 32, 240, 241, 134, 125, 157, 95, 47, 224,
                ],
            ),
        ] {
            assert_eq!(
                decrypt(&budget(11), &cipher, seed, 4).as_deref(),
                Some(b"Type1 glyph".as_slice())
            );
        }
    }

    /// prefixを除いた実保存量が境界で成功し、解放しても累積量を戻さない。
    #[test]
    fn stored_bytes_are_exact_cumulative_and_stopped() {
        let cipher = [
            184, 88, 141, 165, 57, 159, 32, 240, 241, 134, 125, 157, 95, 47, 224,
        ];
        let target = budget(10);
        assert!(decrypt(&target, &cipher, 55665, 4).is_none());
        assert!(matches!(
            target.check(),
            Err(ParseError::ResourceLimit {
                resource: "embedded font bytes",
                actual: 11,
                ..
            })
        ));
        let target = budget(11);
        assert!(decrypt(&target, &cipher, 55665, 4).is_some());
        assert!(decrypt(&target, &cipher, 55665, 4).is_none());
        assert!(decrypt(&target, &[], 55665, 0).is_none());
        assert_eq!(
            decrypt(&budget(0), &cipher[..4], 55665, 4),
            Some(Vec::new())
        );
        assert_eq!(decrypt(&budget(0), &cipher[..3], 55665, 4), None);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 任意の二進入力・seed・prefixで、別の暗号化実装と一致し切り詰めを拒否する。
        #[test]
        fn roundtrip_keeps_binary_data_and_prefix_boundary(plain in proptest::collection::vec(any::<u8>(), 0..512), seed in any::<u16>(), prefix in 0_usize..12) {
            let mut source = vec![0xa5; prefix];
            source.extend_from_slice(&plain);
            let mut state = seed;
            let cipher = source.iter().map(|byte| {
                let encrypted = byte ^ (state >> 8) as u8;
                state = (u32::from(encrypted) + u32::from(state)).wrapping_mul(52845).wrapping_add(22719) as u16;
                encrypted
            }).collect::<Vec<_>>();
            prop_assert_eq!(decrypt(&budget(plain.len() as u64), &cipher, seed, prefix), Some(plain));
            prop_assert!(decrypt(&budget(0), &cipher, seed, cipher.len()+1).is_none());
        }
    }
}
