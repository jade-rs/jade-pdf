//! 圧縮objectの索引・本文保存と再作成を独立した要求容量で検証する。
use super::*;
use proptest::prelude::*;
use std::fmt::Write as _;

/// metadata容量だけを変更する予算を作る。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 一つの未圧縮ObjStmと、通常/圧縮entryを持つ旧xref streamを作る。
fn pdf(body: &[u8]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let offset = bytes.len() as u32;
    bytes.extend_from_slice(
        format!(
            "1 0 obj << /Type /ObjStm /N 1 /First 4 /Length {} >> stream\n7 0 ",
            body.len() + 4
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(b"\nendstream endobj\n9 0 obj << /Type /XRef /Size 10 /W [1 4 1] /Index [1 1 7 1] /Length 12 >> stream\n");
    bytes.push(1);
    bytes.extend_from_slice(&offset.to_be_bytes());
    bytes.extend_from_slice(&[0, 2, 0, 0, 0, 1, 0]);
    bytes.extend_from_slice(b"\nendstream endobj\n");
    bytes
}

/// 旧xref一時配列・member配列・保存slot・本文・二つのRc管理領域を数える。
fn capacity(body_length: usize) -> u64 {
    (4 * std::mem::size_of::<(u32, XrefStreamEntry, usize)>()
        + std::mem::size_of::<(u32, u32, u32)>()
        + std::mem::size_of::<(u32, Rc<Vec<u8>>)>()
        + body_length
        + 2 * (std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>())) as u64
}

/// member全件とowner/xref自身をIndexへ登録し、正式/旧経路の両方を作る。
fn members_pdf(bodies: &[Vec<u8>], formal: bool) -> Vec<u8> {
    let mut header = String::new();
    let mut area = Vec::new();
    for (index, body) in bodies.iter().enumerate() {
        write!(header, "{} {} ", 10 + index, area.len()).unwrap();
        area.extend_from_slice(body);
    }
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let owner_offset = bytes.len() as u32;
    bytes.extend_from_slice(
        format!(
            "1 0 obj << /Type /ObjStm /N {} /First {} /Length {} >> stream\n",
            bodies.len(),
            header.len(),
            header.len() + area.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(&area);
    bytes.extend_from_slice(b"\nendstream endobj\n");
    let xref_offset = bytes.len() as u32;
    let mut entries = Vec::new();
    entries.push(1);
    entries.extend_from_slice(&owner_offset.to_be_bytes());
    entries.extend_from_slice(&0u32.to_be_bytes());
    for index in 0..bodies.len() {
        entries.push(2);
        entries.extend_from_slice(&1u32.to_be_bytes());
        entries.extend_from_slice(&(index as u32).to_be_bytes());
    }
    entries.push(1);
    entries.extend_from_slice(&xref_offset.to_be_bytes());
    entries.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(format!("999 0 obj << /Type /XRef /Size 1000 /W [1 4 4] /Index [1 1 10 {} 999 1] /Length {} >> stream\n", bodies.len(), entries.len()).as_bytes());
    bytes.extend_from_slice(&entries);
    bytes.extend_from_slice(b"\nendstream endobj\n");
    if formal {
        bytes.extend_from_slice(format!("startxref\n{xref_offset}\n%%EOF\n").as_bytes());
    }
    bytes
}

/// 正式xrefと構造の読み取りを独立予算で済ませ、検査済み入力のcacheを準備する。
fn prepare(budget: &DecodeBudget, bytes: &[u8]) {
    let xref = crate::xref::XrefIndex::read(
        Path::new("compressed.pdf"),
        bytes,
        &ParseLimits::default(),
        &DecodeBudget::default(),
    )
    .unwrap();
    let spans = crate::structure::validate_structure(
        Path::new("compressed.pdf"),
        bytes,
        9,
        &ParseLimits::default(),
        &DecodeBudget::default(),
    )
    .unwrap();
    budget.cache_xref(xref);
    budget.cache_streams(bytes, &spans);
}

/// 検査済みstream cacheだけの準備容量を公開エラーから取得する。
fn preparation_capacity(bytes: &[u8]) -> u64 {
    let mut limit = 0;
    for _ in 0..16 {
        let budget = budget(limit);
        prepare(&budget, bytes);
        match budget.check() {
            Ok(()) => return limit,
            Err(ParseError::ResourceLimit { actual, .. }) => {
                assert!(actual > limit);
                limit = actual;
            }
            Err(error) => panic!("準備失敗: {error}"),
        }
    }
    panic!("準備容量を特定できない")
}

/// 本文/保存slot/共有管理領域と、正式member配列の全要求容量を数える。
fn member_capacity(bodies: &[Vec<u8>]) -> u64 {
    let n = bodies.len();
    (n * (std::mem::size_of::<(u32, u32, u32)>()
        + std::mem::size_of::<(u32, Rc<Vec<u8>>)>()
        + std::mem::size_of::<Vec<u8>>()
        + 2 * std::mem::size_of::<usize>())
        + bodies.iter().map(Vec::len).sum::<usize>()
        + std::mem::size_of::<Vec<u8>>()
        + 2 * std::mem::size_of::<usize>()) as u64
}

/// 旧xref候補の初回4slotと倍増時の全容量を数える。
fn revision_capacity(count: usize) -> u64 {
    let mut capacity = 4;
    let mut total = capacity;
    while capacity < count {
        capacity *= 2;
        total += capacity;
    }
    (total * std::mem::size_of::<(u32, XrefStreamEntry, usize)>()) as u64
}

/// 全容量超過は公開エラーとして残り、不完全な本文集合を返さない。
fn assert_limit(budget: &DecodeBudget, limit: u64, actual: u64) {
    let error = budget.check();
    assert!(
        matches!(&error, Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit: found, actual: requested,
    }) if *found == limit && *requested == actual),
        "容量{limit}/{actual}: {error:?}"
    );
    assert!(budget.is_stopped());
}

/// 旧経路でも索引を確保する前に容量0を拒否する。
#[test]
fn compressed_storage_rejects_zero_before_index_allocation() {
    let bytes = pdf(b"<< /A 1 >>");
    let budget = budget(0);
    assert!(extract_compressed_object_bodies(&budget, &bytes).is_empty());
    assert_limit(
        &budget,
        0,
        (4 * std::mem::size_of::<(u32, XrefStreamEntry, usize)>()) as u64,
    );
    assert!(budget.compressed_objects(&bytes).is_none());
}

/// 独立した容量モデルの境界と、解放後の再作成を累積へ数える。
#[test]
fn compressed_storage_capacity_is_exact_and_cumulative() {
    let body = b"<< /A 1 >>";
    let bytes = pdf(body);
    let one = capacity(body.len());
    for limit in [one - 1, one, one * 2 - 1, one * 2] {
        let budget = budget(limit);
        let first = extract_compressed_object_bodies(&budget, &bytes);
        if limit < one {
            assert!(first.is_empty());
            assert_limit(&budget, limit, one);
            continue;
        }
        assert_eq!(&first.get(&7).unwrap()[..], body);
        drop(first);
        let second = extract_compressed_object_bodies(&budget, &bytes);
        if limit < one * 2 {
            assert!(second.is_empty());
            let actual = if limit == one {
                one + (4 * std::mem::size_of::<(u32, XrefStreamEntry, usize)>()) as u64
            } else {
                one * 2
            };
            assert_limit(&budget, limit, actual);
        } else {
            assert_eq!(&second.get(&7).unwrap()[..], body);
            budget.check().unwrap();
        }
    }
}

/// 消費済みの失敗後は本文集合の復元を再開しない。
#[test]
fn compressed_storage_stays_empty_after_consumed_failure() {
    let bytes = pdf(b"<<>>");
    let budget = budget(0);
    assert!(budget.metadata_vector::<u8>(1).is_none());
    assert_limit(&budget, 0, 1);
    assert!(extract_compressed_object_bodies(&budget, &bytes).is_empty());
    assert!(budget.check().is_ok());
}

/// 正式xrefはmember索引と本文だけを予約し、cache hitでは同じ本文を共有する。
#[test]
fn indexed_compressed_storage_capacity_and_cache_are_exact() {
    let bodies = vec![b"<< /A 1 >>".to_vec(), b"[1 2 3]".to_vec()];
    let bytes = members_pdf(&bodies, true);
    let prep = preparation_capacity(&bytes);
    let one = member_capacity(&bodies);
    for limit in [prep, prep + one - 1, prep + one] {
        let budget = budget(limit);
        prepare(&budget, &bytes);
        let first = extract_compressed_object_bodies(&budget, &bytes);
        if limit < prep + one {
            assert!(first.is_empty());
            let actual = if limit == prep {
                prep + (2 * std::mem::size_of::<(u32, u32, u32)>()) as u64
            } else {
                prep + one
            };
            assert_limit(&budget, limit, actual);
            assert!(budget.compressed_objects(&bytes).is_none());
        } else {
            let second = extract_compressed_object_bodies(&budget, &bytes);
            for (index, body) in bodies.iter().enumerate() {
                let number = 10 + index as u32;
                assert_eq!(&first.get(&number).unwrap()[..], body);
                assert!(Rc::ptr_eq(
                    first.get(&number).unwrap(),
                    second.get(&number).unwrap()
                ));
            }
            budget.check().unwrap();
            assert!(budget.metadata_vector::<u8>(1).is_none());
            assert_limit(&budget, limit, limit + 1);
            assert!(budget.compressed_objects(&bytes).is_none());
            assert!(extract_compressed_object_bodies(&budget, &bytes).is_empty());
        }
    }
}

/// 一部本文の保存後に止まっても、部分集合を公開・cacheしない。
#[test]
fn compressed_storage_does_not_publish_partial_bodies() {
    let bodies = vec![b"<<>>".to_vec(), b"[1]".to_vec()];
    let bytes = members_pdf(&bodies, true);
    let prep = preparation_capacity(&bytes);
    let actual = prep
        + (2 * (std::mem::size_of::<(u32, u32, u32)>() + std::mem::size_of::<(u32, Rc<Vec<u8>>)>())
            + 2 * (std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>())
            + bodies[0].len()) as u64;
    let budget = budget(actual - 1);
    prepare(&budget, &bytes);
    assert!(extract_compressed_object_bodies(&budget, &bytes).is_empty());
    assert_limit(&budget, actual - 1, actual);
    assert!(budget.compressed_objects(&bytes).is_none());
}

/// 旧xrefの複数memberでもowner本文の展開は一回だけ数える。
#[test]
fn legacy_members_share_one_owner_decode() {
    let bodies = vec![b"<< /A 1 >>".to_vec(), b"<< /B 2 >>".to_vec()];
    let bytes = members_pdf(&bodies, false);
    let payload_length = b"10 0 11 10 ".len() + bodies.iter().map(Vec::len).sum::<usize>();
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_decoded_bytes: payload_length as u64,
        ..Default::default()
    });
    let stored = extract_compressed_object_bodies(&budget, &bytes);
    assert_eq!(stored.iter().count(), 2);
    budget.check().unwrap();
}

