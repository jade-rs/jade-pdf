//! CFF1 INDEXを、正確な長さの事前予約と一基準offsetで格納する。
use crate::decode::DecodeBudget;

/// 文字列・glyph programを借用し、件数・offset・総長を検査して格納する。
pub(super) fn encode(budget: &DecodeBudget, objects: &[&[u8]]) -> Option<Vec<u8>> {
    let _ = (budget, objects);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, ParseLimits};
    use proptest::prelude::*;

    /// 空配列・空object・複数objectを、異なる境界として格納する。
    #[test]
    fn cff_index_has_one_based_boundaries_and_empty_special_case() {
        for (objects, expected) in [
            (vec![], vec![0, 0]),
            (vec![b"".as_slice()], vec![0, 1, 1, 1, 1]),
            (vec![b"A".as_slice()], vec![0, 1, 1, 1, 2, 65]),
            (
                vec![b"ab".as_slice(), b"cde".as_slice()],
                vec![0, 2, 1, 1, 3, 6, 97, 98, 99, 100, 101],
            ),
        ] {
            assert_eq!(encode(&DecodeBudget::default(), &objects), Some(expected));
        }
    }

    /// 最後のoffsetが桁を跨ぐ境界で、幅を一段だけ増やす。
    #[test]
    fn index_offset_size_accounts_for_the_extra_one() {
        for (length, width) in [(254, 1), (255, 2), (65534, 2), (65535, 3)] {
            let object = vec![7; length];
            let encoded = encode(&DecodeBudget::default(), &[&object]).unwrap();
            assert_eq!(encoded[2], width);
            assert_eq!(encoded.len(), 3 + 2 * usize::from(width) + length);
            assert_eq!(&encoded[encoded.len() - length..], &object);
        }
    }

    /// 総保存量は正確かつ累積で、件数・8MiB上限も確保より前に拒否する。
    #[test]
    fn index_storage_and_format_limits_are_checked_before_allocation() {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: 6,
            ..Default::default()
        });
        assert!(encode(&budget, &[b"A"]).is_some());
        assert!(encode(&budget, &[b"A"]).is_none());
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "embedded font bytes",
                actual: 12,
                ..
            })
        ));
        let too_many = vec![b"".as_slice(); 65536];
        assert!(encode(&DecodeBudget::default(), &too_many).is_none());
        let too_large = vec![0; crate::parser::MAX_EMBEDDED_FONT_BYTES];
        assert!(encode(&DecodeBudget::default(), &[&too_large]).is_none());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// 任意の二進objectが、独立に読み取ったoffset境界でそのまま復元できる。
        #[test]
        fn binary_objects_keep_exact_slices(objects in proptest::collection::vec(proptest::collection::vec(any::<u8>(),0..128),0..40)) {
            let references=objects.iter().map(Vec::as_slice).collect::<Vec<_>>();
            let bytes=encode(&DecodeBudget::default(),&references).unwrap();
            prop_assert_eq!(u16::from_be_bytes([bytes[0],bytes[1]]) as usize,objects.len());
            if objects.is_empty() { prop_assert_eq!(bytes,vec![0,0]); }
            else {
                let width=usize::from(bytes[2]);let data=3+(objects.len()+1)*width;
                for (index,object) in objects.iter().enumerate() {
                    let start=bytes[3+index*width..3+(index+1)*width].iter().fold(0_usize,|v,b|v*256+usize::from(*b));
                    let end=bytes[3+(index+1)*width..3+(index+2)*width].iter().fold(0_usize,|v,b|v*256+usize::from(*b));
                    prop_assert_eq!(&bytes[data+start-1..data+end-1],object);
                }
            }
        }
    }
}
