//! Type1 charstringだけを読み、PostScriptや未知のOtherSubrsは実行しない。
use crate::decode::DecodeBudget;

/// 外形を保ったまま、font containerから独立して扱う絶対座標の描画命令。
#[derive(Debug, PartialEq)]
pub(super) enum Command {
    /// 新しい輪郭の始点。
    Move([f64; 2]),
    /// 直線の終点。
    Line([f64; 2]),
    /// 三次曲線の制御点二つと終点。
    Curve([f64; 6]),
    /// 輪郭を閉じる。Type1内部の相対座標原点は変えない。
    Close,
}

/// 完成した水平glyphのadvanceと外形。
#[derive(Debug, PartialEq)]
pub(super) struct Glyph {
    /// 1000単位座標系の水平advance。
    pub(super) advance: f64,
    /// 元の順序を保つ外形。途中失敗では公開しない。
    pub(super) commands: Vec<Command>,
}

/// 復号済みの一glyphを有限のstack・再帰・命令数で読む。
pub(super) fn decode(budget: &DecodeBudget, bytes: &[u8], subrs: &[Vec<u8>]) -> Option<Glyph> {
    let mut state = interpreter::State::new(budget);
    state.run(bytes, subrs, 0)?;
    state.finish()
}

#[path = "type1_outline_interpreter.rs"]
mod interpreter;

#[cfg(test)]
#[path = "type1_outline_comparison_tests.rs"]
mod comparison_tests;

#[cfg(test)]
#[path = "type1_outline_boundary_tests.rs"]
mod boundary_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, ParseLimits};
    use proptest::prelude::*;

    /// テスト側だけの整数encoderで、境界を跨ぐ任意の座標を作る。
    fn number(value: i32, out: &mut Vec<u8>) {
        match value {
            -107..=107 => out.push((value + 139) as u8),
            108..=1131 => {
                let n = value - 108;
                out.extend_from_slice(&[(247 + n / 256) as u8, (n % 256) as u8]);
            }
            -1131..=-108 => {
                let n = -value - 108;
                out.extend_from_slice(&[(251 + n / 256) as u8, (n % 256) as u8]);
            }
            _ => {
                out.push(255);
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
    }

    /// 指定幅と三角形外形を持つ独立したglyph programを作る。
    fn triangle(x: i32, y: i32, width: i32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for n in [0, width] {
            number(n, &mut bytes);
        }
        bytes.push(13);
        for n in [x, y] {
            number(n, &mut bytes);
        }
        bytes.push(21);
        for pair in [[100, 0], [0, 100], [-100, -100]] {
            for n in pair {
                number(n, &mut bytes);
            }
            bytes.push(5);
        }
        bytes.extend_from_slice(&[9, 14]);
        bytes
    }

    /// sidebearing・制御点・閉路・advanceを、それぞれ独立した座標値で検査する。
    #[test]
    fn width_and_cubic_geometry_survive() {
        let mut bytes = Vec::new();
        for n in [10, 20, 600, 0] {
            number(n, &mut bytes);
        }
        bytes.extend_from_slice(&[12, 7]);
        for n in [30, 40] {
            number(n, &mut bytes);
        }
        bytes.push(21);
        for n in [100, 0, 0, 200, -100, 0] {
            number(n, &mut bytes);
        }
        bytes.push(8);
        bytes.extend_from_slice(&[9, 14]);
        assert_eq!(
            decode(&DecodeBudget::default(), &bytes, &[]),
            Some(Glyph {
                advance: 600.0,
                commands: vec![
                    Command::Move([40., 60.]),
                    Command::Curve([140., 60., 140., 260., 40., 260.]),
                    Command::Close
                ]
            })
        );
    }

    /// subroutineのoperand stackを共有し、divを位置命令から区別する。
    #[test]
    fn subroutine_and_div_keep_the_source_path() {
        let bytes = [139, 248, 236, 141, 12, 12, 13, 139, 139, 21, 139, 10, 14];
        let subrs = vec![vec![239, 139, 5, 139, 239, 5, 11]];
        assert_eq!(
            decode(&DecodeBudget::default(), &bytes, &subrs),
            Some(Glyph {
                advance: 300.0,
                commands: vec![
                    Command::Move([0., 0.]),
                    Command::Line([100., 0.]),
                    Command::Line([100., 100.]),
                    Command::Close
                ]
            })
        );
    }

    /// 再帰・未知PostScript連携・operand不足・未終了・余分なreturnを完了結果にしない。
    #[test]
    fn invalid_or_dynamic_programs_are_not_guessed() {
        for bytes in [
            vec![139, 10, 14],
            vec![139, 139, 12, 16, 14],
            vec![139, 13, 14],
            vec![139, 248, 236, 13],
            vec![11],
        ] {
            assert!(decode(&DecodeBudget::default(), &bytes, &[vec![139, 10, 11]]).is_none());
        }
        let target = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: 0,
            ..Default::default()
        });
        assert!(decode(&target, &triangle(0, 0, 600), &[]).is_none());
        assert!(matches!(
            target.check(),
            Err(ParseError::ResourceLimit {
                resource: "embedded font bytes",
                ..
            })
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 整数の符号・圧縮表現・平行移動にかかわらず、元のadvanceと頂点を保持する。
        #[test]
        fn translated_triangles_keep_advance_and_vertices(x in -2000_i32..2000,y in -2000_i32..2000,width in 1_i32..4000) {
            let decoded=decode(&DecodeBudget::default(),&triangle(x,y,width),&[]).unwrap();
            prop_assert_eq!(decoded.advance,f64::from(width));
            prop_assert_eq!(decoded.commands,vec![Command::Move([f64::from(x),f64::from(y)]),Command::Line([f64::from(x+100),f64::from(y)]),Command::Line([f64::from(x+100),f64::from(y+100)]),Command::Line([f64::from(x),f64::from(y)]),Command::Close]);
        }
    }
}
