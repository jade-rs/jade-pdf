//! native objectのclipを親Formの座標へ戻し、完全に外れた本文を除外する。
use crate::{decode::DecodeBudget, form_clip::Clip};
use pdfium_render::prelude::*;

#[path = "pdfium_clip_index.rs"]
mod ownership;

/// nativeの各pathを同じ凸性検証で交差し、未対応pathは既存の品質診断に委ねる。
pub(crate) fn object_clip(
    budget: &DecodeBudget,
    object: &PdfPageObject<'_>,
    inherited: Option<&Clip>,
    matrix: [f32; 6],
) -> Option<Clip> {
    let native = object.get_clip_path()?;
    // Pdfiumの「clipなし」の-1はwrapperのu16変換でMAXになる。
    // wrapperのiteratorは末尾でもindexを進めるため、範囲とgetで取得する。
    if native.len() == u16::MAX {
        return None;
    }
    let mut result = None;
    for index in native.as_range() {
        let segments = native.get(index).ok()?;
        if budget.is_stopped() {
            return None;
        }
        if segments.len() > 65 {
            continue;
        }
        let mut path = crate::content_clip::Path::default();
        let mut started = false;
        let mut supported = true;
        for segment in segments.iter() {
            match segment.segment_type() {
                PdfPathSegmentType::MoveTo if !started => started = true,
                PdfPathSegmentType::LineTo if started => {}
                _ => {
                    supported = false;
                    break;
                }
            }
            let (x, y) = segment.point();
            path.push(
                budget,
                crate::content_clip::transform([x.value as f64, y.value as f64], matrix),
            )?;
        }
        if !supported {
            continue;
        }
        let Some(polygon) = path.finish() else {
            continue;
        };
        let next = Clip::polygon(polygon);
        result = Some(match result.as_ref().or(inherited) {
            Some(previous) => previous.intersection(budget, &next)?,
            None => next,
        });
    }
    result
}

/// objectの外接矩形がclipと全く交差しない場合だけ非表示と判定する。
pub(crate) fn outside(
    budget: &DecodeBudget,
    object: &PdfPageObject<'_>,
    clip: Option<&Clip>,
    matrix: [f32; 6],
) -> bool {
    let (Some(clip), Ok(bounds)) = (clip, object.bounds()) else {
        return false;
    };
    let rect = bounds.to_rect();
    clip.transformed_bounds(
        budget,
        [
            rect.left().value,
            rect.bottom().value,
            rect.right().value,
            rect.top().value,
        ],
        matrix,
    )
    .is_none()
}

/// Form内のobject座標をページ空間へ変換する行列を返す。
pub(crate) fn form_matrix(object: &PdfPageObject<'_>, parent: [f32; 6]) -> Option<[f32; 6]> {
    let matrix = object.matrix().ok()?;
    Some(crate::parser::concat_matrix(
        parent,
        [
            matrix.a(),
            matrix.b(),
            matrix.c(),
            matrix.d(),
            matrix.e(),
            matrix.f(),
        ],
    ))
}

/// 部分clipを持つobjectだけを調べ、ページ文字indexごとの非表示判定を保存する。
pub(crate) fn excluded<'a>(
    budget: &DecodeBudget,
    objects: &'a impl PdfPageObjectsCommon<'a>,
    page_text: &PdfPageText<'_>,
) -> Result<Vec<bool>, crate::ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let mut result = Vec::new();
    let mut ownership = None;
    exclude_objects(
        budget,
        objects,
        objects,
        page_text,
        None,
        [1., 0., 0., 1., 0., 0.],
        0,
        &mut result,
        &mut ownership,
    )?;
    Ok(result)
}

/// Formのclipをページ座標で交差し、重なる文字を幾何検索で取り違えずに除外する。
#[allow(clippy::too_many_arguments)]
fn exclude_objects<'a, 'r, 't>(
    budget: &DecodeBudget,
    objects: &'a impl PdfPageObjectsCommon<'a>,
    root_objects: &'r impl PdfPageObjectsCommon<'r>,
    page_text: &'t PdfPageText<'_>,
    inherited: Option<&Clip>,
    matrix: [f32; 6],
    depth: usize,
    result: &mut Vec<bool>,
    ownership: &mut Option<ownership::Index<'t>>,
) -> Result<(), crate::ParseError> {
    let _ = budget.check_form_depth(depth);
    budget.check()?;
    for object in objects.iter() {
        budget.check()?;
        if object.as_text_object().is_none() && object.as_x_object_form_object().is_none() {
            continue;
        }
        let clip = object_clip(budget, &object, inherited, matrix);
        budget.check()?;
        let region = clip.as_ref().or(inherited);
        if let Some(form) = object.as_x_object_form_object() {
            if let Some(matrix) = form_matrix(&object, matrix) {
                exclude_objects(
                    budget,
                    form,
                    root_objects,
                    page_text,
                    region,
                    matrix,
                    depth + 1,
                    result,
                    ownership,
                )?;
            }
        } else if let (Some(text), Some(clip), Ok(bounds)) =
            (object.as_text_object(), region, object.bounds())
        {
            let rect = bounds.to_rect();
            if clip.contains_rect(
                [
                    rect.left().value,
                    rect.bottom().value,
                    rect.right().value,
                    rect.top().value,
                ],
                matrix,
            ) {
                continue;
            }
            let count = page_text.len() as usize;
            if result.is_empty() {
                *result = budget.metadata_vector(count).ok_or_else(|| {
                    crate::ParseError::Io(std::io::Error::other("native clip storage stopped"))
                })?;
                result.resize(count, false);
            }
            if ownership.is_none() {
                *ownership = Some(ownership::Index::new(budget, root_objects, page_text)?);
            }
            let index = ownership.as_ref().unwrap();
            if let Some(entries) = index.entries(text) {
                for entry in entries {
                    budget.check()?;
                    let character = index.character(entry.index).map_err(native_error)?;
                    exclude_character(budget, &character, clip, result)?;
                }
                continue;
            }
            // 幾何が一意でないobjectは、既存SDKの厳密な所属検索と予約を維持する。
            budget.charge_metadata_workspace_bytes(
                count.max(4) as u64 * 3 * std::mem::size_of::<i32>() as u64,
            );
            budget.check()?;
            let chars = text
                .chars(page_text)
                .map_err(|error| crate::ParseError::Io(std::io::Error::other(error.to_string())))?;
            for character in chars.iter() {
                exclude_character(budget, &character, clip, result)?;
            }
        }
    }
    Ok(())
}

/// SDKの失敗を既存と同じ解析I/Oエラーへ変換する。
fn native_error(error: PdfiumError) -> crate::ParseError {
    crate::ParseError::Io(std::io::Error::other(error.to_string()))
}

/// 親FormのCTMを含む文字矩形を同じclip判定へ通し、元indexだけを更新する。
fn exclude_character(
    budget: &DecodeBudget,
    character: &PdfPageTextChar<'_>,
    clip: &Clip,
    result: &mut [bool],
) -> Result<(), crate::ParseError> {
    budget.check()?;
    let rect = character.loose_bounds().map_err(native_error)?;
    if clip
        .transformed_bounds(
            budget,
            [
                rect.left().value,
                rect.bottom().value,
                rect.right().value,
                rect.top().value,
            ],
            [1., 0., 0., 1., 0., 0.],
        )
        .is_none()
    {
        result[character.index()] = true;
    }
    budget.check()
}
