//! PDF画像の下端原点とWord描画anchorの上端原点を分離する。
use super::{ParsedImageElement, ParsedPage, pdf_points_to_emu};
use docx_rs::{Pic, RelativeFromHType, RelativeFromVType};

/// 所属する正立ページの高さから画像上端を求め、余白に依存しない位置を保存する。
/// 寸法不明・ページ回転が未解決の場合は従来の配置を保つ。
pub(super) fn picture(mut image: ParsedImageElement, page: Option<&ParsedPage>) -> Pic {
    let extent = upright_extent(&image);
    let bytes = std::mem::take(&mut image.png_bytes);
    let mut pic = if let Some((width, height)) = extent {
        // PNGの実解像度と表示pxを介さず、原本pointの寸法をDrawingMLへ渡す。
        Pic::new_with_dimensions(bytes, 1, 1).size(width, height)
    } else {
        Pic::new_with_dimensions(bytes, image.width_px, image.height_px)
    };
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

/// 正立した画像の有限な原本矩形だけを使い、回転前の辺が不明な画像は従来値へ戻す。
fn upright_extent(image: &ParsedImageElement) -> Option<(u32, u32)> {
    if image.rotation_degrees % 360 != 0 {
        return None;
    }
    Some((
        axis_extent(image.bounds.x0, image.bounds.x1)?,
        axis_extent(image.bounds.y0, image.bounds.y1)?,
    ))
}

/// 元の二端をdoubleで差し引き、正のEMU寸法を一回だけ丸める。
fn axis_extent(start: f32, end: f32) -> Option<u32> {
    let points = (f64::from(end) - f64::from(start)).abs();
    let emu = points * 12_700.;
    (points.is_finite() && points > 0. && emu <= f64::from(u32::MAX))
        .then(|| (emu.round() as u32).max(1))
}