/// 後続番号が不正でも境界offsetが有効なら前のmemberだけを復元する。
#[test]
fn malformed_member_number_does_not_discard_valid_neighbor() {
    let bodies = vec![b"<<>>".to_vec(), b"[1]".to_vec()];
    let bytes = members_pdf(&bodies, false);
    let text = String::from_utf8_lossy(&bytes);
    let offset = text.find("10 0 11 4 ").unwrap();
    let mut bytes = bytes;
    bytes[offset + 5..offset + 7].copy_from_slice(b"XX");
    let budget = DecodeBudget::default();
    let stored = extract_compressed_object_bodies(&budget, &bytes);
    assert_eq!(&stored.get(&10).unwrap()[..], bodies[0]);
    assert!(stored.get(&11).is_none());
    budget.check().unwrap();
}

/// 宣言Nに足りないheaderは、途中memberも公開しない。
#[test]
fn incomplete_member_header_returns_empty_storage() {
    let bodies = vec![b"<<>>".to_vec(), b"[1]".to_vec()];
    let bytes = members_pdf(&bodies, false);
    let text = String::from_utf8_lossy(&bytes);
    let offset = text.find("/N 2").unwrap();
    let mut bytes = bytes;
    bytes[offset + 3] = b'3';
    let budget = DecodeBudget::default();
    assert!(extract_compressed_object_bodies(&budget, &bytes).is_empty());
    budget.check().unwrap();
}

