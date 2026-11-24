//! PDF下端原点の画像を、ページ上端原点のWord anchorへ変換する回帰。
use super::*;
use proptest::prelude::*;

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
