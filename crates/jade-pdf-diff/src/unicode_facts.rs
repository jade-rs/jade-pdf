//! Bounded SDK diagnostics for APIs not exposed by pdfium-render's character wrapper.
//! The input reader supplies a live raw document; no private Rust layouts are used.
use anyhow::{ensure, Context, Result};
use pdfium_render::prelude::Pdfium;
use std::{collections::HashSet, ffi::c_void, mem::MaybeUninit};

use crate::Limits;

pub(crate) struct PageFacts {
    pub characters: usize,
    pub errors: usize,
    pub sdk_text_objects_without_characters: usize,
    pub source_text_characters_missing: usize,
    pub source_text_accounting_unavailable: bool,
    pub source_unicode_scalars_unmatched: usize,
    pub source_unicode_accounting_unavailable: bool,
    pub fingerprint: u64,
    pub projection_fingerprint: u64,
    pub coordinate_scale: f64,
    pub normalized_objects: usize,
    pub work_characters: usize,
    error_bits: Vec<u64>,
    object_starts: Vec<u64>,
    paired_low_bits: Vec<u64>,
    tagged_cells: Vec<(usize, crate::tagged_table::Content)>,
    pub table_declarations: Vec<crate::tagged_table::Declaration>,
    pub reading_order_requires_review: bool,
    pub actual_regions: Vec<crate::actual_text::Region>,
    pub projected_empty: bool,
    pub preserve_object_boundaries: bool,
    projected_bits: Vec<u64>,
    projected_groups: Vec<(usize, Option<usize>)>,
}

impl PageFacts {
    pub fn projected(&self, index: usize) -> bool {
        self.projected_bits
            .get(index / 64)
            .is_some_and(|bits| bits & (1 << (index % 64)) != 0)
    }
    pub fn projected_group(&self, index: usize) -> Option<usize> {
        self.projected_groups
            .partition_point(|&(start, _)| start <= index)
            .checked_sub(1)
            .and_then(|i| self.projected_groups[i].1)
    }

    pub fn tagged_content(&self, index: usize) -> crate::tagged_table::Content {
        self.tagged_cells
            .partition_point(|&(start, _)| start <= index)
            .checked_sub(1)
            .map(|at| self.tagged_cells[at].1)
            .unwrap_or_default()
    }
    pub fn has_error(&self, index: usize) -> bool {
        self.error_bits
            .get(index / 64)
            .is_some_and(|bits| bits & (1 << (index % 64)) != 0)
    }

    pub fn is_paired_low(&self, index: usize) -> bool {
        self.paired_low_bits
            .get(index / 64)
            .is_some_and(|bits| bits & (1 << (index % 64)) != 0)
    }

    pub fn starts_text_object(&self, index: usize) -> bool {
        self.object_starts
            .get(index / 64)
            .is_some_and(|bits| bits & (1 << (index % 64)) != 0)
    }
}

// A consistency check between two parses of the same immutable source. This is
// not a cryptographic source-identity proof; source equality is checked separately.
pub(crate) const FINGERPRINT_START: u64 = 0xcbf29ce484222325;
pub(crate) fn fingerprint_add(fingerprint: u64, scalar: u32) -> u64 {
    (fingerprint ^ u64::from(scalar)).wrapping_mul(0x100000001b3)
}

// Match index-specific suppression against public geometry in the wrapper's
// second parse. Scalar order alone cannot distinguish repeated characters.
pub(crate) fn geometry_add(
    fingerprint: u64,
    index: usize,
    bounds: Option<[f32; 4]>,
    origin: Option<[f32; 2]>,
) -> u64 {
    let mut value = fingerprint_add(fingerprint, index as u32);
    for parts in [
        bounds.as_ref().map(|x| x.as_slice()),
        origin.as_ref().map(|x| x.as_slice()),
    ] {
        value = fingerprint_add(value, u32::from(parts.is_some()));
        if let Some(parts) = parts {
            for &coordinate in parts {
                value = fingerprint_add(
                    value,
                    if coordinate == 0.0 {
                        0
                    } else {
                        coordinate.to_bits()
                    },
                );
            }
        }
    }
    value
}

