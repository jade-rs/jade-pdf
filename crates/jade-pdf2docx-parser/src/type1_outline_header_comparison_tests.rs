//! 元FontFileのheaderをproductで読み、独立した原本解析結果と比較する。
use super::*;
use crate::type1_outline::header;
use std::collections::BTreeMap;

/// 独立Type1解析器がboolを0/1として保存した場合も、値の型を検査して比較する。
fn reference_bool(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(value) => Some(*value),
        serde_json::Value::Number(value) => match value.as_i64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => panic!("independent reference bool must be 0 or 1"),
        },
        _ => panic!("independent reference bool has an invalid type"),
    }
}

/// 固定した独立解析器による原本headerの値。
#[derive(Deserialize)]
pub(super) struct Metadata {
    /// 元のFontName。
    name: String,
    /// 元のFontMatrix。
    matrix: [f64; 6],
    /// 元のFontBBox。
    bbox: [f64; 4],
    /// 未展開PostScript文字列のhex表現。
    info_strings_hex: BTreeMap<String, Option<String>>,
    /// 元の数値・boolと指定の有無。
    info_numbers: BTreeMap<String, serde_json::Value>,
    /// 元の256slot Encoding。
    encoding: Vec<String>,
}

/// Rustで読むのは元FontFileのcleartextだけであり、独立metadataは比較にしか使わない。
#[test]
#[ignore = "reviewed original Type1 header gold requires JADE_TYPE1_OUTLINE_GOLD"]
fn original_headers_match_independent_metadata_and_encoding() {
    let input = std::env::var("JADE_TYPE1_OUTLINE_GOLD").unwrap();
    let bytes = std::fs::read(input).unwrap();
    assert!(bytes.len() <= 8 * 1024 * 1024);
    let gold: Gold = serde_json::from_slice(&bytes).unwrap();
    let mut total = 0;
    for font in gold.fonts {
        let expected = font.header.as_ref().unwrap();
        let budget = DecodeBudget::default();
        let actual = header::parse(&budget, &font.fontfile[..font.length1])
            .expect("original static header with fully reviewed source metadata");
        assert_eq!(
            actual.name,
            expected.name.as_bytes(),
            "{}",
            font.source_name
        );
        assert_eq!(actual.matrix, expected.matrix, "{}", font.source_name);
        assert_eq!(actual.bbox, expected.bbox, "{}", font.source_name);
        for (key, value) in [
            ("Notice", actual.info.notice),
            ("Copyright", actual.info.copyright),
            ("version", actual.info.version),
            ("FullName", actual.info.full_name),
            ("FamilyName", actual.info.family),
            ("Weight", actual.info.weight),
        ] {
            let hex =
                value.map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
            assert_eq!(
                hex, expected.info_strings_hex[key],
                "{} {key}",
                font.source_name
            );
        }
        for (key, value) in [
            ("ItalicAngle", actual.info.italic_angle),
            ("UnderlinePosition", actual.info.underline_position),
            ("UnderlineThickness", actual.info.underline_thickness),
            ("ascent", actual.info.ascent),
            ("descent", actual.info.descent),
        ] {
            assert_eq!(
                value,
                expected.info_numbers[key].as_f64(),
                "{} {key}",
                font.source_name
            );
        }
        assert_eq!(
            actual.info.fixed,
            reference_bool(&expected.info_numbers["isFixedPitch"])
        );
        assert_eq!(
            actual.info.fs_type.map(u64::from),
            expected.info_numbers["FSType"].as_u64()
        );
        assert_eq!(actual.encoding.base, crate::type1_program::Base::Custom);
        assert_eq!(expected.encoding.len(), 256);
        for (code, expected) in expected.encoding.iter().enumerate() {
            assert_eq!(
                actual.encoding.slots[code].unwrap_or(b".notdef"),
                expected.as_bytes(),
                "{} {code}",
                font.source_name
            );
        }
        assert!(budget.check().is_ok());
        println!(
            "{}: metadata and all256Encoding slots match; FSType={:?}",
            font.source_name, actual.info.fs_type
        );
        total += 1;
    }
    assert_eq!(total, 11);
}
