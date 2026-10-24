//! Rendered appearance and interactive widget comparison, using the same Pdfium instance.

use anyhow::{ensure, Context, Result};
use image::RgbaImage;
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{Bounds, DocumentDiff, Limits, PageViewport, Status};

pub(crate) struct SourceFacts {
    pub same_source: bool,
    pub old_units: crate::page_units::PageUnits,
    pub new_units: crate::page_units::PageUnits,
    pub old_unicode: Vec<crate::unicode_facts::PageFacts>,
    pub new_unicode: Vec<crate::unicode_facts::PageFacts>,
    pub old_canonical: std::result::Result<crate::canonical_forms::FormSnapshot, String>,
    pub new_canonical: std::result::Result<crate::canonical_forms::FormSnapshot, String>,
    pub old_widgets: crate::widget_values::WidgetValues,
    pub new_widgets: crate::widget_values::WidgetValues,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualDiffOptions {
    /// Sum of declared image source pixels per page, including nested forms.
    pub max_source_image_pixels: u64,
    /// Raster comparison resolution in physical dots per inch, including UserUnit.
    pub dpi: u32,
    /// Sum of old and new raster pixels over the whole comparison.
    pub max_total_pixels: u64,
    /// Maximum per-channel difference to ignore, including alpha.
    pub channel_tolerance: u8,
}

impl Default for VisualDiffOptions {
    fn default() -> Self {
        Self {
            max_source_image_pixels: crate::raster_limits::DEFAULT_SOURCE_IMAGE_PIXELS,
            dpi: 144,
            max_total_pixels: 200_000_000,
            channel_tolerance: 0,
        }
    }
}

impl VisualDiffOptions {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.max_source_image_pixels > 0,
            "visual max_source_image_pixels must be positive"
        );
        ensure!(
            (1..=600).contains(&self.dpi),
            "visual dpi must be between 1 and 600"
        );
        ensure!(
            self.max_total_pixels > 0,
            "visual max_total_pixels must be positive"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualStatus {
    IdenticalSource,
    Unchanged,
    Changed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualPageDiff {
    pub status: VisualStatus,
    /// Present only when equal-size rasters were actually compared.
    pub changed_pixels: Option<u64>,
    /// Enclosing changed region, in each page's unrotated PDF user space.
    pub old_bounds: Option<Bounds>,
    pub new_bounds: Option<Bounds>,
    pub old_viewport: Option<PageViewport>,
    pub new_viewport: Option<PageViewport>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComparisonCoverage {
    pub identical_source: bool,
    pub visual_dpi: Option<u32>,
    pub channel_tolerance: Option<u8>,
    pub max_total_pixels: Option<u64>,
    pub max_source_image_pixels: Option<u64>,
    pub rendered_page_pairs: usize,
    pub skipped_page_pairs: usize,
    /// Pixels charged to the budget, including failed rendering attempts.
    pub pixel_budget_used: u64,
    /// Widgets and available AcroForm records are read, but full interactive
    /// semantics, signatures, XFA and annotations still require review.
    pub interactive_pages_requiring_review: usize,
    /// Inputs whose empty AcroForm shells pass the bounded no-field/widget/action proof.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub verified_empty_acroform_shells: usize,
    pub widget_fields_examined: usize,
    pub canonical_fields_examined: usize,
    pub canonical_properties_unavailable: usize,
    pub canonical_form_review_reasons: Vec<String>,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

// Custom decoding returns validated constants, never references into input JSON.
type FieldKind = &'static str;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormFieldSnapshot {
    pub annotation_index: usize,
    pub name: Option<String>,
    #[serde(deserialize_with = "deserialize_field_kind")]
    pub kind: FieldKind,
    pub group_index: Option<u32>,
    /// Effective inherited /Ff field flags (read-only, required, export behavior, etc.).
    pub flags: u32,
    /// Canonical scalar value. For multiple selections, Pdfium exposes the first
    /// export value; selected_options contains every selected index and label.
    pub value: Option<String>,
    pub display_value: Option<String>,
    pub checked: Option<bool>,
    /// Per-control export value, including unchecked checkbox/radio controls.
    pub export_value: Option<String>,
    pub selected_options: Vec<(usize, Option<String>)>,
    pub bounds: Bounds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormFieldChange {
    pub status: Status,
    pub old: Option<FormFieldSnapshot>,
    pub new: Option<FormFieldSnapshot>,
}

fn deserialize_field_kind<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<&'static str, D::Error> {
    match String::deserialize(deserializer)?.as_str() {
        "unknown" => Ok("unknown"),
        "text" => Ok("text"),
        "checkbox" => Ok("checkbox"),
        "radio" => Ok("radio"),
        "combo" => Ok("combo"),
        "list" => Ok("list"),
        _ => Err(serde::de::Error::custom("Unsupported PDF field kind")),
    }
}

fn fields(
    page: &PdfPage<'_>,
    page_number: usize,
    limits: &Limits,
    text_budget: &mut u64,
    widgets: &crate::widget_values::WidgetValues,
) -> Result<(Vec<FormFieldSnapshot>, bool)> {
    let annotations = page.annotations();
    ensure!(
        annotations.len() <= limits.max_blocks_per_page,
        "PDF annotations exceed max_blocks_per_page"
    );
    let mut result = Vec::new();
    let mut needs_review = !annotations.is_empty();
    // Index access propagates errors instead of silently truncating an iterator.
    for i in 0..annotations.len() {
        let annotation = annotations
            .get(i)
            .context("Failed to read PDF annotation")?;
        let Some(field) = annotation.as_form_field() else {
            continue;
        };
        let bounds = annotation
            .bounds()
            .context("Failed to read widget bounds")?;
        let raw = widgets.get(&(page_number, i + 1));
        let mut snapshot = FormFieldSnapshot {
            annotation_index: i + 1,
            name: raw.and_then(|v| v.name.clone()),
            kind: "unknown",
            group_index: raw.and_then(|v| v.group_index),
            flags: raw.map_or(0, |v| v.flags),
            value: raw.and_then(|v| v.value.clone()),
            display_value: None,
            checked: raw.and_then(|v| v.checked),
            export_value: raw.and_then(|v| v.export_value.clone()),
            selected_options: Vec::new(),
            bounds: Bounds {
                x0: bounds.left().value.into(),
                y0: bounds.bottom().value.into(),
                x1: bounds.right().value.into(),
                y1: bounds.top().value.into(),
            },
        };
        match field {
            PdfFormField::Text(_) => {
                snapshot.kind = "text";
            }
            PdfFormField::Checkbox(_) => {
                snapshot.kind = "checkbox";
            }
            PdfFormField::RadioButton(_) => {
                snapshot.kind = "radio";
            }
            PdfFormField::ComboBox(f) => {
                snapshot.kind = "combo";
                let _ = f;
                let choice = widgets
                    .get(&(page_number, i + 1))
                    .context("Missing bounded combo value")?;
                snapshot.value = choice.value.clone();
                snapshot.selected_options = choice.selected.clone();
                snapshot.display_value = choice
                    .selected
                    .first()
                    .and_then(|(_, label)| label.clone())
                    .or_else(|| choice.value.clone());
            }
            PdfFormField::ListBox(f) => {
                snapshot.kind = "list";
                let _ = f;
                let choice = widgets
                    .get(&(page_number, i + 1))
                    .context("Missing bounded list value")?;
                snapshot.value = choice.value.clone();
                snapshot.selected_options = choice.selected.clone();
                snapshot.display_value =
                    choice.selected.first().and_then(|(_, label)| label.clone());
            }
            PdfFormField::Signature(_) => snapshot.kind = "signature",
            PdfFormField::PushButton(_) => snapshot.kind = "push_button",
            PdfFormField::Unknown(_) => {
                needs_review = true;
            }
        }
        if snapshot.kind != "unknown" {
            ensure!(raw.is_some(), "Missing bounded widget value");
        }
        let bounds = snapshot.bounds;
        ensure!(
            [
                bounds.x0,
                bounds.y0,
                bounds.x1,
                bounds.y1,
                bounds.width(),
                bounds.height()
            ]
            .into_iter()
            .all(f64::is_finite)
                && bounds.width() >= 0.0
                && bounds.height() >= 0.0,
            "Invalid PDF widget bounds"
        );
        let bytes = snapshot
            .name
            .as_ref()
            .map_or(0, String::len)
            .saturating_add(snapshot.value.as_ref().map_or(0, String::len));
        *text_budget = text_budget
            .checked_add(bytes as u64)
            .context("PDF form text budget overflow")?;
        crate::scan::ensure_size(*text_budget, limits)?;
        result.push(snapshot);
    }
    Ok((result, needs_review))
}

fn pair_fields(
    old: Vec<FormFieldSnapshot>,
    new: Vec<FormFieldSnapshot>,
    tolerance: f64,
    old_unit: f64,
    new_unit: f64,
) -> Vec<FormFieldChange> {
    use std::collections::{HashMap, VecDeque};
    let exact_key = |f: &FormFieldSnapshot, unit: f64| {
        let bounds = f.bounds.scaled(unit);
        (
            f.name.clone(),
            f.kind,
            [bounds.x0, bounds.y0, bounds.x1, bounds.y1].map(f64::to_bits),
        )
    };
    let name_key = |f: &FormFieldSnapshot| (f.name.clone(), f.kind);
    let mut targets: Vec<_> = new.into_iter().map(Some).collect();
    let mut exact: HashMap<_, VecDeque<usize>> = HashMap::new();
    for (i, f) in targets.iter().enumerate() {
        exact
            .entry(exact_key(f.as_ref().unwrap(), new_unit))
            .or_default()
            .push_back(i);
    }
    let mut pairs = Vec::new();
    let mut remaining = Vec::new();
    for f in old {
        if let Some(i) = exact
            .get_mut(&exact_key(&f, old_unit))
            .and_then(VecDeque::pop_front)
        {
            pairs.push((f, targets[i].take().unwrap()));
        } else {
            remaining.push(f);
        }
    }
    if old_unit != new_unit {
        // SDK rectangles use f32; rebasing can perturb physical coordinates by
        // a few ulps. Match nearby same-name/kind controls before the unique-name
        // fallback, including multiple radio controls belonging to one field.
        let cell = tolerance.max(1.0);
        let location = |f: &FormFieldSnapshot, unit: f64| {
            (
                (f.bounds.x0 * unit / cell).floor() as i64,
                (f.bounds.y0 * unit / cell).floor() as i64,
            )
        };
        let mut nearby: HashMap<_, VecDeque<usize>> = HashMap::new();
        for (i, target) in targets.iter().enumerate() {
            if let Some(target) = target {
                let (x, y) = location(target, new_unit);
                nearby
                    .entry((name_key(target), x, y))
                    .or_default()
                    .push_back(i);
            }
        }
        let budget = remaining
            .len()
            .saturating_add(targets.len())
            .saturating_mul(16);
        let mut work = 0usize;
        let mut unpaired = Vec::new();
        for source in remaining {
            let (x, y) = location(&source, old_unit);
            let mut found = None;
            'search: for dx in -1i64..=1 {
                for dy in -1i64..=1 {
                    if work >= budget {
                        break 'search;
                    }
                    let key = (
                        name_key(&source),
                        x.saturating_add(dx),
                        y.saturating_add(dy),
                    );
                    if let Some(candidates) = nearby.get_mut(&key) {
                        while candidates.front().is_some_and(|&i| targets[i].is_none()) {
                            candidates.pop_front();
                        }
                        for &i in candidates.iter() {
                            if work >= budget {
                                break 'search;
                            }
                            work += 1;
                            if targets[i].as_ref().is_some_and(|target| {
                                field_bounds_equal(&source, target, old_unit, new_unit, tolerance)
                            }) {
                                found = Some(i);
                                break 'search;
                            }
                        }
                    }
                }
            }
            if let Some(i) = found {
                pairs.push((source, targets[i].take().unwrap()));
            } else {
                // Unproven associations remain explicit removals/additions;
                // changed interactive inputs already require review.
                unpaired.push(source);
            }
        }
        remaining = unpaired;
    }
    // Control-group indices and annotation order can change during a harmless
    // PDF rewrite. Use geometry first, then unique named fields for moved controls.
    let mut old_counts = HashMap::new();
    let mut new_by_name: HashMap<_, Vec<usize>> = HashMap::new();
    for f in &remaining {
        *old_counts.entry(name_key(f)).or_insert(0usize) += 1;
    }
    for (i, f) in targets.iter().enumerate() {
        if let Some(f) = f {
            new_by_name.entry(name_key(f)).or_default().push(i);
        }
    }
    let mut changes = Vec::new();
    for f in remaining {
        let key = name_key(&f);
        if f.name.is_some()
            && old_counts.get(&key) == Some(&1)
            && new_by_name.get(&key).is_some_and(|v| v.len() == 1)
        {
            let i = new_by_name[&key][0];
            pairs.push((f, targets[i].take().unwrap()));
        } else {
            changes.push(FormFieldChange {
                status: Status::Removed,
                old: Some(f),
                new: None,
            });
        }
    }
    for (source, target) in pairs {
        let equal = source.flags == target.flags
            && source.value == target.value
            && source.checked == target.checked
            && source.export_value == target.export_value
            && source.selected_options == target.selected_options
            && field_bounds_equal(&source, &target, old_unit, new_unit, tolerance);
        if !equal {
            changes.push(FormFieldChange {
                status: Status::Modified,
                old: Some(source),
                new: Some(target),
            });
        }
    }
    changes.extend(targets.into_iter().flatten().map(|new| FormFieldChange {
        status: Status::Added,
        old: None,
        new: Some(new),
    }));
    changes.sort_by_key(|f| {
        f.new
            .as_ref()
            .or(f.old.as_ref())
            .map(|f| f.annotation_index)
    });
    changes
}

fn field_bounds_equal(
    source: &FormFieldSnapshot,
    target: &FormFieldSnapshot,
    old_unit: f64,
    new_unit: f64,
    tolerance: f64,
) -> bool {
    let (a, b) = (
        source.bounds.scaled(old_unit),
        target.bounds.scaled(new_unit),
    );
    [a.x0 - b.x0, a.y0 - b.y0, a.x1 - b.x1, a.y1 - b.y1]
        .into_iter()
        .all(|v| v.abs() <= tolerance)
}

fn raster(page: &PdfPage<'_>, dpi: u32, user_unit: f64) -> Result<(RgbaImage, PageViewport)> {
    let scale = crate::render::physical_scale(dpi, user_unit)?;
    let (width, height) = crate::render::checked_render_dimensions(page, scale)?;
    let (view_box, rotation) = crate::geometry::read_page_geometry(page)?;
    let mut viewport = PageViewport::new(view_box, rotation, width, height)?;
    viewport.user_unit = Some(user_unit);
    let bitmap = page.render_with_config(&PdfRenderConfig::new().scale_page_by_factor(scale))?;
    let image = bitmap.as_image().into_rgba8();
    ensure!(
        image.dimensions() == (width, height),
        "Unexpected comparison bitmap dimensions"
    );
    Ok((image, viewport))
}

fn compare_rasters(
    old: &PdfPage<'_>,
    new: &PdfPage<'_>,
    options: &VisualDiffOptions,
    used: &mut u64,
    old_unit: Result<f64>,
    new_unit: Result<f64>,
) -> Result<VisualPageDiff> {
    let old_unit = old_unit?;
    let new_unit = new_unit?;
    let (ow, oh) = crate::render::checked_render_dimensions(
        old,
        crate::render::physical_scale(options.dpi, old_unit)?,
    )?;
    let (nw, nh) = crate::render::checked_render_dimensions(
        new,
        crate::render::physical_scale(options.dpi, new_unit)?,
    )?;
    let pixels = u64::from(ow) * u64::from(oh) + u64::from(nw) * u64::from(nh);
    ensure!(
        pixels <= options.max_total_pixels.saturating_sub(*used),
        "visual max_total_pixels exceeded"
    );
    crate::raster_limits::check_page(old, options.max_source_image_pixels)?;
    crate::raster_limits::check_page(new, options.max_source_image_pixels)?;
    *used += pixels;
    let (a, av) = raster(old, options.dpi, old_unit)?;
    let (b, bv) = raster(new, options.dpi, new_unit)?;
    let (changed_pixels, region) = if a.dimensions() != b.dimensions() {
        (None, None)
    } else {
        let mut count = 0u64;
        let mut region = Bounds {
            x0: f64::INFINITY,
            y0: f64::INFINITY,
            x1: 0.0,
            y1: 0.0,
        };
        for (i, (a, b)) in a.pixels().zip(b.pixels()).enumerate() {
            if a.0
                .into_iter()
                .zip(b.0)
                .any(|(a, b)| a.abs_diff(b) > options.channel_tolerance)
            {
                count += 1;
                let x = (i % ow as usize) as f64;
                let y = (i / ow as usize) as f64;
                region.x0 = region.x0.min(x);
                region.y0 = region.y0.min(y);
                region.x1 = region.x1.max(x + 1.0);
                region.y1 = region.y1.max(y + 1.0);
            }
        }
        (Some(count), (count > 0).then_some(region))
    };
    let changed = changed_pixels != Some(0);
    let old_bounds = if let Some(r) = region {
        Some(av.pdf_to_pixel.inverse()?.transform_bounds(r))
    } else {
        changed.then_some(av.view_box)
    };
    let new_bounds = if let Some(r) = region {
        Some(bv.pdf_to_pixel.inverse()?.transform_bounds(r))
    } else {
        changed.then_some(bv.view_box)
    };
    Ok(VisualPageDiff {
        status: if changed {
            VisualStatus::Changed
        } else {
            VisualStatus::Unchanged
        },
        changed_pixels,
        old_bounds,
        new_bounds,
        old_viewport: Some(av),
        new_viewport: Some(bv),
        reason: None,
    })
}

pub(crate) fn supplement(
    diff: &mut DocumentDiff,
    old: &PdfDocument<'_>,
    new: &PdfDocument<'_>,
    limits: &Limits,
    options: &crate::DiffOptions,
    source: &SourceFacts,
) -> Result<()> {
    let mut coverage = ComparisonCoverage {
        identical_source: source.same_source,
        visual_dpi: options.visual.as_ref().map(|o| o.dpi),
        channel_tolerance: options.visual.as_ref().map(|o| o.channel_tolerance),
        max_total_pixels: options.visual.as_ref().map(|o| o.max_total_pixels),
        max_source_image_pixels: options.visual.as_ref().map(|o| o.max_source_image_pixels),
        ..ComparisonCoverage::default()
    };
    let old_form =
        old.form().is_some() || source.old_canonical.as_ref().is_ok_and(|s| s.form_present);
    let new_form =
        new.form().is_some() || source.new_canonical.as_ref().is_ok_and(|s| s.form_present);
    let old_empty = old_form && source.old_canonical.as_ref().is_ok_and(|s| s.empty_shell);
    let new_empty = new_form && source.new_canonical.as_ref().is_ok_and(|s| s.empty_shell);
    coverage.verified_empty_acroform_shells = usize::from(old_empty) + usize::from(new_empty);
    let has_form = (old_form && !old_empty) || (new_form && !new_empty);
    match (&source.old_canonical, &source.new_canonical) {
        (Ok(a), Ok(b)) => {
            let mut omitted_properties = 0usize;
            coverage.canonical_fields_examined = a.fields.len() + b.fields.len();
            for (side, snapshot) in [("old", a), ("new", b)] {
                for reason in &snapshot.review_reasons {
                    coverage
                        .canonical_form_review_reasons
                        .push(format!("{side}: {reason}"));
                }
                for field in &snapshot.fields {
                    for (property, reason) in &field.unavailable_properties {
                        coverage.canonical_properties_unavailable += 1;
                        if coverage.canonical_form_review_reasons.len() < 1000 {
                            let name = field
                                .name
                                .iter()
                                .flat_map(|part| part.chars().chain(std::iter::once('.')))
                                .take(160)
                                .collect::<String>();
                            coverage
                                .canonical_form_review_reasons
                                .push(format!("{side} field {name} {property}: {reason}"));
                        } else {
                            omitted_properties += 1;
                        }
                    }
                }
            }
            if omitted_properties > 0 {
                coverage.canonical_form_review_reasons.push(format!("{omitted_properties} further property diagnostics omitted; {} unavailable properties in total",coverage.canonical_properties_unavailable));
            }
            diff.canonical_form_changes = crate::canonical_forms::pair(
                a.fields.clone(),
                b.fields.clone(),
                a.inventory_complete,
                b.inventory_complete,
            );
        }
        _ => {
            for (side, result) in [
                ("old", &source.old_canonical),
                ("new", &source.new_canonical),
            ] {
                if let Err(error) = result {
                    coverage.canonical_form_review_reasons.push(format!(
                        "{side}: {}",
                        error.chars().take(240).collect::<String>()
                    ));
                }
            }
        }
    }
    let mut old_text = 0u64;
    let mut new_text = 0u64;
    for page in &mut diff.pages {
        if source.same_source {
            page.visual = Some(VisualPageDiff {
                status: VisualStatus::IdenticalSource,
                changed_pixels: None,
                old_bounds: None,
                new_bounds: None,
                old_viewport: None,
                new_viewport: None,
                reason: None,
            });
            continue;
        }
        let a = page
            .old_index
            .map(|i| old.pages().get((i - 1) as u16))
            .transpose()?;
        let b = page
            .new_index
            .map(|i| new.pages().get((i - 1) as u16))
            .transpose()?;
        let (af, ar) = if let Some(a) = &a {
            fields(
                a,
                page.old_index.unwrap(),
                limits,
                &mut old_text,
                &source.old_widgets,
            )?
        } else {
            (Vec::new(), false)
        };
        let (bf, br) = if let Some(b) = &b {
            fields(
                b,
                page.new_index.unwrap(),
                limits,
                &mut new_text,
                &source.new_widgets,
            )?
        } else {
            (Vec::new(), false)
        };
        coverage.widget_fields_examined += af.len() + bf.len();
        if has_form || ar || br {
            coverage.interactive_pages_requiring_review += 1;
        }
        let old_unit = page.old_size.and_then(|p| p.user_unit).unwrap_or(1.0);
        let new_unit = page.new_size.and_then(|p| p.user_unit).unwrap_or(1.0);
        page.form_changes = pair_fields(af, bf, options.position_tolerance, old_unit, new_unit);
        if !page.form_changes.is_empty()
            && (page.status == Status::Unchanged || page.status == Status::Moved)
        {
            page.status = Status::Modified;
        }
        if let (Some(a), Some(b), Some(visual)) = (&a, &b, &options.visual) {
            let result = match compare_rasters(
                a,
                b,
                visual,
                &mut coverage.pixel_budget_used,
                source.old_units.get(page.old_index.unwrap() - 1),
                source.new_units.get(page.new_index.unwrap() - 1),
            ) {
                Ok(result) => {
                    coverage.rendered_page_pairs += 1;
                    result
                }
                Err(error) => {
                    coverage.skipped_page_pairs += 1;
                    VisualPageDiff {
                        status: VisualStatus::Skipped,
                        changed_pixels: None,
                        old_bounds: None,
                        new_bounds: None,
                        old_viewport: None,
                        new_viewport: None,
                        reason: Some(error.to_string().chars().take(240).collect()),
                    }
                }
            };
            if result.status == VisualStatus::Changed
                && (page.status == Status::Unchanged || page.status == Status::Moved)
            {
                page.status = Status::Modified;
            }
            page.visual = Some(result);
        } else if a.is_some() && b.is_some() {
            coverage.skipped_page_pairs += 1;
        }
    }
    diff.comparison = Some(coverage);
    crate::diff::refresh_summary(diff);
    diff.summary.canonical_form_fields_changed = diff.canonical_form_changes.len();
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn equal_files(old: &std::path::Path, new: &std::path::Path) -> Result<bool> {
    use std::io::Read;
    let mut old = std::fs::File::open(old)?;
    let mut new = std::fs::File::open(new)?;
    if old.metadata()?.len() != new.metadata()?.len() {
        return Ok(false);
    }
    let mut a = [0u8; 16384];
    let mut b = [0u8; 16384];
    loop {
        let na = old.read(&mut a)?;
        let nb = new.read(&mut b)?;
        if na != nb || a[..na] != b[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}