fn raw_geometry(pdfium: &Pdfium, text: *mut c_void, index: i32, fingerprint: u64) -> u64 {
    let b = pdfium.bindings();
    let (mut left, mut bottom, mut right, mut top) = (0.0, 0.0, 0.0, 0.0);
    let bounds = b
        .is_true(b.FPDFText_GetCharBox(
            text.cast(),
            index,
            &mut left,
            &mut right,
            &mut bottom,
            &mut top,
        ))
        .then(|| {
            let (l, bot, r, t) = (left as f32, bottom as f32, right as f32, top as f32);
            [l.min(r), bot.min(t), l.max(r), bot.max(t)]
        });
    let (mut x, mut y) = (0.0, 0.0);
    let origin = b
        .is_true(b.FPDFText_GetCharOrigin(text.cast(), index, &mut x, &mut y))
        .then_some([x as f32, y as f32]);
    geometry_add(fingerprint, index as usize, bounds, origin)
}

fn output_argument<T, R>(_: &impl Fn(*mut T) -> R) -> Box<MaybeUninit<T>> {
    Box::new(MaybeUninit::zeroed())
}

/// The caller owns the document until this function returns. A selected page is
/// used for text layers; image-only rendering never requests these diagnostics.
pub(crate) fn read(
    pdfium: &Pdfium,
    document: *mut c_void,
    limits: &Limits,
    selected_page: Option<usize>,
    units: &crate::page_units::PageUnits,
) -> Result<Vec<PageFacts>> {
    let b = pdfium.bindings();
    let count = b.FPDF_GetPageCount(document.cast());
    ensure!(
        count > 0 && count as usize <= limits.max_pages,
        "Invalid Unicode diagnostic page count"
    );
    let range = if let Some(page) = selected_page {
        ensure!(
            page > 0 && page <= count as usize,
            "Unicode diagnostic page is not available"
        );
        page - 1..page
    } else {
        0..count as usize
    };
    let mut result = Vec::new();
    let mut source_characters = 0u64;
    let budget = limits.max_input_mb.saturating_mul(1024 * 1024);
    let mut normalized_objects = 0usize;
    let mut tag_budget = crate::tagged_table::Budget::default();
    let mut object_budget = TextObjectBudget::default();
    let mut actual_budget = crate::actual_text::Budget::default();
    for number in range {
        let page = b.FPDF_LoadPage(document.cast(), number as i32);
        ensure!(
            !page.is_null(),
            "Failed to load Unicode diagnostic page {}",
            number + 1
        );
        let original_rotation = b.FPDFPage_GetRotation(page);
        let page_result: Result<PageFacts> = (|| {
            ensure!(
                (0..=3).contains(&original_rotation),
                "Invalid text page rotation"
            );
            let get_box = |output| b.FPDF_GetPageBoundingBox(page, output);
            let mut bounds = output_argument(&get_box);
            ensure!(
                b.is_true(get_box(bounds.as_mut_ptr())),
                "Invalid Unicode diagnostic page geometry"
            );
            // SAFETY: the SDK succeeded and initialized its documented FS_RECTF.
            let rect = unsafe { bounds.assume_init() };
            let original_bounds = crate::Bounds {
                x0: f64::from(rect.left),
                y0: f64::from(rect.bottom),
                x1: f64::from(rect.right),
                y1: f64::from(rect.top),
            };
            crate::geometry::validate_view_box(original_bounds)?;
            let scale = crate::physical_text::scale(units.get(number).ok())?;
            let baseline = if scale != 1.0 || original_rotation != 0 {
                Some(raw_inventory(pdfium, page.cast())?)
            } else {
                None
            };
            if original_rotation != 0 {
                b.FPDFPage_SetRotation(page, 0);
                ensure!(
                    b.FPDFPage_GetRotation(page) == 0,
                    "Failed to neutralize diagnostic text rotation"
                );
            }
            let object_count = crate::physical_text::prepare_raw(
                pdfium,
                page.cast(),
                scale,
                original_bounds,
                crate::physical_text::MAX_NORMALIZED_OBJECTS.saturating_sub(normalized_objects),
            )?;
            normalized_objects += object_count;
            let table_facts = crate::tagged_table::read(pdfium, page.cast(), &mut tag_budget);
            tag_budget.check()?;
            let mut unobserved = object_budget.read(pdfium, page.cast())?;
            let projection = crate::actual_text::read(
                pdfium,
                page.cast(),
                units.source_actual(number),
                table_facts.membership.as_ref(),
                &mut actual_budget,
            )?;
            let projected_characters: usize = projection
                .regions
                .iter()
                .map(|r| r.text.chars().count())
                .sum();
            ensure!(
                projected_characters <= 2_000_000,
                "ActualText output exceeds character budget"
            );
            unobserved.retain(|object| !projection.represented(*object));
            let text = b.FPDFText_LoadPage(page);
            ensure!(!text.is_null(), "Failed to load Unicode diagnostics");
            let text_result: Result<PageFacts> = (|| {
                let characters = b.FPDFText_CountChars(text);
                ensure!(
                    (0..=2_000_000).contains(&characters),
                    "Unicode diagnostics exceed the character budget"
                );
                let work_characters = (characters as usize)
                    .max(baseline.as_ref().map_or(0, |x| x.0))
                    .saturating_add(projected_characters);
                source_characters = source_characters
                    .checked_add(work_characters as u64)
                    .context("Unicode diagnostic budget overflow")?;
                ensure!(
                    source_characters <= budget,
                    "Unicode diagnostics exceed max_input_mb work budget"
                );
                let mut facts = PageFacts {
                    characters: characters as usize,
                    errors: 0,
                    sdk_text_objects_without_characters: 0,
                    source_text_characters_missing: 0,
                    source_text_accounting_unavailable: false,
                    source_unicode_scalars_unmatched: 0,
                    source_unicode_accounting_unavailable: false,
                    fingerprint: FINGERPRINT_START,
                    projection_fingerprint: FINGERPRINT_START,
                    coordinate_scale: scale,
                    normalized_objects: object_count,
                    work_characters,
                    error_bits: Vec::new(),
                    object_starts: Vec::new(),
                    paired_low_bits: Vec::new(),
                    tagged_cells: Vec::new(),
                    table_declarations: table_facts.declarations,
                    reading_order_requires_review: table_facts.requires_review,
                    actual_regions: projection.regions.clone(),
                    projected_empty: false,
                    preserve_object_boundaries: projection
                        .regions
                        .iter()
                        .any(|r| r.text.trim().is_empty()),
                    projected_bits: Vec::new(),
                    projected_groups: Vec::new(),
                };
                let mut observed_source_characters = 0usize;
                let mut observed_unicode = std::collections::BTreeMap::new();
                let mut previous_object = None;
                let mut previous_group = None;
                let mut outside_content = 0usize;
                let mut removed_content = false;
                let mut uncovered_records = 0usize;
                let mut previous_cell = crate::tagged_table::Content::default();
                let mut inventory = Vec::new();
                for index in 0..characters {
                    // Preserve object boundaries without allocating font names
                    // or one handle per character. Only relative boundaries
                    // are retained; raw SDK addresses never escape this pass.
                    let object = b.FPDFText_GetTextObject(text, index);
                    if !object.is_null() {
                        unobserved.remove(&(object as usize));
                        if previous_object != Some(object) {
                            let cell = table_facts
                                .membership
                                .as_ref()
                                .map(|tags| tags.content(pdfium, object.cast()))
                                .unwrap_or_default();
                            if cell != previous_cell {
                                if tag_budget.charge(1) {
                                    facts.tagged_cells.push((index as usize, cell));
                                } else {
                                    tag_budget.check()?;
                                }
                                previous_cell = cell;
                            }
                        }
                        if previous_object.is_some_and(|previous| previous != object) {
                            if facts.object_starts.is_empty() {
                                facts.object_starts.resize(facts.characters.div_ceil(64), 0);
                            }
                            facts.object_starts[index as usize / 64] |= 1 << (index as usize % 64);
                        }
                        previous_object = Some(object);
                    }
                    let projected = projection.suppressed(object as usize);
                    if projected {
                        facts.projection_fingerprint =
                            raw_geometry(pdfium, text.cast(), index, facts.projection_fingerprint);
                        if facts.projected_bits.is_empty() {
                            facts
                                .projected_bits
                                .resize(facts.characters.div_ceil(64), 0);
                        }
                        facts.projected_bits[index as usize / 64] |= 1 << (index as usize % 64);
                    }
                    let group = projection
                        .objects
                        .get(&(object as usize))
                        .and_then(|info| info.group)
                        .filter(|&g| g < facts.actual_regions.len());
                    if group != previous_group {
                        actual_budget.charge(1)?;
                        facts.projected_groups.push((index as usize, group));
                        previous_group = group;
                    }
                    let generated = b.FPDFText_IsGenerated(text, index);
                    match generated {
                        0 if !projection.content_replaced(object as usize) => {
                            observed_source_characters += 1
                        }
                        0 => {}
                        1 => {}
                        _ => anyhow::bail!("Failed to read SDK generated-character status"),
                    }
                    let scalar = b.FPDFText_GetUnicode(text, index);
                    let paired_high =
                        can_pair_surrogates(pdfium, text.cast(), index, characters, scalar);
                    if paired_high {
                        if facts.paired_low_bits.is_empty() {
                            facts
                                .paired_low_bits
                                .resize(facts.characters.div_ceil(64), 0);
                        }
                        facts.paired_low_bits[(index as usize + 1) / 64] |=
                            1 << ((index as usize + 1) % 64);
                    }
                    if generated == 0 && !facts.is_paired_low(index as usize) && !projected {
                        let decoded = if paired_high {
                            let next = b.FPDFText_GetUnicode(text, index + 1);
                            char::from_u32(0x10000 + ((scalar - 0xd800) << 10) + (next - 0xdc00))
                        } else if b.FPDFText_IsHyphen(text, index) == 1 {
                            Some('-')
                        } else {
                            char::from_u32(scalar)
                        };
                        if let Some(value) = decoded {
                            crate::source_text::unicode::add_scalar(&mut observed_unicode, value);
                        } else {
                            facts.source_unicode_accounting_unavailable = true;
                        }
                    }
                    if !projected {
                        uncovered_records += 1;
                    }
                    if generated == 0
                        && char::from_u32(scalar)
                            .is_some_and(|c| !c.is_whitespace() && !c.is_control())
                    {
                        if projected {
                            removed_content = true;
                        } else {
                            outside_content += 1;
                        }
                    }
                    facts.fingerprint = fingerprint_add(facts.fingerprint, scalar);
                    let error = b.FPDFText_HasUnicodeMapError(text, index);
                    if baseline.is_some() {
                        if let Some(key) =
                            inventory_key(pdfium, text.cast(), index, characters, scalar, error)?
                        {
                            inventory.push(key);
                        }
                    }
                    match error {
                        0 => {}
                        1 => {
                            if facts.error_bits.is_empty() {
                                // <= one bit/source character, <=64MiB at default
                                // document budget, plus at most one word per page.
                                facts.error_bits.resize(facts.characters.div_ceil(64), 0);
                            }
                            facts.error_bits[index as usize / 64] |= 1 << (index as usize % 64);
                            facts.errors += 1;
                        }
                        _ => anyhow::bail!("Failed to read SDK Unicode mapping status"),
                    }
                }
                if let Some((_, before)) = baseline {
                    crate::physical_text::ensure_preserved(before, inventory)?;
                }
                ensure!(
                    uncovered_records.saturating_add(projected_characters) <= 2_000_000,
                    "ActualText and SDK output exceed character budget"
                );
                facts.source_unicode_accounting_unavailable |= projection.unavailable;
                facts.source_text_accounting_unavailable |= projection.unavailable;
                for region in &facts.actual_regions {
                    for c in region.text.chars() {
                        crate::source_text::unicode::add_scalar(&mut observed_unicode, c);
                    }
                }
                // Region geometry is reliable ownership, not a proof of how its
                // logical text interleaves with unrelated SDK text on the page.
                let semantic_regions = facts
                    .actual_regions
                    .iter()
                    .filter(|r| !r.text.trim().is_empty());
                facts.projected_empty = (removed_content || !facts.actual_regions.is_empty())
                    && outside_content == 0
                    && semantic_regions.clone().next().is_none();
                facts.reading_order_requires_review |= semantic_regions.clone().any(|r| r.anchor)
                    || semantic_regions.count() > 1
                    || outside_content > 0 && (removed_content || !facts.actual_regions.is_empty());
                facts.sdk_text_objects_without_characters = unobserved.len();
                match units.source_text_minimum(number) {
                    Some(minimum) => {
                        facts.source_text_characters_missing =
                            minimum.saturating_sub(observed_source_characters)
                    }
                    None => facts.source_text_accounting_unavailable = true,
                }
                if let Some(source) = units.source_unicode(number) {
                    facts.source_unicode_accounting_unavailable |= source.unavailable;
                    let missing: usize = source
                        .scalars
                        .iter()
                        .map(|(c, count)| {
                            count.saturating_sub(*observed_unicode.get(c).unwrap_or(&0))
                        })
                        .sum();
                    let excess: usize = if source.unavailable {
                        0
                    } else {
                        observed_unicode
                            .iter()
                            .map(|(c, count)| {
                                count.saturating_sub(*source.scalars.get(c).unwrap_or(&0))
                            })
                            .sum()
                    };
                    facts.source_unicode_scalars_unmatched = missing + excess;
                } else {
                    facts.source_unicode_accounting_unavailable = true;
                }
                Ok(facts)
            })();
            b.FPDFText_ClosePage(text);
            text_result
        })();
        // FPDFPage_SetRotation mutates the live page dictionary. Restore it
        // even if enumeration/normalization/extraction failed; never save it.
        let restored = if original_rotation != 0 {
            b.FPDFPage_SetRotation(page, original_rotation);
            b.FPDFPage_GetRotation(page) == original_rotation
        } else {
            true
        };
        b.FPDF_ClosePage(page);
        ensure!(restored, "Failed to restore diagnostic text rotation");
        result.push(page_result?);
    }
    Ok(result)
}

