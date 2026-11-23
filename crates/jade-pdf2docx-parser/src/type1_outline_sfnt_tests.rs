//! table配置とchecksumを、writerとは独立した読取式で照合する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 試験用のhead。書込時に旧調整値を取り除くことも検査する。
fn head() -> [u8; 54] {
    let mut bytes = [0; 54];
    bytes[..4].copy_from_slice(&0x00010000_u32.to_be_bytes());
    bytes[8..12].fill(0xaa);
    bytes[12..16].copy_from_slice(&0x5f0f3cf5_u32.to_be_bytes());
    bytes
}

/// 独立なpadding加算で、font全体のchecksumを計算する。
fn sum(bytes: &[u8]) -> u32 {
    let mut result = 0_u32;
    for offset in (0..bytes.len()).step_by(4) {
        let mut word = 0_u32;
        for index in 0..4 {
            word = word
                .wrapping_mul(256)
                .wrapping_add(u32::from(*bytes.get(offset + index).unwrap_or(&0)));
        }
        result = result.wrapping_add(word);
    }
    result
}

/// 任意位置の32bit値を大端として読む。
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// tag順・directory値・整列・headと全体のchecksumを個別に検証する。
#[test]
fn directory_is_sorted_aligned_and_independently_checksummed() {
    let header = head();
    let tables = [
        Table {
            tag: *b"post",
            bytes: b"xyz",
        },
        Table {
            tag: *b"head",
            bytes: &header,
        },
        Table {
            tag: *b"CFF ",
            bytes: b"abcde",
        },
    ];
    let bytes = pack(&DecodeBudget::default(), &tables).unwrap();
    assert_eq!(&bytes[..12], &[79, 84, 84, 79, 0, 3, 0, 32, 0, 1, 0, 16]);
    assert_eq!(sum(&bytes), 0xb1b0afba);
    assert_eq!(bytes.len() % 4, 0);
    for (index, tag) in [b"CFF ", b"head", b"post"].iter().enumerate() {
        let row = 12 + 16 * index;
        assert_eq!(&bytes[row..row + 4], *tag);
        let offset = u32_at(&bytes, row + 8) as usize;
        let length = u32_at(&bytes, row + 12) as usize;
        assert_eq!(offset % 4, 0);
        let mut payload = bytes[offset..offset + length].to_vec();
        if *tag == b"head" {
            assert_ne!(&payload[8..12], &[0xaa; 4]);
            payload[8..12].fill(0);
        }
        assert_eq!(u32_at(&bytes, row + 4), sum(&payload));
        let original = tables.iter().find(|table| &table.tag == *tag).unwrap();
        let mut expected = original.bytes.to_vec();
        if *tag == b"head" {
            expected[8..12].fill(0);
        }
        assert_eq!(payload, expected);
    }
    assert_eq!(header[8..12], [0xaa; 4]);
}

/// 保存は最終font長ちょうどで累積計上し、重複tagと壊れたheadを拒否する。
#[test]
fn exact_storage_and_invalid_tables_are_checked() {
    let header = head();
    let tables = [Table {
        tag: *b"head",
        bytes: &header,
    }];
    let required = 12 + 16 + 56;
    let exact = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: required,
        ..Default::default()
    });
    assert_eq!(pack(&exact, &tables).unwrap().len(), required as usize);
    assert!(pack(&exact, &tables).is_none());
    assert!(
        matches!(exact.check(),Err(ParseError::ResourceLimit { resource:"embedded font bytes",actual,.. }) if actual==required*2)
    );
    assert!(pack(&DecodeBudget::default(), &[]).is_none());
    assert!(
        pack(
            &DecodeBudget::default(),
            &[
                Table {
                    tag: *b"head",
                    bytes: &header
                },
                Table {
                    tag: *b"head",
                    bytes: &header
                }
            ]
        )
        .is_none()
    );
    assert!(
        pack(
            &DecodeBudget::default(),
            &[Table {
                tag: *b"head",
                bytes: &[0; 12]
            }]
        )
        .is_none()
    );
    assert!(
        pack(
            &DecodeBudget::default(),
            &[Table {
                tag: *b"name",
                bytes: &[0; 54]
            }]
        )
        .is_none()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意二進tableでも、font全体のchecksumと元payloadを維持する。
    #[test]
    fn arbitrary_tables_keep_their_bytes(payload in proptest::collection::vec(any::<u8>(),0..4096),reverse in any::<bool>()) {
        let header=head();let mut tables=[Table {tag:*b"head",bytes:&header},Table {tag:*b"CFF ",bytes:&payload}];if reverse { tables.reverse(); }
        let bytes=pack(&DecodeBudget::default(),&tables).unwrap();
        prop_assert_eq!(sum(&bytes),0xb1b0afba);
        let offset=u32_at(&bytes,20) as usize;let length=u32_at(&bytes,24) as usize;
        prop_assert_eq!(&bytes[offset..offset+length],&payload);
    }
}
