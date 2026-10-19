#![cfg(feature = "ndlocr-lite")]

use ndlocr_lite_rs::pipeline::background::normalize_chromatic_panels;

#[test]
/// 公開libraryで色パネルの補間外縁を暗い縦線へ変えない。
fn public_library_removes_lateral_fringe() {
    let (width, height) = (400, 160);
    let mut rgb = vec![255; width * height * 3];
    for y in 16..144 {
        let step = y / 32;
        for x in 32 + step..368 + step {
            rgb[(y * width + x) * 3..(y * width + x) * 3 + 3].copy_from_slice(&[255, 165, 0]);
        }
        for x in [31 + step, 368 + step] {
            let white = ((y * 17) % 220) as u16;
            let color = [255u16, 165, 0].map(|c| (c + (255 - c) * white / 255) as u8);
            rgb[(y * width + x) * 3..(y * width + x) * 3 + 3].copy_from_slice(&color);
        }
    }
    let out = normalize_chromatic_panels(&rgb, width, height).unwrap();
    for y in 18..142 {
        for x in [31 + y / 32, 368 + y / 32] {
            assert!(
                out[(y * width + x) * 3..(y * width + x) * 3 + 3]
                    .iter()
                    .map(|&v| u16::from(v))
                    .sum::<u16>()
                    >= 660
            );
        }
    }
}
