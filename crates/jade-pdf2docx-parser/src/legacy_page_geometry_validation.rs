//! Rootのない互換ページの幾何属性を検査し、抽出処理へ共有する。
use crate::{
    ParseError, ParseLimits,
    decode::DecodeBudget,
    limits::check_limit,
    metadata,
    metadata_workspace::{Set, ensure_active},
    page_attributes::{
        Objects, dictionary_value, is_null, rectangle_value, resolve_value, rotation_value,
    },
};
use std::path::Path;

/// 最寄りの非null属性から確定した互換ページの表示情報。
pub(crate) struct Geometry {
    /// 互換入力では欠落を許す、正規化済みの用紙矩形。
    pub(crate) media_box: Option<[f32; 4]>,
    /// 用紙矩形との交差を取った表示矩形。
    pub(crate) crop_box: Option<[f32; 4]>,
    /// 90度の整数倍で検査済みの回転角。
    pub(crate) rotation: i32,
}

/// 継承・属性・座標の参照を予算内で解決し、破損値を親で補わない。
pub(crate) fn resolve<'a>(
    path: &Path,
    objects: &Objects<'a>,
    mut node: &'a [u8],
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<Geometry, ParseError> {
    ensure_active(budget)?;
    let mut media_box = None;
    let mut crop_box = None;
    let mut rotation = None;
    let mut visited = Set::default();
    let mut depth = 0u64;
    loop {
        if !metadata::dictionary_entries_complete(node) {
            return Err(invalid(path, "invalid geometry dictionary entries"));
        }
        for (name, result) in [
            (b"MediaBox".as_slice(), &mut media_box),
            (b"CropBox".as_slice(), &mut crop_box),
        ] {
            if result.is_some() {
                continue;
            }
            if let Some(value) = metadata::value(node, name) {
                let (value, _) = resolve_value(path, objects, value, limits, budget)?;
                if !is_null(value) {
                    *result = Some(rectangle_value(path, objects, value, limits, budget)?);
                }
            }
        }
        if rotation.is_none()
            && let Some(value) = metadata::value(node, b"Rotate")
        {
            let (value, _) = resolve_value(path, objects, value, limits, budget)?;
            if !is_null(value) {
                rotation = Some(
                    rotation_value(value)
                        .ok_or_else(|| invalid(path, "Rotate is not an integer multiple of 90"))?,
                );
            }
        }
        if media_box.is_some() && crop_box.is_some() && rotation.is_some() {
            break;
        }
        let Some(parent) = metadata::value(node, b"Parent") else {
            break;
        };
        if is_null(parent) {
            break;
        }
        let reference = metadata::reference_with_generation(parent)
            .ok_or_else(|| invalid(path, "geometry parent is not an indirect reference"))?;
        depth += 1;
        check_limit(
            "page inheritance depth",
            depth,
            limits.max_container_nesting as u64,
        )?;
        if !visited.insert(budget, reference)? {
            return Err(invalid(path, "cyclic geometry parent reference"));
        }
        let (_, body) = objects
            .get(&reference.0)
            .filter(|(generation, _)| *generation == reference.1)
            .ok_or_else(|| invalid(path, "missing or wrong-generation geometry parent"))?;
        node = dictionary_value(path, body)?;
    }
    let crop_box = match (media_box, crop_box) {
        (Some(media), Some(crop)) => Some(crate::page_attributes::crop_intersection(
            path, media, crop,
        )?),
        (_, crop) => crop,
    };
    Ok(Geometry {
        media_box,
        crop_box,
        rotation: rotation.unwrap_or(0),
    })
}

/// 互換ページの幾何属性の破損を公開構造エラーとして返す。
fn invalid(path: &Path, reason: &'static str) -> ParseError {
    ParseError::InvalidPdfStructure {
        path: path.into(),
        reason,
    }
}
