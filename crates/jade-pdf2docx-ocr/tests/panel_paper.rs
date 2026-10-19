#![cfg(feature = "ndlocr-lite")]

use ndlocr_lite_rs::pipeline::background::normalize_chromatic_panels;

#[test]
/// 公開library pinが、凹形の色背景の外側白紙と内側白文字を区別する。
fn public_library_preserves_exterior_paper() {
    let (w, h) = (400, 96);
    let mut rgb = vec![255; w * h * 3];
    for y in 8..88 {
        for x in 16..384 {
            if !(36..72).contains(&y) || x >= 140 || (y < 60 && x < 32) {
                rgb[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&[255, 165, 0]);
            }
        }
    }
    for y in 40..56 {
        for x in 260..268 {
            rgb[(y * w + x) * 3..(y * w + x) * 3 + 3].fill(255);
        }
    }
    let out = normalize_chromatic_panels(&rgb, w, h).unwrap();
    assert_eq!(&out[(48 * w + 64) * 3..(48 * w + 64) * 3 + 3], &[255; 3]);
    assert_eq!(&out[(48 * w + 263) * 3..(48 * w + 263) * 3 + 3], &[0; 3]);
}
