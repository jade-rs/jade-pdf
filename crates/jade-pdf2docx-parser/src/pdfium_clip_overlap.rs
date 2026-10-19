//! 部分clipと別objectの重なりによるnative重複排除を、文字抽出前に拒否する。
use crate::{ParseError, decode::DecodeBudget, form_clip::Clip};
use pdfium_render::prelude::*;
use std::path::Path;

/// ページ座標の文字外接矩形と部分clipの有無を保存する。
struct Envelope {
    /// ページ座標の矩形。
    bounds: [f32; 4],
    /// 一部だけがclip内にあるobjectかどうか。
    partial: bool,
}

/// 部分clipのないページは追加配列を確保せず、曖昧な重複を抽出前に止める。
pub(crate) fn check<'a>(
    budget: &DecodeBudget,
    path: &Path,
    objects: &'a impl PdfPageObjectsCommon<'a>,
    tags: &crate::pdfium_font_tags::Tagged,
) -> Result<(), ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let identity = [1., 0., 0., 1., 0., 0.];
    if !visit(budget, objects, tags, None, identity, 0, &mut |item, _| {
        Ok(item.partial)
    })? {
        return Ok(());
    }
    let mut items: Vec<Envelope> = Vec::new();
    visit(budget, objects, tags, None, identity, 0, &mut |item, _| {
        crate::metadata_workspace::push(budget, &mut items, item)?;
        Ok(false)
    })?;
    // clipは巡回中に借用し、継承領域をobjectごとに複製しない。
    // 部分clipのobjectだけを起点にし、通常object同士の二乗比較を避ける。
    let mut cursor = 0;
    visit(
        budget,
        objects,
        tags,
        None,
        identity,
        0,
        &mut |item, clip| {
            let index = cursor;
            cursor += 1;
            if !item.partial {
                return Ok(false);
            }
            let clip = clip.expect("partial object has a clipping region");
            for (other_index, other) in items.iter().enumerate() {
                budget.check()?;
                if other_index != index
                    && let Some(overlap) = intersection(item.bounds, other.bounds)
                    && !clip.contains_rect(overlap, identity)
                {
                    return Err(ParseError::Pdfium {
                        path: path.to_path_buf(),
                        message: concat!(
                            "overlapping partially clipped text: ",
                            "native deduplication may lose visible glyphs",
                        )
                        .into(),
                    });
                }
            }
            Ok(false)
        },
    )?;
    Ok(())
}

/// 二つの外接矩形の交差領域を返し、clip内だけでの隣接を区別する。
fn intersection(a: [f32; 4], b: [f32; 4]) -> Option<[f32; 4]> {
    let rect = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    (rect[0] <= rect[2] && rect[1] <= rect[3]).then_some(rect)
}

/// Formの変換・clipを引き継ぎ、可視objectを訪問する。trueで早期終了する。
#[allow(clippy::too_many_arguments)]
fn visit<'a>(
    budget: &DecodeBudget,
    objects: &'a impl PdfPageObjectsCommon<'a>,
    tags: &crate::pdfium_font_tags::Tagged,
    inherited: Option<&Clip>,
    matrix: [f32; 6],
    depth: usize,
    callback: &mut impl FnMut(Envelope, Option<&Clip>) -> Result<bool, ParseError>,
) -> Result<bool, ParseError> {
    let _ = budget.check_form_depth(depth);
    budget.check()?;
    for object in objects.iter() {
        budget.check()?;
        if object.as_text_object().is_none() && object.as_x_object_form_object().is_none() {
            continue;
        }
        let clip = crate::pdfium_clip::object_clip(budget, &object, inherited, matrix);
        budget.check()?;
        let region = clip.as_ref().or(inherited);
        if let Some(form) = object.as_x_object_form_object() {
            if let Some(matrix) = crate::pdfium_clip::form_matrix(&object, matrix)
                && visit(budget, form, tags, region, matrix, depth + 1, callback)?
            {
                return Ok(true);
            }
            continue;
        }
        let text = object.as_text_object().unwrap();
        if crate::pdfium_visible_text::hidden_object(text, tags)
            || crate::pdfium_clip::outside(budget, &object, region, matrix)
        {
            budget.check()?;
            continue;
        }
        let Ok(bounds) = object.bounds() else {
            continue;
        };
        let rect = bounds.to_rect();
        let rect = [
            rect.left().value,
            rect.bottom().value,
            rect.right().value,
            rect.top().value,
        ];
        let partial = region.is_some_and(|clip| !clip.contains_rect(rect, matrix));
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for point in [
            [rect[0], rect[1]],
            [rect[0], rect[3]],
            [rect[2], rect[1]],
            [rect[2], rect[3]],
        ] {
            let point = crate::content_clip::transform([point[0] as f64, point[1] as f64], matrix);
            bounds[0] = bounds[0].min(point[0] as f32);
            bounds[1] = bounds[1].min(point[1] as f32);
            bounds[2] = bounds[2].max(point[0] as f32);
            bounds[3] = bounds[3].max(point[1] as f32);
        }
        if callback(Envelope { bounds, partial }, region)? {
            return Ok(true);
        }
    }
    budget.check()?;
    Ok(false)
}