// A text page can silently omit entire text objects, including a glyph whose
// ToUnicode mapping is meaningful but whose font geometry resembles a space.
// GetTextByObject uses the same character list and cannot establish absence.
// Keep this separate from per-character Unicode mapping errors and reading order.
#[derive(Default)]
struct TextObjectBudget {
    visited: usize,
}

impl TextObjectBudget {
    const MAX_OBJECTS: usize = 200_000;
    const MAX_DEPTH: usize = 32;

    fn charge(&mut self, count: i32, depth: usize) -> Result<usize> {
        ensure!(count >= 0, "Failed to enumerate SDK text objects");
        ensure!(
            depth <= Self::MAX_DEPTH,
            "SDK text object nesting exceeds work budget"
        );
        self.visited = self
            .visited
            .checked_add(count as usize)
            .context("SDK text object work budget overflow")?;
        ensure!(
            self.visited <= Self::MAX_OBJECTS,
            "SDK text objects exceed document work budget"
        );
        Ok(count as usize)
    }

    fn read(&mut self, pdfium: &Pdfium, page: *mut c_void) -> Result<HashSet<usize>> {
        let b = pdfium.bindings();
        let count = self.charge(b.FPDFPage_CountObjects(page.cast()), 0)?;
        let mut objects = HashSet::new();
        for index in 0..count {
            let object = b.FPDFPage_GetObject(page.cast(), index as i32);
            self.visit(pdfium, object.cast(), 0, &mut objects)?;
        }
        Ok(objects)
    }

