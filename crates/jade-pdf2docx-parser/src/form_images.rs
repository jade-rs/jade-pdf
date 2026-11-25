//! Form画像の可視領域を配置へ保存し、本文の探索責務から分離する。
use super::*;

/// 実際に呼ばれたForm画像を追加し、容量不足では部分結果を公開しない。
pub(super) fn append(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    streams: &[crate::structure::StreamSpan<'_>],
    quality: &mut QualityReport,
    placements: &mut Vec<ImagePlacement>,
) -> Option<()> {
    if streams.iter().any(|stream| {
        crate::metadata::value(stream.dictionary, b"Subtype")
            .is_some_and(|v| !crate::metadata::is_name(v, b"Image"))
    }) {
        crate::form_content::visit_images(
            decode_budget,
            bytes,
            quality,
            |page, object_number, matrix, paint, clip| {
                if !matrix.iter().all(|v| v.is_finite()) {
                    return Some(());
                }
                let Some(bounds) = clip.transformed_bounds(decode_budget, [0., 0., 1., 1.], matrix)
                else {
                    return (!decode_budget.is_stopped()).then_some(());
                };
                let clipped = !clip.contains_rect([0., 0., 1., 1.], matrix);
                let clip = if clipped {
                    Some(clip.intersection(decode_budget, clip)?)
                } else {
                    None
                };
                crate::metadata_workspace::try_push(
                    decode_budget,
                    placements,
                    ImagePlacement {
                        object_number,
                        page,
                        bounds,
                        floating: true,
                        rotation_degrees: rotation_degrees_from_matrix(matrix),
                        matrix,
                        paint,
                        clip,
                    },
                )
            },
        );
        if decode_budget.is_stopped() {
            return None;
        }
    }
    Some(())
}
