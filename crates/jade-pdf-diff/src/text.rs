//! Read each Pdfium character exactly once. Rectangular text queries can repeat
//! overlapping glyphs and omit characters at rectangle boundaries.

use anyhow::{ensure, Context, Result};
use pdfium_render::prelude::*;
use std::collections::HashMap;

use crate::Bounds;

mod inline;
mod order;
mod table;

pub(crate) struct ExtractedRun {
    pub text: String,
    pub source_text_region: bool,
    pub source_text_anchor: bool,
    pub source_regions: Vec<crate::render::SourceTextRegion>,
    pub sdk_unicode_mapping_errors: usize,
    pub bounds: Bounds,
    pub size: Option<f64>,
    pub spans: Vec<(f64, f64)>,
    origin: (f64, f64),
    angle: f64,
    vertical: bool,
    layout: Bounds,
    tagged_cell: Option<crate::tagged_table::Cell>,
    outside_mark: Option<i32>,
}

pub(crate) struct ExtractedPage {
    pub runs: Vec<ExtractedRun>,
    pub source_characters: u64,
    pub sdk_unicode_mapping_errors: usize,
    pub reading_order_requires_review: bool,
}

struct RunBuilder {
    text: String,
    sdk_unicode_mapping_errors: usize,
    spans: Vec<(f64, f64)>,
    bounds: Option<Bounds>,
    size: Option<f64>,
    last_origin: Option<(f64, f64)>,
    angle: Option<f64>,
    vertical: Option<bool>,
    last_bounds: Option<Bounds>,
    last_baseline_end: Option<(f64, f64)>,
    first_origin: Option<(f64, f64)>,
    layout: Option<Bounds>,
    rtl: bool,
    tagged_cell: Option<crate::tagged_table::Cell>,
    outside_mark: Option<i32>,
}

impl RunBuilder {
    fn new() -> Self {
        Self {
            text: String::new(),
            sdk_unicode_mapping_errors: 0,
            spans: Vec::new(),
            bounds: None,
            size: None,
            last_origin: None,
            angle: None,
            vertical: None,
            last_bounds: None,
            last_baseline_end: None,
            first_origin: None,
            layout: None,
            rtl: false,
            tagged_cell: None,
            outside_mark: None,
        }
    }

    fn finish(&mut self, output: &mut Vec<ExtractedRun>, max_blocks: usize) -> Result<()> {
        while self.text.ends_with(char::is_whitespace) {
            self.text.pop();
            self.spans.pop();
        }
        if !self.text.trim().is_empty() {
            ensure!(output.len() < max_blocks, "PDF exceeds max_blocks_per_page {max_blocks}. No partial comparison was performed.");
            output.push(ExtractedRun {
                text: std::mem::take(&mut self.text),
                source_text_region: false,
                source_text_anchor: false,
                source_regions: Vec::new(),
                sdk_unicode_mapping_errors: self.sdk_unicode_mapping_errors,
                bounds: self.bounds.context("Text has no valid PDF bounds")?,
                size: self.size,
                spans: std::mem::take(&mut self.spans),
                origin: self.first_origin.unwrap_or((0.0, 0.0)),
                angle: self.angle.unwrap_or(0.0),
                vertical: self.vertical == Some(true),
                layout: self.layout.context("Text has no PDF origins")?,
                tagged_cell: self.tagged_cell,
                outside_mark: self.outside_mark,
            });
        }
        *self = Self::new();
        Ok(())
    }
}

/// Page rotation describes presentation, not the default user-space text.
/// Pdfium uses it to reorder characters and generate word/line separators.
/// Temporarily query an unrotated page so all display rotations see the same
/// SDK stream. Restore the live page before returning, including on errors.
pub(crate) fn extract_runs(
    page: &mut PdfPage<'_>,
    unicode_facts: &crate::unicode_facts::PageFacts,
    user_unit: Option<f64>,
    max_blocks: usize,
    max_chars: u64,
) -> Result<ExtractedPage> {
    let rotation = page
        .rotation()
        .context("Failed to read text page rotation")?;
    if rotation == PdfPageRenderRotation::None {
        return extract_unrotated_runs(page, unicode_facts, user_unit, max_blocks, max_chars);
    }
    page.set_rotation(PdfPageRenderRotation::None);
    let result = (|| {
        ensure!(
            page.rotation()? == PdfPageRenderRotation::None,
            "Failed to neutralize text page rotation"
        );
        extract_unrotated_runs(page, unicode_facts, user_unit, max_blocks, max_chars)
    })();
    page.set_rotation(rotation);
    ensure!(
        page.rotation()? == rotation,
        "Failed to restore text page rotation"
    );
    result
}

