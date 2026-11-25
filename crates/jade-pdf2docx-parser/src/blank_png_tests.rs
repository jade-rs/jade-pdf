//! 白一色の画像判定を、parser 自身の PNG 符号化結果で確認する。
use super::*;

/// RGB 画素列を PNG にする。
fn png(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let budget = DecodeBudget::new(&ParseLimits::default());
    encode_rgb8_png(&budget, width, height, rgb).expect("encode")
}

/// 白一色の背景画像を空白とみなす。
#[test]
fn white_image_is_blank() {
    assert!(png_is_blank_white(&png(4, 3, &[255; 36])));
}

/// 1 画素でも描画があれば空白とみなさない。
#[test]
fn image_with_ink_is_not_blank() {
    let mut pixels = vec![255; 36];
    pixels[16] = 0;
    assert!(!png_is_blank_white(&png(4, 3, &pixels)));
}

/// 解読できないバイト列は内容ありとして残す。
#[test]
fn undecodable_bytes_are_not_blank() {
    assert!(!png_is_blank_white(b"not a png"));
}
