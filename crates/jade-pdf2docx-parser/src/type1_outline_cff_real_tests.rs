//! 実数operandの値を、指数付き小数を扱う既存consumerでも保持する数値回帰。
use super::Dict;
use proptest::prelude::*;

/// 実際のDICT writerから数値operandを一つ取り出す。
fn operand(value: f64) -> Vec<u8> {
    let mut dict = Dict::new();
    dict.real(value).unwrap();
    dict.as_slice().to_vec()
}

/// Adobe CFFのnibble表に従い、生成器とは独立に十進数文字列へ復元する。
fn decimal(bytes: &[u8]) -> String {
    assert_eq!(bytes[0], 30);
    let mut text = String::new();
    for byte in &bytes[1..] {
        for nibble in [byte >> 4, byte & 15] {
            match nibble {
                0..=9 => text.push(char::from(b'0' + nibble)),
                10 => text.push('.'),
                11 => text.push('E'),
                12 => text.push_str("E-"),
                14 => text.push('-'),
                15 => return text,
                _ => panic!("reserved CFF real nibble"),
            }
        }
    }
    panic!("missing CFF real terminator")
}

/// 実描画で観測した、指数桁数も小数桁へ加算するconsumerの数値誤差をモデル化する。
/// 完全なLibreOffice実装ではなく、標準decimal readerとの差だけを独立して表す。
fn observed_consumer(text: &str) -> f64 {
    let value = text.parse::<f64>().unwrap();
    if let Some((mantissa, exponent)) = text.split_once('E')
        && mantissa.contains('.')
    {
        let digits = exponent.trim_start_matches(['-', '+']).len() as i32;
        return value / 10f64.powi(digits);
    }
    value
}

/// 元glyphの座標は変更せず、実数の表記によってFontMatrixや傾斜角が1/10にならない。
#[test]
fn cff_real_operands_keep_scale_in_decimal_consumers() {
    for value in [1. / 16360., 1. / 15000., -9.46, 1. / 10000., 0., -0.] {
        let text = decimal(&operand(value));
        assert_eq!(text.parse::<f64>().unwrap().to_bits(), value.to_bits());
        let actual = observed_consumer(&text);
        assert!(
            (actual - value).abs() <= value.abs() * 1e-12 + 1e-15,
            "CFF real operand must preserve consumer scale: {value} encoded as {text}, read as {actual}"
        );
    }
}

/// 対応する全em単位数で、実数表記によるFontMatrixの桁ずれがない。
#[test]
fn every_supported_em_preserves_font_matrix_scale() {
    for units in 16u16..=16384 {
        let scale = 1. / f64::from(units);
        let text = decimal(&operand(scale));
        let actual = observed_consumer(&text);
        assert!(
            (actual * f64::from(units) - 1.).abs() < 1e-12,
            "em={units}: {text}"
        );
        assert_eq!(text.parse::<f64>().unwrap().to_bits(), scale.to_bits());
    }
}

proptest! {
    /// 任意の有限f64で有効数字・符号・指数を失わず、特定font値だけの回避策にしない。
    #[test]
    fn finite_real_bit_patterns_round_trip_without_consumer_scale_loss(bits in any::<u64>()) {
        let value = f64::from_bits(bits);
        prop_assume!(value.is_finite());
        let bytes = operand(value);
        let text = decimal(&bytes);
        prop_assert_eq!(text.parse::<f64>().unwrap().to_bits(), bits);
        prop_assert_eq!(observed_consumer(&text).to_bits(), bits);
        prop_assert!(bytes.len() <= 32);
    }
}