fn extract_unrotated_runs(
    page: &PdfPage<'_>,
    unicode_facts: &crate::unicode_facts::PageFacts,
    user_unit: Option<f64>,
    max_blocks: usize,
    max_chars: u64,
) -> Result<ExtractedPage> {
    // Coordinates and exported font sizes stay raw. Absolute layout floors are
    // physical points, converted once to this page's coordinate basis.
    let scale = unicode_facts.coordinate_scale;
    let point = scale / user_unit.unwrap_or(1.0);
    let text = page.text().context("Failed to extract PDF text")?;
    ensure!(
        text.len() >= 0 && text.len() as usize == unicode_facts.characters,
        "Unicode diagnostic character count changed while opening"
    );
    // Check before PdfPageText::chars() allocates a vector of every character index.
    ensure!(
        text.len() >= 0 && text.len() as u64 <= max_chars.min(2_000_000),
        "PDF extracted text exceeds the character budget. No partial comparison was performed."
    );
    ensure!(
        max_blocks > 0 || text.is_empty() || unicode_facts.projected_empty,
        "PDF exceeds max_blocks_per_page 0"
    );
    let chars = text.chars();
    let mut output = Vec::new();
    let mut run = RunBuilder::new();
    let mut separated = false;
    let mut generated_space = false;
    let mut sdk_unicode_mapping_errors = 0;
    let mut fingerprint = crate::unicode_facts::FINGERPRINT_START;
    let mut projection_fingerprint = crate::unicode_facts::FINGERPRINT_START;
    let mut region_sizes = vec![None; unicode_facts.actual_regions.len()];
    let mut region_mixed = vec![false; unicode_facts.actual_regions.len()];
    let mut region_origins = vec![None; unicode_facts.actual_regions.len()];
    let mut region_errors = vec![0usize; unicode_facts.actual_regions.len()];
    for ch in chars.iter() {
        let raw_mapping_error = unicode_facts.has_error(ch.index());
        sdk_unicode_mapping_errors += usize::from(raw_mapping_error);
        let raw_unicode = ch.unicode_value();
        fingerprint = crate::unicode_facts::fingerprint_add(fingerprint, raw_unicode);
        if unicode_facts.projected(ch.index()) {
            let bounds = ch.tight_bounds().ok().map(|r| {
                [
                    r.left().value,
                    r.bottom().value,
                    r.right().value,
                    r.top().value,
                ]
            });
            let origin = ch.origin().ok().map(|(x, y)| [x.value, y.value]);
            projection_fingerprint = crate::unicode_facts::geometry_add(
                projection_fingerprint,
                ch.index(),
                bounds,
                origin,
            );
            run.finish(&mut output, max_blocks)?;
            separated = true;
            generated_space = false;
            if let Some(group) = unicode_facts.projected_group(ch.index()) {
                let size = f64::from(ch.scaled_font_size().value.abs());
                if size.is_finite() && size > 0.0 {
                    if region_sizes[group]
                        .is_some_and(|old: f64| (old - size).abs() > 0.001 * point)
                    {
                        region_mixed[group] = true;
                    } else {
                        region_sizes[group] = Some(size);
                    }
                } else {
                    region_mixed[group] = true;
                }
                if region_origins[group].is_none() {
                    region_origins[group] = ch
                        .origin()
                        .ok()
                        .map(|(x, y)| (f64::from(x.value), f64::from(y.value)));
                }
                region_errors[group] += usize::from(raw_mapping_error);
            }
            continue;
        }
        // An omitted marked separator may sit between otherwise touching SDK
        // objects. Keep those objects separate until source regions are ordered.
        if unicode_facts.preserve_object_boundaries && unicode_facts.starts_text_object(ch.index())
        {
            run.finish(&mut output, max_blocks)?;
        }
        if unicode_facts.is_paired_low(ch.index()) {
            continue; // Its raw scalar/error still contributed to the checks above.
        }
        let mut character_errors = usize::from(raw_mapping_error);
        let scalar = if unicode_facts.is_paired_low(ch.index() + 1) {
            character_errors += usize::from(unicode_facts.has_error(ch.index() + 1));
            let low = chars
                .get(ch.index() + 1)
                .context("Missing paired Unicode character")?
                .unicode_value();
            Some(
                crate::unicode_facts::surrogate_scalar(raw_unicode, low)
                    .context("Paired Unicode characters changed during extraction")?,
            )
        } else {
            char::from_u32(raw_unicode)
        };
        let mapping_error = character_errors > 0;
        let unicode = match scalar {
            // This flag can accompany printable fallback scalars or controls
            // such as CR. Check it before whitespace/control filtering so that
            // neither becomes apparently valid text nor silently disappears.
            // Retain printable best-effort scalars (including Type3 names),
            // but make an error mapped as a separator visible at its glyph.
            Some(c) if mapping_error && (c.is_control() || c.is_whitespace() || c == '\u{ad}') => {
                Some('\u{fffd}')
            }
            None if mapping_error => Some('\u{fffd}'),
            // Pdfium rewrites a physically printed line-end hyphen to U+0002.
            // Preserve that glyph and its bounds instead of dropping a control.
            Some('\u{2}') if ch.is_hyphen().context("Failed to inspect PDF hyphen")? => Some('-'),
            other => other,
        };
        if unicode == Some('\u{ad}') {
            continue;
        }
        if matches!(unicode, Some('\r' | '\n')) {
            // Pdfium inserts different CR/LF boundaries after display rotation,
            // including between font faces on the same baseline. Geometry is
            // authoritative for grouping; these generated separators are hints.
            separated = true;
            continue;
        }
        let rect = ch.tight_bounds().or_else(|_| ch.loose_bounds()).ok();
        let origin = ch
            .origin()
            .ok()
            .map(|(x, y)| (f64::from(x.value), f64::from(y.value)));
        let bounds = rect
            .map(|r| Bounds {
                x0: f64::from(r.left().value.min(r.right().value)),
                y0: f64::from(r.bottom().value.min(r.top().value)),
                x1: f64::from(r.left().value.max(r.right().value)),
                y1: f64::from(r.bottom().value.max(r.top().value)),
            })
            .or_else(|| {
                origin.map(|(x, y)| Bounds {
                    x0: x,
                    y0: y,
                    x1: x,
                    y1: y,
                })
            });
        let c = match unicode {
            Some(c) if !c.is_control() || c.is_whitespace() => c,
            // A visible glyph with no Unicode mapping cannot silently disappear.
            None | Some('\0')
                if !ch.is_generated().unwrap_or(false)
                    && bounds.is_some_and(|b| b.width() > 0.0 && b.height() > 0.0) =>
            {
                '\u{fffd}'
            }
            _ => continue,
        };
        if c.is_whitespace() {
            if !run.text.is_empty() {
                let generated = ch
                    .is_generated()
                    .context("Failed to inspect PDF word separator")?;
                let last = run.spans.last().copied().unwrap_or((0.0, 0.0));
                // Synthetic or empty space boxes can contain unscaled font
                // metrics under a page CTM, including encoded RTL spaces.
                // A blank has no ink bounds: anchor it to real neighboring ink.
                let ink = bounds.filter(|b| {
                    [b.x0, b.y0, b.x1, b.y1].into_iter().all(f64::is_finite)
                        && b.width() > 0.0
                        && b.height() > 0.0
                });
                let span = match (generated, ink) {
                    (false, Some(b)) => (b.x0, b.x1),
                    _ => {
                        let edge = run.last_bounds.map(|b| b.x1).unwrap_or(last.1);
                        (edge, edge)
                    }
                };
                run.text.push(c);
                run.spans.push(span);
                generated_space |= generated;
            }
            continue;
        }
        let bounds = bounds.context("PDF character has no bounds or origin")?;
        ensure!(
            [bounds.x0, bounds.y0, bounds.x1, bounds.y1]
                .into_iter()
                .all(f64::is_finite),
            "PDF character has non-finite bounds"
        );
        let origin = origin.unwrap_or_else(|| bounds.center());
        ensure!(
            origin.0.is_finite() && origin.1.is_finite(),
            "PDF character has a non-finite origin"
        );
        let size = f64::from(ch.scaled_font_size().value.abs());
        let size = (size.is_finite() && size > 0.0).then_some(size);
        // The SDK angle uses atan2(c,a), so a horizontal glyph's italic
        // shear can appear rotated. The transformed text baseline is (a,b).
        // Preserve the SDK's clockwise convention for true rotations.
        let matrix = ch.matrix().ok().map(|m| {
            [
                f64::from(m.a()),
                f64::from(m.b()),
                f64::from(m.c()),
                f64::from(m.d()),
            ]
        });
        let angle = matrix
            .and_then(baseline_angle)
            .or_else(|| ch.angle_degrees().ok().map(f64::from));
        let oblique = angle.is_some_and(|a| {
            let a = a.rem_euclid(90.0);
            a.min(90.0 - a) > 0.1
        });
        let frame = oblique
            .then(|| {
                matrix.and_then(|m| BaselineFrame::new(m, f64::from(ch.unscaled_font_size().value)))
            })
            .flatten();
        let oblique = frame.is_some();
        let baseline_end = frame.and_then(|f| {
            let loose = ch.loose_bounds().ok()?;
            f.end(
                origin,
                (
                    f64::from(loose.left().value) / 2.0 + f64::from(loose.right().value) / 2.0,
                    f64::from(loose.bottom().value) / 2.0 + f64::from(loose.top().value) / 2.0,
                ),
            )
        });
        let content = unicode_facts.tagged_content(ch.index());
        let tagged_cell = content.cell;
        if run.tagged_cell != tagged_cell {
            run.finish(&mut output, max_blocks)?;
            run.tagged_cell = tagged_cell;
        }
        let em = size.or(run.size).unwrap_or(12.0 * point).max(point);
        let combining = unicode_normalization::char::canonical_combining_class(c) != 0
            || matches!(c, '\u{200c}' | '\u{200d}' | '\u{fe0e}' | '\u{fe0f}');
        if let Some((x, y)) = run.last_origin {
            let dx = (origin.0 - x).abs();
            let dy = (origin.1 - y).abs();
            // Generated spaces or line breaks can separate adjacent columns on
            // the same baseline. Do not join a word boundary across that gutter.
            let word_boundary = separated || generated_space;
            let horizontal = dy <= em * 0.35 && dx <= em * if word_boundary { 1.5 } else { 3.0 };
            let upright = angle.is_some_and(|a| {
                let a = a.rem_euclid(360.0);
                a.min(360.0 - a) < 0.1
            });
            // The SDK can visit separate same-font objects out of x order
            // after display rotation. Do not bake a backward jump into an
            // upright LTR run before geometric canonicalization can sort it.
            let backtracking = horizontal
                && run.vertical != Some(true)
                && !run.rtl
                && !is_rtl(c)
                && upright
                && origin.0 < x - em * 0.1;
            // The same geometric word gap must be canonicalized whether the
            // SDK visits adjacent PDF text objects forwards or backwards.
            let object_gap = horizontal
                && upright
                && !run.rtl
                && !is_rtl(c)
                && unicode_facts.starts_text_object(ch.index())
                && !run.text.ends_with(char::is_whitespace)
                && run.last_bounds.is_some_and(|previous| {
                    (bounds.x0 - previous.x1).max(previous.x0 - bounds.x1) > em * 0.15
                });
            let cjk = |c: char| matches!(c, '\u{3000}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '\u{20000}'..='\u{323af}');
            let vertical_writing = angle.is_some_and(|a| (a.rem_euclid(180.0) - 90.0).abs() < 1.0)
                || (cjk(c) && run.text.chars().next_back().is_some_and(cjk));
            let vertical = vertical_writing && dx <= em * 0.35 && dy <= em * 1.8;
            let oblique_step = frame.and_then(|f| f.displacement((x, y), origin));
            let oblique_forward = oblique_step.is_some_and(|(along, across)| {
                across.abs() <= 0.35
                    && along >= -0.1
                    && along <= if word_boundary { 1.5 } else { 3.0 }
            });
            let oblique_rtl = run.rtl
                && is_rtl(c)
                && oblique_step.is_some_and(|(along, across)| {
                    across.abs() <= 0.35 && along.abs() <= if word_boundary { 1.5 } else { 3.0 }
                });
            let oblique_vertical = cjk(c)
                && run.text.chars().next_back().is_some_and(cjk)
                && oblique_step
                    .is_some_and(|(along, across)| along.abs() <= 0.35 && across.abs() <= 1.8);
            let contiguous = if oblique {
                match run.vertical {
                    Some(true) => oblique_vertical,
                    Some(false) => oblique_forward || oblique_rtl,
                    None => oblique_forward || oblique_rtl || oblique_vertical,
                }
            } else {
                match run.vertical {
                    Some(true) => vertical,
                    Some(false) => horizontal,
                    None => horizontal || vertical,
                }
            };
            let same_size = match (run.size, size) {
                (Some(a), Some(b)) => (a - b).abs() <= 0.1 * point,
                _ => true,
            };
            let same_angle = run
                .angle
                .zip(angle)
                .is_none_or(|(old, new)| (old - new).abs() < 0.1);
            if !combining
                && (!contiguous || !same_size || !same_angle || backtracking || object_gap)
            {
                run.finish(&mut output, max_blocks)?;
            } else if run.vertical.is_none() && dx.max(dy) > em * 0.1 {
                run.vertical = Some(if oblique {
                    oblique_vertical && !oblique_forward
                } else {
                    dy > dx
                });
            }
        }
        let object_boundary = unicode_facts.starts_text_object(ch.index());
        if (separated || oblique && object_boundary)
            && !run.text.is_empty()
            && !run.text.ends_with(char::is_whitespace)
        {
            if let Some(previous) = run.last_bounds {
                let space = if oblique {
                    frame.is_some_and(|f| {
                        let gap = if run.rtl && is_rtl(c) {
                            baseline_end
                                .zip(run.last_origin)
                                .and_then(|(end, start)| f.displacement(end, start))
                        } else {
                            run.last_baseline_end
                                .and_then(|end| f.displacement(end, origin))
                        };
                        gap.is_some_and(|(along, across)| along > 0.15 && across.abs() <= 0.35)
                    })
                } else {
                    (bounds.x0 - previous.x1).max(previous.x0 - bounds.x1) > em * 0.15
                };
                if run.vertical != Some(true) && space {
                    run.text.push(' ');
                    run.spans.push((previous.x1, previous.x1));
                }
            }
        }
        separated = false;
        generated_space = false;
        if run.text.is_empty() {
            run.outside_mark = content.outside;
        } else if run.outside_mark != content.outside {
            // Mixed structural marks are not an unambiguous orphan cell.
            run.outside_mark = None;
        }
        run.text.push(c);
        run.tagged_cell = tagged_cell;
        run.rtl |= is_rtl(c);
        run.sdk_unicode_mapping_errors += character_errors;
        run.first_origin.get_or_insert(origin);
        run.layout = Some(match run.layout {
            Some(b) => Bounds {
                x0: b.x0.min(origin.0),
                x1: b.x1.max(origin.0),
                y0: b.y0.min(origin.1),
                y1: b.y1.max(origin.1),
            },
            None => Bounds {
                x0: origin.0,
                x1: origin.0,
                y0: origin.1,
                y1: origin.1,
            },
        });
        run.spans.push((bounds.x0, bounds.x1));
        run.bounds = Some(match run.bounds {
            Some(b) => Bounds {
                x0: b.x0.min(bounds.x0),
                y0: b.y0.min(bounds.y0),
                x1: b.x1.max(bounds.x1),
                y1: b.y1.max(bounds.y1),
            },
            None => bounds,
        });
        if !combining {
            run.last_origin = Some(origin);
            run.last_bounds = Some(bounds);
            run.last_baseline_end = baseline_end;
            run.size = run.size.or(size);
            run.angle = angle;
        }
    }
    run.finish(&mut output, max_blocks)?;
    ensure!(
        fingerprint == unicode_facts.fingerprint
            && projection_fingerprint == unicode_facts.projection_fingerprint
            && sdk_unicode_mapping_errors == unicode_facts.errors,
        "Unicode diagnostics changed while opening the immutable PDF"
    );
    for (index, region) in unicode_facts.actual_regions.iter().enumerate() {
        let text: String = region
            .text
            .chars()
            .filter(|c| !c.is_control() || c.is_whitespace())
            .collect();
        if text.is_empty() || unicode_facts.projected_empty && text.trim().is_empty() {
            continue;
        }
        ensure!(
            output.len() < max_blocks,
            "PDF exceeds max_blocks_per_page {max_blocks}. No partial comparison was performed."
        );
        let origin = region
            .origin
            .or(region_origins[index])
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .unwrap_or((region.bounds.x0, region.bounds.y1));
        let spans = vec![(region.bounds.x0, region.bounds.x1); text.chars().count()];
        output.push(ExtractedRun {
            text,
            source_text_region: true,
            source_text_anchor: region.anchor,
            source_regions: vec![crate::render::SourceTextRegion {
                start: 0,
                end: spans.len(),
                bounds: region.bounds,
                anchor: region.anchor,
            }],
            sdk_unicode_mapping_errors: region_errors[index],
            bounds: region.bounds,
            size: if region_mixed[index] {
                None
            } else {
                region_sizes[index].or(region.font_size)
            },
            spans,
            origin,
            angle: region.angle,
            vertical: false,
            layout: Bounds {
                x0: region.bounds.x0.min(origin.0),
                y0: region.bounds.y0.min(origin.1),
                x1: region.bounds.x1.max(origin.0),
                y1: region.bounds.y1.max(origin.1),
            },
            tagged_cell: region.content.cell,
            outside_mark: region.content.outside,
        });
    }
    let reading_order_requires_review =
        table::retain_complete(&mut output, point, &unicode_facts.table_declarations)
            || unicode_facts.reading_order_requires_review;
    let mut runs = canonicalize_runs(output, point);
    runs.retain(|run| !run.text.trim().is_empty());
    for run in &mut runs {
        if !run.source_text_region {
            anchor_blank_spans(&run.text, &mut run.spans);
        }
        if scale != 1.0 {
            run.bounds.x0 /= scale;
            run.bounds.y0 /= scale;
            run.bounds.x1 /= scale;
            run.bounds.y1 /= scale;
            run.size = run.size.map(|x| x / scale);
            for (x0, x1) in &mut run.spans {
                *x0 /= scale;
                *x1 /= scale;
            }
            for region in &mut run.source_regions {
                region.bounds.x0 /= scale;
                region.bounds.x1 /= scale;
                region.bounds.y0 /= scale;
                region.bounds.y1 /= scale;
            }
            run.origin.0 /= scale;
            run.origin.1 /= scale;
            run.layout.x0 /= scale;
            run.layout.y0 /= scale;
            run.layout.x1 /= scale;
            run.layout.y1 /= scale;
        }
    }
    Ok(ExtractedPage {
        runs,
        source_characters: unicode_facts.work_characters as u64,
        sdk_unicode_mapping_errors,
        reading_order_requires_review,
    })
}

fn anchor_blank_spans(text: &str, spans: &mut [(f64, f64)]) {
    let mut blank_start = None;
    for (i, c) in text.chars().enumerate() {
        if c.is_whitespace() {
            blank_start.get_or_insert(i);
        } else if let Some(start) = blank_start.take().filter(|&s| s > 0) {
            let previous = spans[start - 1];
            // Infer the local direction from real neighboring ink, so RTL and
            // mirrored text use the left edge rather than jumping back right.
            let edge = if spans[i].0 < previous.0 {
                previous.0
            } else {
                previous.1
            };
            for span in &mut spans[start..i] {
                if span.0 == span.1 {
                    *span = (edge, edge);
                }
            }
        }
    }
}

/// Invert only the linear text transform, then measure distances in ems.
/// This retains baseline direction under rotation, shear and anisotropic scale.
#[derive(Clone, Copy)]
struct BaselineFrame {
    matrix: [f64; 4],
    determinant: f64,
    font_size: f64,
}

impl BaselineFrame {
    fn new(matrix: [f64; 4], font_size: f64) -> Option<Self> {
        let [a, b, c, d] = matrix;
        let determinant = a * d - b * c;
        (matrix.into_iter().all(f64::is_finite)
            && font_size.is_finite()
            && font_size != 0.0
            && determinant.is_finite()
            && determinant != 0.0)
            .then_some(Self {
                matrix,
                determinant,
                font_size,
            })
    }

    fn displacement(self, from: (f64, f64), to: (f64, f64)) -> Option<(f64, f64)> {
        let [a, b, c, d] = self.matrix;
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let along = (d * dx - c * dy) / self.determinant / self.font_size;
        let across = (-b * dx + a * dy) / self.determinant / self.font_size;
        (along.is_finite() && across.is_finite()).then_some((along, across))
    }

    fn end(self, origin: (f64, f64), loose_center: (f64, f64)) -> Option<(f64, f64)> {
        // The pinned SDK constructs a horizontal loose glyph cell from its
        // source advance width, transforms it, and unions the ink box. Its
        // center remains useful for word spacing after inverse transformation.
        // Bad/overhanging metrics can widen that cell, so retain only finite,
        // positive, bounded estimates and never change exported glyph bounds.
        let (half, _) = self.displacement(origin, loose_center)?;
        let advance = half * 2.0;
        if !(advance > 0.0 && advance <= 3.0) {
            return None;
        }
        let [a, b, _, _] = self.matrix;
        let end = (
            origin.0 + a * advance * self.font_size,
            origin.1 + b * advance * self.font_size,
        );
        (end.0.is_finite() && end.1.is_finite()).then_some(end)
    }
}

fn baseline_angle([a, b, c, d]: [f64; 4]) -> Option<f64> {
    let length = a.hypot(b);
    let determinant = a * d - b * c;
    if [a, b, c, d, length, determinant]
        .into_iter()
        .all(f64::is_finite)
        && length > 0.0
        && determinant != 0.0
    {
        Some((-b).atan2(a).to_degrees().rem_euclid(360.0))
    } else {
        None
    }
}

fn is_rtl(c: char) -> bool {
    matches!(c, '\u{590}'..='\u{8ff}' | '\u{fb1d}'..='\u{fdff}' | '\u{fe70}'..='\u{feff}')
}

fn canonicalize_runs(runs: Vec<ExtractedRun>, point: f64) -> Vec<ExtractedRun> {
    // A rotated page may return all italic text before the regular text, even
    // when they share a line. Collect by baseline before joining adjacent runs.
    let mut rows: Vec<Vec<ExtractedRun>> = Vec::new();
    let mut buckets: HashMap<(i64, i64, i64, Option<crate::tagged_table::Cell>), Vec<usize>> =
        HashMap::new();
    let mut result = Vec::new();
    for run in runs {
        if run.vertical || run.angle.rem_euclid(180.0).abs() > 0.1 {
            result.push(run);
            continue;
        }
        let em = run.size.unwrap_or(12.0 * point).max(point);
        let tolerance = (em * 0.1).max(0.25 * point);
        let font = (em / point * 10.0).round() as i64;
        let angle = (run.angle * 10.0).round() as i64;
        let y = (run.origin.1 / tolerance).floor() as i64;
        let mut row = None;
        for offset in -1i64..=1 {
            if let Some(candidates) =
                buckets.get(&(font, angle, y.saturating_add(offset), run.tagged_cell))
            {
                row = candidates
                    .iter()
                    .copied()
                    .find(|&i| (rows[i][0].origin.1 - run.origin.1).abs() <= tolerance);
                if row.is_some() {
                    break;
                }
            }
        }
        if let Some(i) = row {
            rows[i].push(run);
        } else {
            buckets
                .entry((font, angle, y, run.tagged_cell))
                .or_default()
                .push(rows.len());
            rows.push(vec![run]);
        }
    }
    let mut directions = Vec::with_capacity(rows.len());
    for row in &mut rows {
        let rtl_count = row
            .iter()
            .flat_map(|r| r.text.chars())
            .filter(|&c| is_rtl(c))
            .count();
        let ltr_count = row
            .iter()
            .flat_map(|r| r.text.chars())
            .filter(|c| c.is_alphabetic() && !is_rtl(*c))
            .count();
        let rtl = rtl_count > ltr_count;
        directions.push(rtl);
        row.sort_by(|a, b| {
            if rtl {
                b.layout.x0.total_cmp(&a.layout.x0)
            } else {
                a.layout.x0.total_cmp(&b.layout.x0)
            }
            .then_with(|| {
                // At a shared origin, a declared separator precedes the next
                // word even when the SDK omitted the separator's records.
                b.text.trim().is_empty().cmp(&a.text.trim().is_empty())
            })
        });
    }
    let barriers = inline::barriers(&rows, point);
    for ((row, barriers), rtl) in rows.into_iter().zip(barriers).zip(directions) {
        let mut iter = row.into_iter();
        let Some(mut current) = iter.next() else {
            continue;
        };
        for (next, barrier) in iter.zip(barriers) {
            let em = current.size.unwrap_or(12.0 * point).max(point);
            let gap = if rtl {
                current.layout.x0 - next.layout.x1
            } else {
                next.layout.x0 - current.layout.x1
            };
            // Form dot leaders can start after a deliberately wider word gap.
            // Keep them attached across SDK-generated rotation line breaks.
            let join_gap = if matches!(current.text.trim(), "•" | "◦" | "‣") {
                2.5
            } else if is_dot_leader(&next.text) {
                2.0
            } else {
                1.5
            };
            if barrier || gap > em * join_gap {
                result.push(current);
                current = next;
                continue;
            }
            let ink_gap = if rtl {
                current.bounds.x0 - next.bounds.x1
            } else {
                next.bounds.x0 - current.bounds.x1
            };
            if ink_gap > em * 0.15
                && !current.text.ends_with(char::is_whitespace)
                && !next.text.starts_with(char::is_whitespace)
            {
                current.text.push(' ');
                current.spans.push((
                    if rtl {
                        current.bounds.x0
                    } else {
                        current.bounds.x1
                    },
                    if rtl {
                        current.bounds.x0
                    } else {
                        current.bounds.x1
                    },
                ));
            }
            let offset = current.spans.len();
            current
                .source_regions
                .extend(next.source_regions.into_iter().map(|mut region| {
                    region.start += offset;
                    region.end += offset;
                    region
                }));
            current.source_text_region |= next.source_text_region;
            current.source_text_anchor |= next.source_text_anchor;
            current.sdk_unicode_mapping_errors += next.sdk_unicode_mapping_errors;
            current.text.push_str(&next.text);
            current.spans.extend(next.spans);
            current.bounds = Bounds {
                x0: current.bounds.x0.min(next.bounds.x0),
                y0: current.bounds.y0.min(next.bounds.y0),
                x1: current.bounds.x1.max(next.bounds.x1),
                y1: current.bounds.y1.max(next.bounds.y1),
            };
            current.layout = Bounds {
                x0: current.layout.x0.min(next.layout.x0),
                x1: current.layout.x1.max(next.layout.x1),
                y0: current.layout.y0.min(next.layout.y0),
                y1: current.layout.y1.max(next.layout.y1),
            };
        }
        result.push(current);
    }
    // Preserve columns when there is an uninterrupted vertical gutter. A global
    // top-to-bottom sort would interleave the two columns of a paper.
    let vertical = result.iter().filter(|r| r.vertical).count() > result.len() / 2;
    if !vertical {
        return table::sort(result, point);
    }
    // Origins come from PDF positioning operators and are independent of the
    // platform's substitute font ink metrics (notably native vs browser CID fonts).
    let mut intervals: Vec<_> = result.iter().map(|r| (r.layout.x0, r.layout.x1)).collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut columns: Vec<(f64, f64)> = Vec::new();
    for (left, right) in intervals {
        if let Some(last) = columns
            .last_mut()
            .filter(|last| left <= last.1 + 2.0 * point)
        {
            last.1 = last.1.max(right);
        } else {
            columns.push((left, right));
        }
    }
    let column = |r: &ExtractedRun| {
        columns
            .partition_point(|c| c.0 <= r.layout.x0)
            .saturating_sub(1)
    };
    result.sort_by(|a, b| {
        let order = if vertical {
            column(b).cmp(&column(a))
        } else {
            column(a).cmp(&column(b))
        };
        order
            .then_with(|| b.origin.1.total_cmp(&a.origin.1))
            .then_with(|| a.layout.x0.total_cmp(&b.layout.x0))
    });
    result
}

fn is_dot_leader(text: &str) -> bool {
    let mut dots = 0;
    for c in text.chars() {
        if c == '.' {
            dots += 1;
        } else if !c.is_whitespace() {
            return false;
        }
    }
    dots >= 3
}

#[cfg(test)]
mod baseline_tests {
    use super::baseline_angle;
    #[test]
    fn baseline_direction_ignores_italic_shear_and_retains_actual_rotation() {
        for shear in [-2.0, -1.0 / 3.0, 0.0, 1.0 / 3.0, 2.0] {
            for scale in [1.0 / 1024.0, 1.0, 64.0, 1024.0] {
                for degrees in [0.0f64, 15.0, 90.0, 180.0, 270.0, 345.0] {
                    let (sin, cos) = degrees.to_radians().sin_cos();
                    let matrix = [
                        scale * cos,
                        scale * sin,
                        scale * (shear * cos - sin),
                        scale * (shear * sin + cos),
                    ];
                    let got = baseline_angle(matrix).unwrap();
                    let expected = (-degrees).rem_euclid(360.0);
                    let delta = (got - expected).rem_euclid(360.0);
                    assert!(
                        delta.min(360.0 - delta) < 1e-9,
                        "{matrix:?}: {got} vs {expected}"
                    );
                }
            }
        }
        assert_eq!(baseline_angle([-1.0, 0.0, 0.0, 1.0]), Some(180.0));
        for matrix in [
            [0.0; 4],
            [1.0, 0.0, 2.0, 0.0],
            [f64::NAN, 0.0, 0.0, 1.0],
            [1.0, f64::INFINITY, 0.0, 1.0],
        ] {
            assert!(baseline_angle(matrix).is_none());
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
use crate::appearance_fixture as rotation_fixture;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod rotation_restore_tests {
    use super::*;
    #[test]
    fn diagnostic_rotation_restores_after_nested_object_limit() {
        let bytes = super::rotation_fixture::unobserved_text(1.0, 90, true, 34, false, false);
        let pdfium = crate::render::bind().unwrap();
        let limits = crate::Limits::default();
        let units = crate::page_units::from_bytes_for_text(&bytes, 1, 0..1);
        crate::input::with_raw_bytes(&pdfium, &bytes, &limits, |handle| {
            let result = crate::unicode_facts::read(&pdfium, handle, &limits, None, &units);
            assert!(result
                .err()
                .unwrap()
                .to_string()
                .contains("nesting exceeds work budget"));
            let b = pdfium.bindings();
            let page = b.FPDF_LoadPage(handle.cast(), 0);
            assert_eq!(b.FPDFPage_GetRotation(page), 1);
            b.FPDF_ClosePage(page);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn live_page_rotation_and_pixels_are_restored_on_success_and_extraction_errors() {
        let bytes = super::rotation_fixture::angled_label_layout(1.0, 90, true, false);
        let pdfium = crate::render::bind().unwrap();
        let limits = crate::Limits::default();
        let units = crate::page_units::from_bytes_for_text(&bytes, 1, 0..1);
        let (document, facts) = crate::input::load_bytes_with(&pdfium, &bytes, &limits, |handle| {
            let b = pdfium.bindings();
            let page = b.FPDF_LoadPage(handle.cast(), 0);
            assert_eq!(b.FPDFPage_GetRotation(page), 1);
            b.FPDF_ClosePage(page);
            let facts = crate::unicode_facts::read(&pdfium, handle, &limits, None, &units)?;
            let page = b.FPDF_LoadPage(handle.cast(), 0);
            assert_eq!(
                b.FPDFPage_GetRotation(page),
                1,
                "raw diagnostic pass changed live page rotation"
            );
            b.FPDF_ClosePage(page);
            Ok(facts)
        })
        .unwrap();
        let mut page = document.pages().get(0).unwrap();
        let pixels = |page: &PdfPage| {
            page.render_with_config(&PdfRenderConfig::new().set_target_width(240))
                .unwrap()
                .as_raw_bytes()
        };
        let original = pixels(&page);
        for (max_blocks, max_chars) in [(20_000, 0), (1, 2_000_000)] {
            assert!(extract_runs(&mut page, &facts[0], Some(1.0), max_blocks, max_chars).is_err());
            assert_eq!(page.rotation().unwrap(), PdfPageRenderRotation::Degrees90);
            assert_eq!(pixels(&page), original);
        }
        let extracted = extract_runs(&mut page, &facts[0], Some(1.0), 20_000, 2_000_000).unwrap();
        assert!(!extracted.runs.is_empty());
        assert_eq!(page.rotation().unwrap(), PdfPageRenderRotation::Degrees90);
        assert_eq!(pixels(&page), original);
    }
}

#[cfg(test)]
mod baseline_frame_tests {
    use super::BaselineFrame;
    #[test]
    fn text_frame_preserves_local_distances_and_advance_under_affine_transforms() {
        for degrees in [15.0_f64, 45.0, 75.0, 135.0, 225.0, 315.0] {
            for shear in [-0.5, 0.0, 0.5] {
                for (sx, sy) in [(0.25, 2.0), (1.0, 1.0), (2.0, 0.25)] {
                    let (sin, cos) = degrees.to_radians().sin_cos();
                    let m = [
                        cos * sx,
                        sin * sx,
                        (cos * shear - sin) * sy,
                        (sin * shear + cos) * sy,
                    ];
                    let frame = BaselineFrame::new(m, 12.0).unwrap();
                    let transform =
                        |x: f64, y: f64| (100.0 + m[0] * x + m[2] * y, 200.0 + m[1] * x + m[3] * y);
                    let origin = transform(0.0, 0.0);
                    let delta = frame.displacement(origin, transform(7.2, 3.0)).unwrap();
                    assert!((delta.0 - 0.6).abs() < 1e-12 && (delta.1 - 0.25).abs() < 1e-12);
                    // Axis-aligned box centers are the transformed rectangle centers.
                    let end = frame.end(origin, transform(3.6, 4.0)).unwrap();
                    let expected = transform(7.2, 0.0);
                    assert!(
                        (end.0 - expected.0).abs() < 1e-10 && (end.1 - expected.1).abs() < 1e-10
                    );
                }
            }
        }
        for m in [[0.0; 4], [1.0, 0.0, 2.0, 0.0], [f64::NAN, 0.0, 0.0, 1.0]] {
            assert!(BaselineFrame::new(m, 12.0).is_none());
        }
        for size in [0.0, f64::INFINITY] {
            assert!(BaselineFrame::new([1.0, 0.0, 0.0, 1.0], size).is_none());
        }
        let signed = BaselineFrame::new([1.0, 0.0, 0.0, 1.0], -12.0).unwrap();
        assert_eq!(
            signed.displacement((0.0, 0.0), (-6.0, 0.0)),
            Some((0.5, -0.0))
        );
        assert_eq!(signed.end((0.0, 0.0), (-3.0, 0.0)), Some((-6.0, 0.0)));
        let frame = BaselineFrame::new([1.0, 0.0, 0.0, 1.0], 12.0).unwrap();
        assert!(frame
            .displacement((0.0, 0.0), (f64::INFINITY, 0.0))
            .is_none());
        assert!(frame.end((0.0, 0.0), (0.0, 0.0)).is_none());
        assert!(frame.end((0.0, 0.0), (100.0, 0.0)).is_none());
    }
}