    fn visit(
        &mut self,
        pdfium: &Pdfium,
        object: *mut c_void,
        depth: usize,
        objects: &mut HashSet<usize>,
    ) -> Result<()> {
        use pdfium_render::prelude::{FPDF_PAGEOBJ_FORM, FPDF_PAGEOBJ_TEXT};
        ensure!(!object.is_null(), "Failed to enumerate SDK page object");
        let b = pdfium.bindings();
        match b.FPDFPageObj_GetType(object.cast()) as u32 {
            FPDF_PAGEOBJ_TEXT => {
                objects.insert(object as usize);
            }
            FPDF_PAGEOBJ_FORM => {
                let count = self.charge(b.FPDFFormObj_CountObjects(object.cast()), depth + 1)?;
                for index in 0..count {
                    let child = b.FPDFFormObj_GetObject(object.cast(), index as _);
                    self.visit(pdfium, child.cast(), depth + 1, objects)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Decode only a complete UTF-16 surrogate pair. The SDK can expand a single
/// ToUnicode glyph mapping into separate UTF-16 code-unit character records.
pub(crate) fn surrogate_scalar(high: u32, low: u32) -> Option<char> {
    if !(0xd800..=0xdbff).contains(&high) || !(0xdc00..=0xdfff).contains(&low) {
        return None;
    }
    char::from_u32(0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00))
}

fn can_pair_surrogates(
    pdfium: &Pdfium,
    text: *mut c_void,
    index: i32,
    count: i32,
    raw: u32,
) -> bool {
    if !(0xd800..=0xdbff).contains(&raw) || index + 1 >= count {
        return false;
    }
    let b = pdfium.bindings();
    if surrogate_scalar(raw, b.FPDFText_GetUnicode(text.cast(), index + 1)).is_none() {
        return false;
    }
    let object = b.FPDFText_GetTextObject(text.cast(), index);
    if object.is_null()
        || object != b.FPDFText_GetTextObject(text.cast(), index + 1)
        || b.FPDFText_IsGenerated(text.cast(), index) != 0
        || b.FPDFText_IsGenerated(text.cast(), index + 1) != 0
    {
        return false;
    }
    let geometry = |at| {
        let (mut l, mut r, mut bottom, mut top) = (0.0, 0.0, 0.0, 0.0);
        let (mut x, mut y) = (0.0, 0.0);
        if !b.is_true(b.FPDFText_GetCharBox(text.cast(), at, &mut l, &mut r, &mut bottom, &mut top))
            || !b.is_true(b.FPDFText_GetCharOrigin(text.cast(), at, &mut x, &mut y))
        {
            return None;
        }
        let values = [
            l as f32,
            r as f32,
            bottom as f32,
            top as f32,
            x as f32,
            y as f32,
        ];
        (values.into_iter().all(f32::is_finite)).then_some(values)
    };
    match (geometry(index), geometry(index + 1)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn inventory_key(
    pdfium: &Pdfium,
    text: *mut c_void,
    index: i32,
    count: i32,
    raw: u32,
    mut error: i32,
) -> Result<Option<u64>> {
    ensure!(
        error == 0 || error == 1,
        "Failed to read SDK Unicode mapping status"
    );
    let b = pdfium.bindings();
    if (0xdc00..=0xdfff).contains(&raw)
        && index > 0
        && can_pair_surrogates(
            pdfium,
            text,
            index - 1,
            count,
            b.FPDFText_GetUnicode(text.cast(), index - 1),
        )
    {
        return Ok(None);
    }
    let c = if can_pair_surrogates(pdfium, text, index, count, raw) {
        let next_error = b.FPDFText_HasUnicodeMapError(text.cast(), index + 1);
        ensure!(
            (0..=1).contains(&next_error),
            "Failed to read paired SDK Unicode mapping status"
        );
        error += next_error;
        surrogate_scalar(raw, b.FPDFText_GetUnicode(text.cast(), index + 1))
    } else {
        char::from_u32(raw)
    };
    let c = if error > 0 && c.is_none_or(|c| c.is_control() || c.is_whitespace() || c == '\u{ad}') {
        '\u{fffd}'
    } else if raw == 2 && pdfium.bindings().FPDFText_IsHyphen(text.cast(), index) == 1 {
        '-'
    } else if raw == 0 && has_unmapped_ink(pdfium, text, index) {
        '\u{fffd}'
    } else {
        c.unwrap_or('\u{fffd}')
    };
    Ok((!c.is_control() && !c.is_whitespace() && c != '\u{ad}')
        .then_some(u64::from(c as u32) | ((error as u64) << 32)))
}

// Match the extractor's visible, nongenerated U+0000 fallback as well as the
// explicit SDK mapping-error cases. Query loose bounds only if tight bounds
// are unavailable, just as PdfPageTextChar does.
fn has_unmapped_ink(pdfium: &Pdfium, text: *mut c_void, index: i32) -> bool {
    let b = pdfium.bindings();
    if b.FPDFText_IsGenerated(text.cast(), index) == 1 {
        return false;
    }
    let (mut left, mut right, mut bottom, mut top) = (0.0, 0.0, 0.0, 0.0);
    if b.is_true(b.FPDFText_GetCharBox(
        text.cast(),
        index,
        &mut left,
        &mut right,
        &mut bottom,
        &mut top,
    )) {
        return (left as f32) != (right as f32) && (bottom as f32) != (top as f32);
    }
    let get_box = |output| b.FPDFText_GetLooseCharBox(text.cast(), index, output);
    let mut bounds = output_argument(&get_box);
    if b.is_true(get_box(bounds.as_mut_ptr())) {
        // SAFETY: the successful SDK call initialized its documented FS_RECTF.
        let rect = unsafe { bounds.assume_init() };
        return rect.left != rect.right && rect.bottom != rect.top;
    }
    false
}

fn raw_inventory(pdfium: &Pdfium, page: *mut c_void) -> Result<(usize, Vec<u64>)> {
    let b = pdfium.bindings();
    let text = b.FPDFText_LoadPage(page.cast());
    ensure!(
        !text.is_null(),
        "Failed to load original SDK text inventory"
    );
    let result = (|| {
        let count = b.FPDFText_CountChars(text);
        ensure!(
            (0..=2_000_000).contains(&count),
            "Original SDK text exceeds the character budget"
        );
        let mut keys = Vec::new();
        for index in 0..count {
            if let Some(key) = inventory_key(
                pdfium,
                text.cast(),
                index,
                count,
                b.FPDFText_GetUnicode(text, index),
                b.FPDFText_HasUnicodeMapError(text, index),
            )? {
                keys.push(key);
            }
        }
        Ok((count as usize, keys))
    })();
    b.FPDFText_ClosePage(text);
    result
}

#[cfg(test)]
mod surrogate_tests {
    use super::surrogate_scalar;
    #[test]
    fn supplementary_boundaries_agree_with_strict_utf16_and_reject_lone_units() {
        for value in [0x10000, 0x1d400, 0x1d452, 0x1f600, 0x20000, 0x10ffff] {
            let c = char::from_u32(value).unwrap();
            let mut units = [0u16; 2];
            let encoded = c.encode_utf16(&mut units);
            assert_eq!(encoded.len(), 2);
            assert_eq!(
                surrogate_scalar(u32::from(encoded[0]), u32::from(encoded[1])),
                Some(c)
            );
            assert_eq!(
                char::decode_utf16(encoded.iter().copied())
                    .next()
                    .unwrap()
                    .unwrap(),
                c
            );
        }
        for (a, b) in [
            (0xd7ff, 0xdc00),
            (0xdc00, 0xd800),
            (0xd800, 0xdbff),
            (0xd800, 0xe000),
            (0xd800, 0x110000),
        ] {
            assert!(surrogate_scalar(a, b).is_none());
        }
    }
}

#[cfg(test)]
mod text_object_budget_tests {
    use super::TextObjectBudget;
    #[test]
    fn object_work_is_document_wide_and_checked_before_enumeration() {
        let mut budget = TextObjectBudget::default();
        assert_eq!(budget.charge(100_000, 0).unwrap(), 100_000);
        assert_eq!(budget.charge(100_000, 32).unwrap(), 100_000);
        assert!(budget.charge(1, 0).is_err());
        assert!(TextObjectBudget::default().charge(-1, 0).is_err());
        assert!(TextObjectBudget::default().charge(0, 33).is_err());
        let mut overflow = TextObjectBudget {
            visited: usize::MAX,
        };
        assert!(overflow.charge(1, 0).is_err());
    }
}
