//! 確認済み外部資料の独立解析結果だけを、明示的な環境変数で比較する。
use super::*;
use serde::Deserialize;

/// 独立gold全体。
#[derive(Deserialize)]
struct Gold {
    /// 原本ごとのglyphとsubroutine。
    fonts: Vec<Font>,
}
/// 一つの原本fontから得た値。
#[derive(Deserialize)]
struct Font {
    /// 失敗時に識別する原本名。
    source_name: String,
    /// fontToolsが独立復号したType1 subroutine。
    subrs: Vec<Vec<u8>>,
    /// 外形とadvanceの独立gold。
    glyphs: Vec<Reference>,
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
    let path = std::env::var("JADE_TYPE1_OUTLINE_GOLD").expect("explicit reviewed gold path");
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() <= 8 * 1024 * 1024);
    let gold: Gold = serde_json::from_slice(&bytes).unwrap();
    let mut total_accepted = 0;
    for font in gold.fonts {
        let (mut accepted, mut rejected) = (0, 0);
        for glyph in font.glyphs {
            let Some(actual) = decode(&DecodeBudget::default(), &glyph.program, &font.subrs) else {
                rejected += 1;
                continue;
            };
            let context = format!("{} {}", font.source_name, glyph.name);
            assert!((actual.advance - glyph.advance).abs() < 1e-7, "{context}");
            assert_eq!(actual.commands.len(), glyph.commands.len(), "{context}");
            for (actual, expected) in actual.commands.iter().zip(&glyph.commands) {
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
            accepted += 1;
        }
        println!(
            "{}: matched={accepted}, unsupported={rejected}",
            font.source_name
        );
        total_accepted += accepted;
    }
    assert!(
        total_accepted > 0,
        "a comparison of zero accepted glyphs is insufficient"
    );
}
