//! 確認済み外部資料の独立解析結果だけを、明示的な環境変数で比較する。
use super::*;
use serde::Deserialize;
#[path = "type1_outline_cff_export_tests.rs"]
mod cff_export;
#[path = "type1_outline_header_comparison_tests.rs"]
mod header_comparison;
#[path = "type1_outline_otf_export_tests.rs"]
mod otf_export;
#[path = "type1_outline_resource_comparison_tests.rs"]
mod resource_comparison;
#[path = "type1_outline_type2_export_tests.rs"]
mod type2_export;

/// 独立gold全体。
#[derive(Deserialize)]
struct Gold {
    /// 原本PDFも直接読む検査で、別ファイルへの差替えを防ぐhash。
    #[serde(default)]
    source_pdf_sha256: Option<String>,
    /// 原本ごとのglyphとsubroutine。
    fonts: Vec<Font>,
    /// 所有の端数glyphに対する、独立して宣言したmetric。
    #[serde(default)]
    owned_otf: Option<otf_export::Owned>,
}
/// 一つの原本fontから得た値。
#[derive(Deserialize)]
struct Font {
    /// 失敗時に識別する原本名。
    source_name: String,
    /// 元のPrivateから独立復号したOtherSubrsの完全な配列定義。
    #[serde(default)]
    other_subrs: Vec<u8>,
    /// 元PDFのstreamから展開した、未変更のFontFile。
    #[serde(default)]
    fontfile: Vec<u8>,
    /// 元のcleartext byte長。
    #[serde(default)]
    length1: usize,
    /// 元のeexec byte長。
    #[serde(default)]
    length2: usize,
    /// 独立に読んだ元のmetadataと、予約名を使わない派生font名。
    #[serde(default)]
    cff: Option<CffMetadata>,
    /// 独立に読んだsource line metric・glyph bounds・Unicode対応。
    #[serde(default)]
    otf: Option<otf_export::Metadata>,
    /// 元PDF幅を使う、独立して宣言した単位変換の検査条件。
    #[serde(default)]
    pdf_scale: Option<otf_export::ScaleProfile>,
    /// 元FontFileから独立して読んだcleartext辞書。
    #[serde(default)]
    header: Option<header_comparison::Metadata>,
    /// 元PDF辞書と元Encodingから独立して得たcode単位の期待値。
    #[serde(default)]
    pdf_resource: Option<resource_comparison::Metadata>,
    /// fontToolsが独立復号したType1 subroutine。
    subrs: Vec<Vec<u8>>,
    /// 外形とadvanceの独立gold。
    glyphs: Vec<Reference>,
}

/// containerの検査へ渡す、一次資料を保持したmetadata。
#[derive(Deserialize)]
struct CffMetadata {
    /// 既存fixtureは1000。小数幅の検証では明示的に細かい単位数を使う。
    #[serde(default = "default_units")]
    units_per_em: u16,
    /// 派生fontのPostScript名。
    name: String,
    /// 元のnoticeと許諾を省略しないbyte列。
    notice: Vec<u8>,
    /// 元の外形範囲。
    bbox: [i16; 4],
    /// 元の傾斜角度。
    italic_angle: f64,
    /// 元の等幅情報。
    is_fixed_pitch: bool,
}

/// 既存の独立goldを変更せず、従来の座標単位として読む。
fn default_units() -> u16 {
    1000
}
/// 一glyphの元programと、独立した解析結果。
#[derive(Deserialize)]
struct Reference {
    /// 原本glyph名。
    name: String,
    /// fontToolsが独立復号したType1 charstring。
    program: Vec<u8>,
    /// 元のadvance。
    advance: f64,
    /// 独立した描画命令。
    commands: Vec<ReferenceCommand>,
}
/// 相互比較に必要な最小の描画命令。
#[derive(Deserialize)]
struct ReferenceCommand {
    /// move・line・curve・close、または未対応のcomponent。
    op: String,
    /// 絶対座標にした制御点と終点。
    points: Vec<f64>,
}

/// 独立goldの座標を変えずに比較し、未対応glyph数も隠さず出力する。
#[test]
#[ignore = "rights-reviewed external fontTools gold requires JADE_TYPE1_OUTLINE_GOLD"]
fn external_charstrings_match_independent_geometry() {
    compare(false, false);
}

/// 標準定義を照合できれば、未対応157文字も含めて全glyphの元外形に一致する。
#[test]
#[ignore = "rights-reviewed external fontTools gold with OtherSubrs requires JADE_TYPE1_OUTLINE_GOLD"]
fn external_verified_standard_subrs_match_all_geometry() {
    compare(true, false);
}

/// 元FontFileからRust自身で二層復号し、独立glyph goldと同じ文字辞書を得る。
#[test]
#[ignore = "rights-reviewed original FontFile gold requires JADE_TYPE1_OUTLINE_GOLD"]
fn external_original_fontfile_reaches_every_source_charstring() {
    compare(true, true);
}

