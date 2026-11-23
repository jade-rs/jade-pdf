//! 原本clusterの独立モデルで、通常文字・幅違い・合字のfont選択を検査する。
use super::*;
use crate::{
    ParseError, ParseLimits,
    type1_outline::{
        plan::{Code, Glyph},
        resource::Slot,
        unicode::Mapping,
    },
};
use proptest::prelude::*;

/// 輪郭を複製せず、所有テスト用の八GIDと未確認256codeを用意する。
fn plan() -> Plan {
    Plan {
        glyphs: (0..8)
            .map(|original| Glyph {
                original,
                pdf_width: None,
            })
            .collect(),
        codes: (0..256).map(|_| binding(None, None)).collect(),
    }
}

/// 元GIDと未正規化Unicodeを一codeに指定する。
fn binding(glyph: Option<u16>, scalars: Option<Vec<u32>>) -> Code {
    Code {
        mapping: Mapping {
            source: Slot {
                glyph,
                width: 500.,
                explicit_width: true,
            },
            scalars,
            from_tounicode: true,
        },
        glyph,
    }
}

/// 指定faceのcmapを独立に調べる。
fn lookup(face: &Face, scalar: u32) -> Option<u16> {
    face.mappings
        .iter()
        .find(|m| m.scalar == scalar)
        .map(|m| m.glyph)
}

/// 一つのffを、通常のf+fへ合字を適用せずに選べる。
#[test]
fn one_ff_glyph_never_installs_a_ligature_on_the_two_f_font() {
    let mut p = plan();
    p.codes[65] = binding(Some(1), Some(vec![102]));
    p.codes[66] = binding(Some(2), Some(vec![102, 102]));
    p.codes[67] = binding(Some(2), Some(vec![102, 102]));
    p.codes[68] = binding(Some(3), Some(vec![102, 102]));
    let result = build(&DecodeBudget::default(), &p).unwrap();
    assert_eq!(result.faces.len(), 3);
    assert_eq!(result.codes[65], Some(0));
    assert_eq!(result.codes[66], result.codes[67]);
    assert_ne!(result.codes[65], result.codes[66]);
    assert_ne!(result.codes[66], result.codes[68]);
    let plain = &result.faces[0];
    assert_eq!(lookup(plain, 102), Some(1));
    assert!(plain.components.is_empty());
    assert!(plain.ligature.is_none());
    for (code, glyph) in [(66, 2), (68, 3)] {
        let face = &result.faces[usize::from(result.codes[code].unwrap())];
        assert_eq!(face.components, [1, 1]);
        assert_eq!(face.ligature, Some(glyph));
        assert_eq!(lookup(face, 102), Some(1));
    }
}

/// 同じ一文字でもPDF幅に由来する別GIDを上書きせず、非BMPも保持する。
#[test]
fn conflicting_scalar_glyphs_get_distinct_direct_fonts() {
    let mut p = plan();
    for (code, glyph, scalar) in [
        (1, 1, 65),
        (2, 2, 65),
        (3, 2, 65),
        (4, 3, 0x1f600),
        (5, 4, 0x1f600),
    ] {
        p.codes[code] = binding(Some(glyph), Some(vec![scalar]));
    }
    let r = build(&DecodeBudget::default(), &p).unwrap();
    assert_eq!(r.faces.len(), 3);
    assert_eq!(r.codes[1], r.codes[4]);
    assert_ne!(r.codes[1], r.codes[2]);
    assert_eq!(r.codes[2], r.codes[3]);
    assert_ne!(r.codes[4], r.codes[5]);
    for (code, gid, scalar) in [
        (1, 1, 65),
        (2, 2, 65),
        (3, 2, 65),
        (4, 3, 0x1f600),
        (5, 4, 0x1f600),
    ] {
        let f = &r.faces[usize::from(r.codes[code].unwrap())];
        assert_eq!(lookup(f, scalar), Some(gid));
        assert!(f.components.is_empty());
        assert!(f.ligature.is_none());
    }
}

