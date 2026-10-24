//! A temporary, unsaved SDK coordinate basis for text extraction only.
//!
//! Pdfium text heuristics contain absolute-coordinate tolerances. Very small
//! raw coordinates can therefore suppress distinct glyphs even when UserUnit
//! preserves their physical size. Query text in physical points and map it
//! back afterwards; callers retain the original page metadata and source.
use anyhow::{ensure, Result};
use pdfium_render::prelude::*;
use std::ffi::c_void;

use crate::Bounds;

pub(crate) const MAX_NORMALIZED_OBJECTS: usize = 200_000;

pub(crate) fn scale(user_unit: Option<f64>) -> Result<f64> {
    let unit = user_unit.unwrap_or(1.0);
    crate::page_units::validate(unit)?;
    // The public object-transform API used by the Rust wrapper takes f32.
    // Use its exact representable factor in both passes and inverse mapping.
    let factor = f64::from(unit as f32);
    ensure!(
        factor.is_finite() && factor > 0.0,
        "Physical text scale is outside SDK precision"
    );
    Ok(factor)
}

fn scaled_box(bounds: Bounds, factor: f64) -> Result<PdfRect> {
    let values = [bounds.x0, bounds.y0, bounds.x1, bounds.y1].map(|x| (x * factor) as f32);
    ensure!(
        values.into_iter().all(f32::is_finite) && values[0] < values[2] && values[1] < values[3],
        "Physical text page box is outside SDK precision"
    );
    Ok(PdfRect::new(
        PdfPoints::new(values[1]),
        PdfPoints::new(values[0]),
        PdfPoints::new(values[3]),
        PdfPoints::new(values[2]),
    ))
}

/// The raw page is owned by the diagnostic caller until this returns.
pub(crate) fn prepare_raw(
    pdfium: &Pdfium,
    page: *mut c_void,
    factor: f64,
    bounds: Bounds,
    remaining_objects: usize,
) -> Result<usize> {
    if factor == 1.0 {
        return Ok(0);
    }
    let rect = scaled_box(bounds, factor)?;
    let b = pdfium.bindings();
    let count = b.FPDFPage_CountObjects(page.cast());
    ensure!(
        count >= 0 && count as usize <= remaining_objects,
        "Physical text normalization exceeds the document object budget"
    );
    for index in 0..count {
        let object = b.FPDFPage_GetObject(page.cast(), index);
        ensure!(
            !object.is_null(),
            "Failed to load physical text page object"
        );
        b.FPDFPageObj_Transform(object, factor, 0.0, 0.0, factor, 0.0, 0.0);
    }
    // The effective view box controls the SDK's page-display ordering. Only
    // this temporary SDK document is changed; no annotation or source is saved.
    b.FPDFPage_SetMediaBox(
        page.cast(),
        rect.left().value,
        rect.bottom().value,
        rect.right().value,
        rect.top().value,
    );
    b.FPDFPage_SetCropBox(
        page.cast(),
        rect.left().value,
        rect.bottom().value,
        rect.right().value,
        rect.top().value,
    );
    Ok(count as usize)
}

pub(crate) fn prepare(
    page: &mut PdfPage<'_>,
    facts: &crate::unicode_facts::PageFacts,
    bounds: Bounds,
) -> Result<()> {
    let factor = facts.coordinate_scale;
    if factor == 1.0 {
        return Ok(());
    }
    let rect = scaled_box(bounds, factor)?;
    let count = page.objects().len();
    ensure!(
        count == facts.normalized_objects && count <= MAX_NORMALIZED_OBJECTS,
        "Physical text page object count changed while opening"
    );
    // Cached objects are sufficient for FPDFText_LoadPage. Regeneration would
    // unnecessarily rewrite resources and could affect shared form content.
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
    for index in 0..count {
        let mut object = page.objects().get(index)?;
        object.transform(factor as f32, 0.0, 0.0, factor as f32, 0.0, 0.0)?;
    }
    page.boundaries_mut().set_media(rect)?;
    page.boundaries_mut().set_crop(rect)?;
    Ok(())
}

/// A sorted multiset comparison retains every previously exposed visible
/// scalar and its uncertainty bit. Extra recovered glyphs are allowed.
pub(crate) fn ensure_preserved(mut before: Vec<u64>, mut after: Vec<u64>) -> Result<()> {
    before.sort_unstable();
    after.sort_unstable();
    let mut at = 0;
    for key in before {
        while at < after.len() && after[at] < key {
            at += 1;
        }
        ensure!(after.get(at) == Some(&key),
            "SDK text normalization lost a character or Unicode mapping diagnostic. No partial text was returned.");
        at += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_requires_every_duplicate_and_mapping_diagnostic() {
        let a = u64::from('a');
        let flagged = a | (1 << 32);
        assert!(ensure_preserved(vec![a, a, flagged], vec![flagged, a, a, 123]).is_ok());
        assert!(ensure_preserved(vec![a, a], vec![a]).is_err());
        assert!(ensure_preserved(vec![flagged], vec![a]).is_err());
        assert!(ensure_preserved(vec![a], vec![flagged]).is_err());
    }

    #[test]
    fn unrepresentable_sdk_coordinates_fail_explicitly() {
        let bounds = Bounds {
            x0: 0.0,
            y0: 0.0,
            x1: 300.0,
            y1: 300.0,
        };
        assert!(scaled_box(bounds, 64.0).is_ok());
        assert!(scaled_box(bounds, f64::MAX).is_err());
    }
}
