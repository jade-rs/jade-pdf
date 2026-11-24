//! PDF画像の下端原点とWord描画anchorの上端原点を分離する。
use super::{ParsedImageElement, ParsedPage, pdf_points_to_emu};
use docx_rs::{Pic, RelativeFromHType, RelativeFromVType};

/// 所属する正立ページの高さから画像上端を求め、余白に依存しない位置を保存する。
/// 寸法不明・ページ回転が未解決の場合は従来の配置を保つ。
pub(super) fn picture(mut image: ParsedImageElement, page: Option<&ParsedPage>) -> Pic {
    let mut pic = Pic::new_with_dimensions(
        std::mem::take(&mut image.png_bytes),
        image.width_px,
        image.height_px,
    );
    if image.floating || image.rotation_degrees != 0 {
        let top = page
            .filter(|page| {
                page.rotation.rem_euclid(360) == 0 && page.height.is_finite() && page.height > 0.
            })
            .map(|page| page.height - image.bounds.y1)
            .filter(|top| top.is_finite());
        pic = pic
            .floating()
            .offset_x(pdf_points_to_emu(image.bounds.x0).unwrap_or(0))
            .offset_y(pdf_points_to_emu(top.unwrap_or(image.bounds.y0)).unwrap_or(0))
            .overlapping();
        if top.is_some() {
            pic = pic
                .relative_from_h(RelativeFromHType::Page)
                .relative_from_v(RelativeFromVType::Page);
        }
        if image.rotation_degrees != 0 {
            pic = pic.rotate(image.rotation_degrees);
        }
    }
    pic
}