/// 内容が同じ別allocationの入力には元入力の本文cacheを流用しない。
#[test]
fn compressed_storage_cache_is_scoped_to_input_slice() {
    let bodies = vec![b"<<>>".to_vec()];
    let bytes = members_pdf(&bodies, true);
    let copied = bytes.clone();
    let used = preparation_capacity(&bytes) + member_capacity(&bodies);
    let next = (4 * std::mem::size_of::<(u32, XrefStreamEntry, usize)>()) as u64;
    let budget = budget(used + next - 1);
    prepare(&budget, &bytes);
    assert_eq!(
        extract_compressed_object_bodies(&budget, &bytes)
            .iter()
            .count(),
        1
    );
    assert!(budget.compressed_objects(&copied).is_none());
    assert!(extract_compressed_object_bodies(&budget, &copied).is_empty());
    assert_limit(&budget, used + next - 1, used + next);
}

/// 正式xrefも再検査でcacheを破棄し、同じ本文を再保存する全容量を数える。
#[test]
fn indexed_compressed_storage_recreation_is_cumulative() {
    let bodies = vec![b"<< /A 1 >>".to_vec(), b"[1]".to_vec()];
    let bytes = members_pdf(&bodies, true);
    let one = preparation_capacity(&bytes) + member_capacity(&bodies);
    for limit in [one * 2 - 1, one * 2] {
        let budget = budget(limit);
        prepare(&budget, &bytes);
        let first = extract_compressed_object_bodies(&budget, &bytes);
        assert_eq!(first.iter().count(), 2);
        drop(first);
        prepare(&budget, &bytes);
        assert!(budget.compressed_objects(&bytes).is_none());
        let second = extract_compressed_object_bodies(&budget, &bytes);
        if limit < one * 2 {
            assert!(second.is_empty());
            assert_limit(&budget, limit, one * 2);
        } else {
            assert_eq!(second.iter().count(), 2);
            budget.check().unwrap();
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// 件数・本文長を変え、旧xref拡張と各保存容量の独立モデルを照合する。
    #[test]
    fn legacy_compressed_storage_matches_capacity_model(lengths in prop::collection::vec(1usize..64, 1..9)) {
        let bodies = lengths.into_iter().enumerate().map(|(index, length)| vec![b'A' + index as u8; length]).collect::<Vec<_>>();
        let bytes = members_pdf(&bodies, false);
        let one = revision_capacity(bodies.len() + 2) + member_capacity(&bodies);
        for limit in [one - 1, one, one * 2 - 1, one * 2] {
            let budget = budget(limit);
            let first = extract_compressed_object_bodies(&budget, &bytes);
            if limit < one {
                prop_assert!(first.is_empty());
                assert_limit(&budget, limit, one);
                continue;
            }
            for (index, body) in bodies.iter().enumerate() {
                prop_assert_eq!(&first.get(&(10 + index as u32)).unwrap()[..], &body[..]);
            }
            if limit == one { continue; }
            drop(first);
            let second = extract_compressed_object_bodies(&budget, &bytes);
            if limit < one * 2 {
                prop_assert!(second.is_empty());
                assert_limit(&budget, limit, one * 2);
            } else {
                prop_assert_eq!(second.iter().count(), bodies.len());
                budget.check().unwrap();
            }
        }
    }

    /// 正式xrefの件数・本文長でも正確な保存容量と無確保のcache hitを照合する。
    #[test]
    fn indexed_compressed_storage_matches_capacity_model(lengths in prop::collection::vec(1usize..64, 1..9)) {
        let bodies = lengths.into_iter().enumerate().map(|(index, length)| vec![b'A' + index as u8; length]).collect::<Vec<_>>();
        let bytes = members_pdf(&bodies, true);
        let prep = preparation_capacity(&bytes);
        let one = prep + member_capacity(&bodies);
        for limit in [one - 1, one] {
            let budget = budget(limit);
            prepare(&budget, &bytes);
            let first = extract_compressed_object_bodies(&budget, &bytes);
            if limit < one {
                prop_assert!(first.is_empty());
                assert_limit(&budget, limit, one);
            } else {
                let second = extract_compressed_object_bodies(&budget, &bytes);
                for (index, body) in bodies.iter().enumerate() {
                    let number = 10 + index as u32;
                    prop_assert_eq!(&first.get(&number).unwrap()[..], &body[..]);
                    prop_assert!(Rc::ptr_eq(first.get(&number).unwrap(), second.get(&number).unwrap()));
                }
                budget.check().unwrap();
            }
        }
    }
}
