//! 検証済みpageと正確な直角CTMが相殺する画像だけ、表示用metadataを正立へ戻す。
use super::{DecodeBudget, ImagePlacement, ParsedImageElement};

/// 画素・描画順・boundsを変えず、後段で回転する表示矩形と寸法を一致させる。
pub(super) fn cancel(
    budget: &DecodeBudget,
    placement: &ImagePlacement,
    image: &mut ParsedImageElement,
) {
    let Some(rotation) = budget.page_rotation(placement.page) else {
        return;
    };
    let rotation = rotation.rem_euclid(360);
    if placement.clip.is_some()
        || !exact_cancellation(placement.matrix, rotation)
        || !inside_verified_page(budget, placement)
    {
        return;
    }
    if rotation == 90 || rotation == 270 {
        std::mem::swap(&mut image.width_px, &mut image.height_px);
    }
    image.rotation_degrees = 0;
}

/// 丸めた角度から推測せず、反射・skewなしの元CTMの符号と零成分を要求する。
fn exact_cancellation(matrix: [f32; 6], rotation: i32) -> bool {
    let [a, b, c, d, ..] = matrix;
    matrix.into_iter().all(f32::is_finite)
        && match rotation {
            90 => a == 0. && d == 0. && b > 0. && c < 0.,
            180 => b == 0. && c == 0. && a < 0. && d < 0.,
            270 => a == 0. && d == 0. && b < 0. && c > 0.,
            _ => false,
        }
}

/// 切り詰めたboundsから寸法を推測せず、元画像全体が検証済み表示領域内にある場合に限る。
fn inside_verified_page(budget: &DecodeBudget, placement: &ImagePlacement) -> bool {
    let Some(media) = budget.page_media_box(placement.page) else {
        return false;
    };
    let Some(crop) = budget.page_crop_box(placement.page) else {
        return false;
    };
    let crop = crop.unwrap_or(media);
    let [left, bottom, right, top] = [
        media[0].max(crop[0]),
        media[1].max(crop[1]),
        media[2].min(crop[2]),
        media[3].min(crop[3]),
    ];
    let b = placement.bounds;
    [left, bottom, right, top, b.x0, b.y0, b.x1, b.y1]
        .into_iter()
        .all(f32::is_finite)
        && left < right
        && bottom < top
        && b.x0 < b.x1
        && b.y0 < b.y1
        && b.x0 >= left
        && b.x1 <= right
        && b.y0 >= bottom
        && b.y1 <= top
}
