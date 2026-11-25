//! privateなSDK識別候補の入口で、未確認・曖昧・未埋込fontを除く契約。
use super::*;
use crate::{ParseLimits, object_index};
use proptest::prelude::*;

/// 同じsubset名とdescriptorを指定するsimple Type1辞書。
const FONT: &str =
    "<< /Type /Font /Subtype /Type1 /BaseFont /ABCDEF+Owned /FontDescriptor 5 0 R >>";
/// 幅や名前の推測に依存しない、明示700の非symbolic descriptor。
const DESCRIPTOR: &str = "<< /Type /FontDescriptor /FontName /ABCDEF+Owned /Flags 32 /FontWeight 700 /FontFile3 6 0 R >>";
/// metadata候補だけの所有stream。描画可能なfontとしては採用せず、後段のSDK確認が必要。
const FILE: &str = "<< /Subtype /Type1C /Length 5 >>\nstream\nowned\nendstream";

/// 参照・xref・stream境界を持つ所有PDFをメモリに作り、外部fontを複製しない。
fn source(font: &str, descriptor: &str, file: &str, scalar: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 4 0 R >> >> /Contents 7 0 R >>",
        font,
        descriptor,
        file,
        "<< /Length 0 >>\nstream\n\nendstream",
        scalar,
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 9\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 9 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// 借用した候補を、テストでのみ所有する値へ移す。
fn candidate(font: &str, descriptor: &str, file: &str, scalar: &str) -> Option<(String, u16)> {
    let data = source(font, descriptor, file, scalar);
    let budget = DecodeBudget::new(&ParseLimits::default());
    let objects = object_index::build(&budget, &data).unwrap();
    let mut resolver = ResourceResolver::new(&objects, &budget);
    let result = read(&mut resolver, objects.get(&4).unwrap().1, &budget)?;
    Some((result.native_name.into(), result.weight))
}

/// direct値、数値参照、名前参照を同じ候補へ対応させる。
#[test]
fn native_subset_eligible_weight_and_restored_name_are_explicit() {
    assert_eq!(
        candidate(FONT, DESCRIPTOR, FILE, "null"),
        Some(("Owned".into(), 700))
    );
    assert_eq!(
        candidate(
            FONT,
            &DESCRIPTOR.replace("/FontWeight 700", "/FontWeight 8 0 R"),
            FILE,
            "500"
        ),
        Some(("Owned".into(), 500))
    );
    assert_eq!(
        candidate(
            &FONT.replace("/ABCDEF+Owned", "8 0 R"),
            &DESCRIPTOR.replace("/ABCDEF+Owned", "8 0 R"),
            FILE,
            "/ABCDEF+Owned"
        ),
        Some(("Owned".into(), 700))
    );
}

/// 他種font、未埋込・別program・symbolic・subsetでない名前を候補にしない。
#[test]
fn native_subset_rejects_unembedded_or_other_font_kinds() {
    for subtype in ["TrueType", "Type0", "Type3"] {
        assert!(
            candidate(
                &FONT.replace("/Subtype /Type1", &format!("/Subtype /{subtype}")),
                DESCRIPTOR,
                FILE,
                "null"
            )
            .is_none()
        );
    }
    for desc in [
        DESCRIPTOR.replace(" /FontFile3 6 0 R", ""),
        DESCRIPTOR.replace("/FontFile3 6 0 R", "/FontFile3 null"),
        DESCRIPTOR.replace("/Flags 32", "/Flags 4"),
        DESCRIPTOR.replace("/Flags 32", "/Flags 0"),
        DESCRIPTOR.replace("/FontFile3 6 0 R", "/FontFile 6 0 R /FontFile3 6 0 R"),
        DESCRIPTOR.replace("/FontFile3 6 0 R", "/FontFile2 6 0 R /FontFile3 6 0 R"),
    ] {
        assert!(candidate(FONT, &desc, FILE, "null").is_none(), "{desc}");
    }
    for name in [
        "Owned",
        "abcdef+Owned",
        "ABCDE+Owned",
        "ABCDEFG+Owned",
        "ABCDEF+",
        "ABCDEF+Symbol",
        "ABCDEF+ZapfDingbats",
        "ABCDEF+Own#65d",
    ] {
        assert!(
            candidate(
                &FONT.replace("ABCDEF+Owned", name),
                &DESCRIPTOR.replace("ABCDEF+Owned", name),
                FILE,
                "null"
            )
            .is_none(),
            "{name}"
        );
    }
    for file in [
        "null",
        "<< /Subtype /Type1C /Length 5 >>",
        "<< /Subtype /OpenType /Length 5 >>\nstream\nowned\nendstream",
    ] {
        assert!(candidate(FONT, DESCRIPTOR, file, "null").is_none());
    }
}

/// 欠落・型違い・不一致・重複の値は、いずれも確認済みweightへ変換しない。
#[test]
fn native_subset_rejects_unconfirmed_and_ambiguous_metadata() {
    for value in [
        "0", "-1", "1001", "700.5", "null", "/Bold", "[700]", "8 0 R",
    ] {
        assert!(
            candidate(
                FONT,
                &DESCRIPTOR.replace("/FontWeight 700", &format!("/FontWeight {value}")),
                FILE,
                "null"
            )
            .is_none()
        );
    }
    for descriptor in [
        DESCRIPTOR.replace("/FontWeight 700", ""),
        DESCRIPTOR.replace("/FontName /ABCDEF+Owned", ""),
        DESCRIPTOR.replace("/FontName /ABCDEF+Owned", "/FontName /ABCDEF+Other"),
        DESCRIPTOR.replace("/FontWeight 700", "/FontWeight 400 /FontWeight 700"),
        DESCRIPTOR.replace(
            "/FontName /ABCDEF+Owned",
            "/FontName /ABCDEF+Owned /FontName /ABCDEF+Owned",
        ),
        DESCRIPTOR.replace("/Flags 32", "/Flags 32 /Flags 32"),
        DESCRIPTOR.replace("/FontFile3 6 0 R", "/FontFile3 6 0 R /FontFile3 6 0 R"),
    ] {
        assert!(
            candidate(FONT, &descriptor, FILE, "null").is_none(),
            "{descriptor}"
        );
    }
    assert!(
        candidate(
            &FONT.replace(
                "/BaseFont /ABCDEF+Owned",
                "/BaseFont /ABCDEF+Owned /BaseFont /ABCDEF+Owned"
            ),
            DESCRIPTOR,
            FILE,
            "null"
        )
        .is_none()
    );
    assert!(
        candidate(
            FONT,
            DESCRIPTOR,
            &FILE.replace("/Subtype /Type1C", "/Subtype /Type1C /Subtype /Type1C"),
            "null"
        )
        .is_none()
    );
}

/// 停止済み予算を候補が復活させず、呼出元が失敗を確認できる。
#[test]
fn native_subset_stopped_budget_does_not_confirm_weight() {
    let data = source(FONT, DESCRIPTOR, FILE, "null");
    let budget = DecodeBudget::new(&ParseLimits::default());
    let objects = object_index::build(&budget, &data).unwrap();
    let mut resolver = ResourceResolver::new(&objects, &budget);
    budget.charge_metadata_workspace_bytes(u64::MAX);
    assert!(read(&mut resolver, objects.get(&4).unwrap().1, &budget).is_none());
    assert!(budget.check().is_err());
}

proptest! {
    #![proptest_config(ProptestConfig {cases:24,failure_persistence:Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct("src/pdfium_subset_weight.proptest-regressions"))),..Default::default()})]
    /// 同じfont名と有効weightを、直接値・参照で変えても候補の意味を保つ。
    #[test]
    fn native_subset_candidate_is_invariant_under_weight_reference(weight in 1u16..=1000, family in "[A-Za-z0-9_-]{1,20}", indirect in any::<bool>()) {
        let name=format!("Owned{family}");let font=FONT.replace("Owned",&name);let desc=DESCRIPTOR.replace("Owned",&name).replace("/FontWeight 700",&if indirect{"/FontWeight 8 0 R".into()}else{format!("/FontWeight {weight}")});
        prop_assert_eq!(candidate(&font,&desc,FILE,&weight.to_string()),Some((name,weight)));
    }
}
