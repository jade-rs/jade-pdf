//! 独立した横並び画像を縦積みせず、同じページ原点の座標と本文を保持する。
use super::*;

/// 本文は一列に固定し、非重複の横並び二画像と独立した第三画像を所有IRへ置く。
fn image_row() -> ParsedDocument {
    let mut parsed = source(2, &[0, 0, 0]);
    parsed
        .text_elements
        .retain(|text| text.text.contains("LEFT"));
    for (image, (x, y)) in
        parsed
            .image_elements
            .iter_mut()
            .zip([(48., 350.), (180., 350.), (48., 240.)])
    {
        image.bounds = ParsedBounds {
            x0: x,
            y0: y,
            x1: x + 48.,
            y1: y + 48.,
        };
    }
    parsed
}

/// 横並びを含む原本canvasの画像は、全て元のpage座標に置き、本文・画像を失わない。
#[test]
fn verified_image_row_uses_original_page_frames_without_inline_stacking() {
    let (xml, images) = package_source(image_row());
    assert!(images >= 1);
    assert_eq!(xml.matches("<wp:anchor ").count(), 3);
    assert_eq!(xml.matches("<wp:inline ").count(), 0);
    for (x, top) in [(48, 394), (180, 394), (48, 504)] {
        assert!(xml.contains(&format!(
            "<wp:positionH relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset>",
            x * 12700
        )));
        assert!(xml.contains(&format!(
            "<wp:positionV relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset>",
            top * 12700
        )));
    }
    for text in [
        "PAGEA LEFT 1",
        "PAGEA LEFT 2",
        "PAGEB LEFT 1",
        "PAGEB LEFT 2",
    ] {
        assert!(xml.contains(text));
    }
}

/// 横並びの根拠がない一画像を、新しいcanvas経路へ昇格しない。
#[test]
fn single_image_keeps_existing_inline_flow() {
    let mut parsed = image_row();
    parsed.image_elements.truncate(1);
    let (xml, _) = package_source(parsed);
    assert_eq!(xml.matches("<wp:anchor ").count(), 0);
    assert_eq!(xml.matches("<wp:inline ").count(), 1);
}

/// 画像矩形が重なる場合は絵の重なり順を推測せず、既存のflowを維持する。
#[test]
fn overlapping_image_frames_keep_existing_flow() {
    let mut parsed = image_row();
    parsed.image_elements[1].bounds = parsed.image_elements[0].bounds;
    let (xml, _) = package_source(parsed);
    assert_eq!(xml.matches("<wp:anchor ").count(), 0);
    assert_eq!(xml.matches("<wp:inline ").count(), 3);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 横位置・高さが変わっても二画像の座標はpage原点を保ち、追加のinline高さを作らない。
    #[test]
    fn row_frames_preserve_original_positions(x_shift in 0u16..40, width in 12u16..72, gap in 12u16..100, bottom in 160u16..400) {
        let mut parsed=image_row();parsed.image_elements.truncate(2);
        let left=48.+f32::from(x_shift);let width=f32::from(width);let bottom=f32::from(bottom);
        let xs=[left,left+width+f32::from(gap)];
        for (image,x) in parsed.image_elements.iter_mut().zip(xs) {
            image.bounds=ParsedBounds {x0:x,y0:bottom,x1:x+width,y1:bottom+48.};
            image.width_px=(width*96./72.).round() as u32;
        }
        let (xml,_)=package_source(parsed);
        prop_assert_eq!(xml.matches("<wp:anchor ").count(),2);
        prop_assert_eq!(xml.matches("<wp:inline ").count(),0);
        for x in xs {
            let expected=format!("<wp:positionH relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset>",(x*12700.).round() as i32);
            prop_assert!(xml.contains(&expected));
        }
        let expected=format!("<wp:positionV relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset>",((792.-bottom-48.)*12700.).round() as i32);
        prop_assert!(xml.contains(&expected));
    }
}

/// page内でもWordの符号付きEMUを超える座標を、上限で飽和したanchorへ読み替えない。
#[test]
fn unrepresentable_page_offsets_keep_existing_flow() {
    let mut parsed = image_row();
    parsed.pages[0].width = 300_000.;
    for (index, image) in parsed.image_elements.iter_mut().enumerate().take(2) {
        image.bounds.x0 = 170_000. + index as f32 * 100.;
        image.bounds.x1 = image.bounds.x0 + 48.;
    }
    assert!(crate::source_image_rows::select(&parsed, 0, &[], false).is_empty());
}

/// 回転page・切断された矩形・不一致の表示寸法・本文重なりをcanvasと推測しない。
#[test]
fn ambiguous_source_frames_keep_existing_flow() {
    for case in 0..4 {
        let mut parsed = image_row();
        match case {
            0 => parsed.pages[0].rotation = 90,
            1 => parsed.image_elements[0].bounds.x0 = -1.,
            2 => parsed.image_elements[0].width_px += 1,
            3 => parsed.text_elements[0].bounds = parsed.image_elements[0].bounds,
            _ => unreachable!(),
        }
        let (xml, _) = package_source(parsed);
        assert_eq!(xml.matches("<wp:anchor ").count(), 0, "case={case}");
        assert_eq!(xml.matches("<wp:inline ").count(), 3, "case={case}");
    }
}

/// 本文と重なる独立した第三画像は従来処理へ残し、確認できる横並び二画像を妨げない。
#[test]
fn text_overlap_outside_verified_row_keeps_only_that_image_inline() {
    let mut parsed = image_row();
    parsed.text_elements[0].bounds = parsed.image_elements[2].bounds;
    let (xml, _) = package_source(parsed);
    assert_eq!(xml.matches("<wp:anchor ").count(), 2);
    assert_eq!(xml.matches("<wp:inline ").count(), 1);
}