/// 同一goldの幅と制御点を、厳密モードまたは標準定義承認モードで比較する。
fn compare(standard: bool, original: bool) {
    let path = std::env::var("JADE_TYPE1_OUTLINE_GOLD").expect("explicit reviewed gold path");
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() <= 8 * 1024 * 1024);
    let gold: Gold = serde_json::from_slice(&bytes).unwrap();
    let mut total_accepted = 0;
    for font in gold.fonts {
        if original {
            drop(compare_original(&font));
        }
        let verified = standard.then(|| {
            let (verified, end) =
                crate::type1_outline::standard::verify(&DecodeBudget::default(), &font.other_subrs)
                    .expect("complete standard OtherSubrs definition");
            assert_eq!(end, font.other_subrs.len());
            let mut trivia = b"% irrelevant prefix\n ".to_vec();
            for &byte in &font.other_subrs {
                trivia.push(byte);
                if byte == b'\n' {
                    trivia.extend_from_slice(b"% benign comment\n");
                }
            }
            trivia.extend_from_slice(b" /not-part-of-array");
            let (spaced, boundary) =
                crate::type1_outline::standard::verify(&DecodeBudget::default(), &trivia).unwrap();
            assert_eq!(spaced.allows_flex(), verified.allows_flex());
            assert_eq!(&trivia[boundary..], b" /not-part-of-array");
            let position = font
                .other_subrs
                .iter()
                .position(u8::is_ascii_alphabetic)
                .unwrap();
            let mut changed = font.other_subrs.clone();
            changed[position] ^= 32;
            assert!(
                crate::type1_outline::standard::verify(&DecodeBudget::default(), &changed)
                    .is_none()
            );
            let empty = DecodeBudget::new(&crate::ParseLimits {
                max_total_embedded_font_bytes: 0,
                max_total_allocation_bytes: 0,
                ..Default::default()
            });
            assert!(crate::type1_outline::standard::verify(&empty, &font.other_subrs).is_some());
            verified
        });
        let (mut accepted, mut rejected) = (0, 0);
        for glyph in font.glyphs {
            let result = match verified {
                Some(verified) => decode_verified(
                    &DecodeBudget::default(),
                    &glyph.program,
                    &font.subrs,
                    verified,
                ),
                None => decode(&DecodeBudget::default(), &glyph.program, &font.subrs),
            };
            let Some(actual) = result else {
                rejected += 1;
                continue;
            };
            let context = format!("{} {}", font.source_name, glyph.name);
            assert_geometry(&actual, &glyph, &context);
            accepted += 1;
        }
        println!(
            "{}: matched={accepted}, unsupported={rejected}",
            font.source_name
        );
        total_accepted += accepted;
        if standard {
            assert_eq!(
                rejected, 0,
                "{} must preserve every original glyph",
                font.source_name
            );
        }
    }
    assert!(
        total_accepted > 0,
        "a comparison of zero accepted glyphs is insufficient"
    );
}

/// 幅・輪郭命令・全制御点を、それぞれ独立goldと照合する。
fn assert_geometry(actual: &Glyph, expected: &Reference, context: &str) {
    assert!(
        (actual.advance - expected.advance).abs() < 1e-7,
        "{context}"
    );
    assert_eq!(actual.commands.len(), expected.commands.len(), "{context}");
    for (actual, expected) in actual.commands.iter().zip(&expected.commands) {
        let (op, points): (&str, &[f64]) = match actual {
            Command::Move(p) => ("Move", p),
            Command::Line(p) => ("Line", p),
            Command::Curve(p) => ("Curve", p),
            Command::Close => ("Close", &[]),
        };
        assert_eq!(op, expected.op, "{context}");
        assert_eq!(points.len(), expected.points.len(), "{context}");
        for (a, b) in points.iter().zip(&expected.points) {
            assert!(
                (a - b).abs() < 1e-7,
                "{context}: {actual:?} vs {:?}",
                expected.points
            );
        }
    }
}

/// 独立復号値をproduct入力として使わず、元のstreamから同じglyphとsubroutineを得る。
fn compare_original(font: &Font) -> Vec<(String, Glyph)> {
    use crate::type1_outline::{crypt, private};
    let budget = DecodeBudget::default();
    let decrypted = crypt::decrypt(
        &budget,
        &font.fontfile[font.length1..font.length1 + font.length2],
        55665,
        4,
    )
    .unwrap();
    let parsed =
        private::parse(&budget, &decrypted).expect("original static Type1 Private dictionary");
    assert_eq!(parsed.glyphs.len(), font.glyphs.len());
    assert_eq!(parsed.subrs.len(), font.subrs.len());
    let mut subrs = budget.font_vector(parsed.subrs.len()).unwrap();
    for (actual, expected) in parsed.subrs.iter().zip(&font.subrs) {
        let bytes = crypt::decrypt(&budget, actual.unwrap(), 4330, parsed.prefix.unwrap()).unwrap();
        assert_eq!(&bytes, expected);
        subrs.push(bytes);
    }
    let mut glyphs = Vec::new();
    for expected in &font.glyphs {
        let actual = parsed
            .glyphs
            .iter()
            .find(|g| g.name == expected.name.as_bytes())
            .unwrap();
        let bytes = crypt::decrypt(&budget, actual.bytes, 4330, parsed.prefix.unwrap()).unwrap();
        assert_eq!(
            bytes, expected.program,
            "{} {}",
            font.source_name, expected.name
        );
        let glyph = match parsed.standard {
            Some(verified) => decode_verified(&budget, &bytes, &subrs, verified),
            None => decode(&budget, &bytes, &subrs),
        }
        .expect("original Rust-decrypted glyph must have drawable geometry");
        assert_geometry(
            &glyph,
            expected,
            &format!("original {} {}", font.source_name, expected.name),
        );
        glyphs.push((expected.name.clone(), glyph));
    }
    assert!(budget.check().is_ok());
    glyphs
}
