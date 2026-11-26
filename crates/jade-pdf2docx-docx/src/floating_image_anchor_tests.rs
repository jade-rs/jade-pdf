//! PDF下端原点の画像を、ページ上端原点のWord anchorへ変換する回帰。
use super::*;

/// 正立へ相殺済みの画像は、回転pageの表示高さを使って上端原点へ変換する。
#[test]
fn cancelled_image_rotation_uses_its_rotated_page_display_height() {
    let mut parsed = source(2, &[0, 1]);
    parsed.pages[0].rotation = 90;
    parsed.pages[1].rotation = 270;
    parsed.pages[1].width = 700.;
    parsed.pages[1].height = 800.;
    for image in &mut parsed.image_elements {
        image.floating = true;
    }
    let (xml, _) = package_source(parsed);
    assert_eq!(
        xml.matches("<wp:positionH relativeFrom=\"page\">").count(),
        2
    );
    assert_eq!(
        xml.matches("<wp:positionV relativeFrom=\"page\">").count(),
        2
    );
    let actual: Vec<i32> = xml
        .split("<wp:positionV relativeFrom=\"page\"><wp:posOffset>")
        .skip(1)
        .map(|part| part.split('<').next().unwrap().parse().unwrap())
        .collect();
    assert_eq!(actual, [384 * 12700, 472 * 12700]);
}

/// 未確認の回転・寸法・表示領域を、回転pageのcanvas配置へ拡大しない。
#[test]
fn rotated_page_with_unconfirmed_image_frame_keeps_previous_anchor_origin() {
    for case in 0..4 {
        let mut parsed = source(2, &[0]);
        parsed.pages[0].rotation = 90;
        parsed.image_elements[0].floating = true;
        match case {
            0 => parsed.pages[0].rotation = 45,
            1 => parsed.image_elements[0].rotation_degrees = 90,
            2 => parsed.image_elements[0].width_px = 65,
            _ => parsed.image_elements[0].bounds.x1 = 1100.,
        }
        let (xml, _) = package_source(parsed);
        assert_eq!(
            xml.matches("<wp:positionH relativeFrom=\"page\">").count(),
            0,
            "case={case}"
        );
        assert_eq!(
            xml.matches("<wp:positionV relativeFrom=\"page\">").count(),
            0,
            "case={case}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 正立へ戻した画像の上端は、各直角pageの表示高さと小数座標から求める。
    #[test]
    fn cancelled_image_top_matches_rotated_display_frame(
        turn in 1u8..=3, width_quarters in 2400u16..3600,
        height_quarters in 3600u16..4800, bottom_quarters in 0u16..1200,
    ) {
        let mut parsed=source(2,&[0]);
        let (width,height,bottom)=(f32::from(width_quarters)/4.,f32::from(height_quarters)/4.,f32::from(bottom_quarters)/4.);
        parsed.pages[0].width=width;parsed.pages[0].height=height;parsed.pages[0].rotation=i32::from(turn)*90;
        parsed.image_elements[0].floating=true;parsed.image_elements[0].bounds.y0=bottom;parsed.image_elements[0].bounds.y1=bottom+48.;
        let expected=(f64::from(if turn==2{height}else{width})-f64::from(bottom)-48.)*12700.;
        let (xml,_)=package_source(parsed);
        let actual:Vec<i32>=xml.split("<wp:positionV relativeFrom=\"page\"><wp:posOffset>").skip(1).map(|v|v.split('<').next().unwrap().parse().unwrap()).collect();
        prop_assert_eq!(actual.len(),1);
        prop_assert!((f64::from(actual[0])-expected).abs()<=1.);
    }
}

/// margin原点を混ぜず、各所属ページの高さから画像上端の位置を求める。
#[test]
fn floating_images_use_page_top_origin_on_their_own_page() {
    let mut parsed = source(2, &[0, 1]);
    parsed.pages[1].height = 900.;
    for image in &mut parsed.image_elements {
        image.floating = true;
    }
    let (xml, images) = package_source(parsed);
    assert!(images >= 1);
    assert_eq!(xml.matches("<wp:anchor ").count(), 2);
    assert_eq!(
        xml.matches("<wp:positionH relativeFrom=\"page\">").count(),
        2,
        "floating image must anchor to the page instead of the section margin"
    );
    assert_eq!(
        xml.matches("<wp:positionV relativeFrom=\"page\">").count(),
        2
    );
    for top in [564, 672] {
        assert!(
            xml.contains(&format!("<wp:posOffset>{}</wp:posOffset>", top * 12700)),
            "floating image must convert PDF bottom origin to page top origin"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 同じ画像でも所属ページと小数座標に応じて、原本の上端距離を保つ。
    #[test]
    fn floating_image_origin_matches_source_coordinates(height_quarters in 3200u16..5600, bottom_quarters in 0u16..2400) {
        let mut parsed = source(2, &[0, 1]);
        let height = f32::from(height_quarters) / 4.;
        for (index, page) in parsed.pages.iter_mut().enumerate() { page.height = height + index as f32 * 75.25; }
        for image in &mut parsed.image_elements {
            image.floating = true;
            image.bounds.y0 = f32::from(bottom_quarters) / 4.;
            image.bounds.y1 = image.bounds.y0 + 48.;
        }
        let expected: Vec<_> = parsed.pages.iter().map(|page| (f64::from(page.height) - (f64::from(bottom_quarters) / 4. + 48.)) * 12700.).collect();
        let (xml, _) = package_source(parsed);
        let actual: Vec<i32> = xml.split("<wp:positionV relativeFrom=\"page\"><wp:posOffset>").skip(1).map(|part| part.split('<').next().unwrap().parse().unwrap()).collect();
        prop_assert_eq!(actual.len(), 2);
        for (actual, expected) in actual.into_iter().zip(expected) { prop_assert!((f64::from(actual) - expected).abs() <= 1., "{actual} vs {expected}"); }
    }
}
