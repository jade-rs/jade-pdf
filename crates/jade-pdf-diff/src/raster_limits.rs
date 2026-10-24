//! Metadata checks before requesting potentially expensive image decoding.
use anyhow::{ensure, Result};
use pdfium_render::prelude::*;

pub(crate) const DEFAULT_SOURCE_IMAGE_PIXELS: u64 = 64_000_000;

pub(crate) fn check_page(page: &PdfPage<'_>, max_image_pixels: u64) -> Result<()> {
    let mut objects = 0usize;
    let mut pixels = 0u64;
    let page_objects = page.objects();
    ensure!(
        page_objects.len() <= 20_000,
        "Raster page object limit exceeded"
    );
    for i in 0..page_objects.len() {
        check_object(
            &page_objects.get(i)?,
            0,
            &mut objects,
            &mut pixels,
            max_image_pixels,
        )?;
    }
    let annotations = page.annotations();
    ensure!(
        annotations.len() <= 20_000,
        "Raster annotation limit exceeded"
    );
    for i in 0..annotations.len() {
        let annotation = annotations.get(i)?;
        let children = annotation.objects();
        ensure!(
            children.len() <= 20_000usize.saturating_sub(objects),
            "Raster page object limit exceeded"
        );
        for j in 0..children.len() {
            check_object(
                &children.get(j)?,
                0,
                &mut objects,
                &mut pixels,
                max_image_pixels,
            )?;
        }
    }
    Ok(())
}

fn check_object(
    object: &PdfPageObject<'_>,
    depth: usize,
    objects: &mut usize,
    pixels: &mut u64,
    maximum: u64,
) -> Result<()> {
    ensure!(depth <= 32, "Raster nested form depth limit exceeded");
    *objects += 1;
    ensure!(*objects <= 20_000, "Raster page object limit exceeded");
    if let Some(image) = object.as_image_object() {
        // width/height query FPDFImageObj_GetImageMetadata, not a decoded bitmap.
        let width = image.width()?;
        let height = image.height()?;
        ensure!(width > 0 && height > 0, "Invalid source image dimensions");
        let count = (width as u64).saturating_mul(height as u64);
        *pixels = pixels.saturating_add(count);
        ensure!(
            *pixels <= maximum,
            "Raster source image pixel limit exceeded ({maximum})"
        );
    }
    if let Some(form) = object.as_x_object_form_object() {
        ensure!(
            form.len() <= 20_000usize.saturating_sub(*objects),
            "Raster page object limit exceeded"
        );
        for i in 0..form.len() {
            check_object(&form.get(i)?, depth + 1, objects, pixels, maximum)?;
        }
    }
    Ok(())
}