/// 未確認glyph・空文字・未知成分・複雑な文字処理を、別Unicodeに置換しない。
#[test]
fn unsupported_clusters_remain_unrouted_without_changing_source_text() {
    let mut p = plan();
    p.codes[65] = binding(Some(1), Some(vec![102]));
    for (code, entry) in [
        (0, binding(None, Some(vec![65]))),
        (1, binding(None, None)),
        (2, binding(Some(2), Some(vec![]))),
        (3, binding(Some(2), None)),
        (4, binding(Some(2), Some(vec![102, 105]))),
        (5, binding(Some(2), Some(vec![102, 0x301]))),
        (6, binding(Some(2), Some(vec![102, 0x200d, 102]))),
        (7, binding(Some(2), Some(vec![102; 33]))),
    ] {
        p.codes[code] = entry;
    }
    p.codes[8] = binding(Some(2), Some(vec![102; 32]));
    let r = build(&DecodeBudget::default(), &p).unwrap();
    for code in 0..8 {
        assert!(r.codes[code].is_none());
    }
    assert!(r.codes[8].is_some());
    assert_eq!(p.codes[4].mapping.scalars, Some(vec![102, 105]));
    assert_eq!(p.codes[5].mapping.scalars, Some(vec![102, 0x301]));
}

/// 不正な末尾・中止・予約不足で、先頭だけのfontを公開しない。
#[test]
fn malformed_or_stopped_plans_are_atomic() {
    for bad in [
        binding(Some(8), Some(vec![65])),
        binding(Some(0), Some(vec![65])),
        binding(Some(1), Some(vec![0xd800])),
    ] {
        let mut p = plan();
        p.codes[65] = binding(Some(1), Some(vec![65]));
        p.codes[255] = bad;
        assert!(build(&DecodeBudget::default(), &p).is_none());
    }
    let mut p = plan();
    p.codes.pop();
    assert!(build(&DecodeBudget::default(), &p).is_none());
    let stopped = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert!(stopped.font_vector::<u8>(1).is_none());
    assert!(build(&stopped, &plan()).is_none());
}

/// face header・cmap・成分列を必要byteだけ予約し、一byte不足を検出する。
#[test]
fn route_storage_is_exact_and_never_reserves_partial_extra_faces() {
    let mut p = plan();
    p.codes[65] = binding(Some(1), Some(vec![102]));
    p.codes[66] = binding(Some(2), Some(vec![102, 102]));
    let bytes = (2 * std::mem::size_of::<Face>()
        + 2 * std::mem::size_of::<cmap::Mapping>()
        + 2 * std::mem::size_of::<u16>()) as u64;
    for limit in [bytes, bytes - 1] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: limit,
            ..Default::default()
        });
        let r = build(&budget, &p);
        if limit == bytes {
            assert!(r.is_some());
            assert!(budget.check().is_ok());
        } else {
            assert!(r.is_none());
            assert!(
                matches!(budget.check(),Err(ParseError::ResourceLimit{resource:"embedded font bytes",limit:l,actual})if l==limit && actual==bytes)
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]
    /// 任意の同Unicode・別GID列を、正規化や最後のGIDへの上書きなしで選べる。
    #[test]
    fn arbitrary_original_clusters_retain_their_exact_output_glyph(entries in prop::collection::vec((1u16..8,prop::collection::vec(0u32..2,1..6)),1..120)) {
        let mut p=plan();p.codes[250]=binding(Some(1),Some(vec![97]));p.codes[251]=binding(Some(2),Some(vec![98]));
        for (code,(gid,scalars)) in entries.iter().enumerate() {p.codes[code]=binding(Some(*gid),Some(scalars.iter().map(|s|97+s).collect()));}
        let r=build(&DecodeBudget::default(),&p).unwrap();
        for (code,(expected,scalars)) in entries.iter().enumerate() {
            let face=&r.faces[usize::from(r.codes[code].unwrap())];
            let mapped=scalars.iter().map(|s|lookup(face,97+s).unwrap()).collect::<Vec<_>>();
            let shaped=if face.components.is_empty() {mapped} else {prop_assert_eq!(&mapped,&face.components);vec![face.ligature.unwrap()]};
            prop_assert_eq!(shaped,vec![*expected]);
            prop_assert_eq!(p.codes[code].mapping.scalars.as_ref().unwrap(),&scalars.iter().map(|s|97+s).collect::<Vec<_>>());
        }
    }
}
