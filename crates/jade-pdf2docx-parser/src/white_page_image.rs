//! 先に紙全体へ描かれた、完全に白く不透明なラスターを確認する。
use super::{DecodeBudget, ImagePlacement, ParsedPageViewport, ParsedTextElement, image_draws};
use crate::{object_index, resource_resolver::ResourceResolver};
use flate2::read::ZlibDecoder;
use std::io::Read;

/// 正式ページのMediaBoxと、正立した画像の描画矩形が一致するか。
pub(super) fn covers_media_box(budget: &DecodeBudget, page: usize, matrix: [f32; 6]) -> bool {
    let [width, b, c, height, x, y] = matrix;
    budget.has_page_tree()
        && width > 0.
        && height > 0.
        && b == 0.
        && c == 0.
        && budget.page_media_box(page) == Some([x, y, x + width, y + height])
}

/// 復号に失敗した先行Contentsを見落とした画像を「最初」と判定しない。
pub(super) fn has_all_contents(
    page: usize,
    references: &[(usize, u32)],
    decoded: &[(usize, Vec<u8>)],
) -> bool {
    let references = references.partition_point(|(p, _)| *p <= page)
        - references.partition_point(|(p, _)| *p < page);
    let decoded =
        decoded.partition_point(|(p, _)| *p <= page) - decoded.partition_point(|(p, _)| *p < page);
    references == decoded
}

/// Device色が原本ページのResourceで置換されていないことを確認する。
pub(super) fn check_device_spaces(
    budget: &DecodeBudget,
    source: &[u8],
    placements: &mut [ImagePlacement],
) {
    let resources = budget.page_resources(source);
    let objects = object_index::build(budget, source);
    let mut resolver = objects
        .as_ref()
        .map(|objects| ResourceResolver::new(objects, budget));
    for placement in placements
        .iter_mut()
        .filter(|p| p.paint.first_unclipped_draw)
    {
        let plain = resources
            .as_ref()
            .and_then(|resources| resources.get(placement.page).copied().flatten())
            .zip(resolver.as_mut())
            .and_then(|(resources, resolver)| {
                image_draws::resources::unmodified_device_spaces(budget, resolver, resources)
            });
        placement.paint.first_unclipped_draw = plain == Some(true);
    }
}

/// 正式ページの表示領域に文字がある場合だけ、紙画像をWord本文から省ける。
pub(super) fn visible_text_pages(
    budget: &DecodeBudget,
    viewports: &[ParsedPageViewport],
    texts: &[ParsedTextElement],
) -> Option<Vec<bool>> {
    let mut seen = budget.metadata_vector(viewports.len())?;
    seen.resize(viewports.len(), false);
    for text in texts {
        if budget.is_stopped() {
            return None;
        }
        let Some(view) = viewports.get(text.page) else {
            continue;
        };
        let b = text.bounds;
        if !seen[text.page]
            && view.page.page_number == text.page
            && b.x0 < b.x1
            && b.y0 < b.y1
            && b.x1 > view.origin_x
            && b.y1 > view.origin_y
            && b.x0 < view.origin_x + view.page.width
            && b.y0 < view.origin_y + view.page.height
            && !text.text.trim().is_empty()
        {
            seen[text.page] = true;
        }
    }
    Some(seen)
}

/// 自前のPNG encoderが出力する非interlace・無filter行を、固定長bufferで照合する。
pub(super) fn is_opaque_white(budget: &DecodeBudget, png: &[u8]) -> bool {
    white_rows(budget, png) == Some(true)
}

/// RGB/RGBAの全成分が白で、alphaがある場合は完全不透明と確認する。
fn white_rows(budget: &DecodeBudget, png: &[u8]) -> Option<bool> {
    if !png.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR") || png.get(37..41)? != b"IDAT" {
        return None;
    }
    let channels = match png.get(24..29)? {
        [8, 2, 0, 0, 0] => 3,
        [8, 6, 0, 0, 0] => 4,
        _ => return None,
    };
    let read_u32 = |offset| {
        Some(u32::from_be_bytes(
            png.get(offset..offset + 4)?.try_into().ok()?,
        ))
    };
    let row_bytes = usize::try_from(read_u32(16)?).ok()?.checked_mul(channels)?;
    let height = read_u32(20)?;
    let end = 41usize.checked_add(usize::try_from(read_u32(33)?).ok()?)?;
    let mut data = ZlibDecoder::new(png.get(41..end)?);
    let mut buffer = [0u8; 4096];
    for _ in 0..height {
        let mut filter = [0];
        data.read_exact(&mut filter).ok()?;
        if filter != [0] {
            return None;
        }
        let mut remaining = row_bytes;
        while remaining > 0 {
            if budget.is_stopped() {
                return None;
            }
            let span = remaining.min(buffer.len());
            data.read_exact(&mut buffer[..span]).ok()?;
            if buffer[..span].iter().any(|&value| value != 255) {
                return Some(false);
            }
            remaining -= span;
        }
    }
    Some(row_bytes > 0 && height > 0 && data.read(&mut buffer[..1]).ok()? == 0)
}
