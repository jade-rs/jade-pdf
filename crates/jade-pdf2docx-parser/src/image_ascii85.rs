//! PDF ASCII85を確保前の検証と共通展開予算付きで復号する。
use super::DecodeBudget;

/// PDF空白とz短縮を処理し、検証済みgroupを固定長の作業領域から渡す。
fn visit(
    data: &[u8],
    budget: &DecodeBudget,
    mut emit: impl FnMut(&[u8]) -> Option<()>,
) -> Option<()> {
    let mut value = 0u64;
    let mut digits = 0usize;
    let mut offset = 0usize;
    while let Some(&byte) = data.get(offset) {
        if offset % 4096 == 0 && budget.is_stopped() {
            return None;
        }
        offset += 1;
        match byte {
            0 | 9 | 10 | 12 | 13 | 32 => {}
            b'z' if digits == 0 => emit(&[0; 4])?,
            b'!'..=b'u' => {
                value = value * 85 + u64::from(byte - b'!');
                digits += 1;
                if digits == 5 {
                    emit(&u32::try_from(value).ok()?.to_be_bytes())?;
                    value = 0;
                    digits = 0;
                }
            }
            b'~' => {
                if data.get(offset) != Some(&b'>') || digits == 1 {
                    return None;
                }
                if digits > 1 {
                    for _ in digits..5 {
                        value = value * 85 + 84;
                    }
                    let bytes = u32::try_from(value).ok()?.to_be_bytes();
                    emit(&bytes[..digits - 1])?;
                }
                return (!budget.is_stopped()).then_some(());
            }
            _ => return None,
        }
    }
    None
}

/// 復号サイズを先に数え、stream・累積・確保予算を適用してから復号する。
pub(super) fn decode(data: &[u8], budget: &DecodeBudget) -> Option<Vec<u8>> {
    let mut length = 0usize;
    visit(data, budget, |bytes| {
        length = length.checked_add(bytes.len())?;
        Some(())
    })?;
    budget.charge_form_bytes(length)?;
    let mut output = budget.try_vector_capacity(length)?;
    visit(data, budget, |bytes| {
        output.extend_from_slice(bytes);
        Some(())
    })?;
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParseLimits;

    /// PDF仕様の部分group・z・空白と全長groupを復元する。
    #[test]
    fn pdf_groups_and_whitespace() {
        for (encoded, expected) in [
            (b"~>".as_slice(), b"".as_slice()),
            (b"z~>", &[0, 0, 0, 0]),
            (b"!!~>", &[0]),
            (b"!!!~>", &[0, 0]),
            (b"!!!!~>", &[0, 0, 0]),
            (b"!!!!!~>", &[0, 0, 0, 0]),
            (b"s8W-!~>", &[255, 255, 255, 255]),
            (b"\0\t\n\x0c\r z !! ~>", &[0, 0, 0, 0, 0]),
        ] {
            assert_eq!(
                decode(encoded, &DecodeBudget::default()).as_deref(),
                Some(expected)
            );
        }
    }

    /// 予算の境界値と二段復号へ引き継ぐ累積消費を検証する。
    #[test]
    fn exact_and_cumulative_budgets() {
        let limits = ParseLimits {
            max_decoded_stream_bytes: 4,
            max_total_decoded_bytes: 5,
            ..Default::default()
        };
        let budget = DecodeBudget::new(&limits);
        assert_eq!(decode(b"z~>", &budget), Some(vec![0; 4]));
        assert_eq!(decode(b"!!~>", &budget), Some(vec![0]));
        assert!(decode(b"!!~>", &budget).is_none());
        assert!(budget.is_stopped());
        let budget = DecodeBudget::new(&limits);
        assert!(decode(b"z!!~>", &budget).is_none());
        assert!(budget.is_stopped());
    }

    /// 不正group・非PDF空白・終端の欠落を復号結果へ通さない。
    #[test]
    fn invalid_encodings_are_rejected() {
        for encoded in [
            b"!~>".as_slice(),
            b"uuuuu~>",
            b"uu~>",
            b"!z~>",
            b"z\x0b~>",
            b"!!!!!",
            b"z~ >",
            b"<~z~>",
        ] {
            assert!(
                decode(encoded, &DecodeBudget::default()).is_none(),
                "{encoded:?}"
            );
        }
    }
    /// 途中の停止要求を両走査が共有するvisitorで確認し、部分結果を返さない。
    #[test]
    fn cancellation_during_scan_discards_output() {
        let token = crate::ParseCancellation::new();
        let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
        let mut data = vec![b'z'; 8192];
        data.extend_from_slice(b"~>");
        let mut groups = 0;
        assert!(
            visit(&data, &budget, |_| {
                groups += 1;
                token.cancel();
                Some(())
            })
            .is_none()
        );
        assert_eq!(groups, 4096);
        assert!(matches!(budget.check(), Err(crate::ParseError::Cancelled)));
        assert!(decode(b"z~>", &budget).is_none());
    }
}
