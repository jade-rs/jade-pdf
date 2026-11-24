//! PDF下端原点の画像を、ページ上端原点のWord anchorへ変換する回帰。
use super::*;

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
