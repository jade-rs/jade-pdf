//! Read-only SDK projection of independently validated source replacements.
//! All SDK handles stay inside the diagnostic page pass. Exported regions own
//! their text and bounding rectangles; no invented individual glyph positions.
use crate::{source_text::structure::Plan, tagged_table, Bounds};
use anyhow::{ensure, Result};
use pdfium_render::prelude::*;
use std::{collections::HashMap, ffi::c_void};

#[derive(Clone)]
pub(crate) struct Region {
    pub text: String,
    pub bounds: Bounds,
    pub content: tagged_table::Content,
    pub anchor: bool,
    pub font_size: Option<f64>,
    pub origin: Option<(f64, f64)>,
    pub angle: f64,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct ObjectInfo {
    pub group: Option<usize>,
    pub marked: bool,
    pub blank: bool,
    pub empty: bool,
}
#[derive(Default)]
pub(crate) struct Projection {
    pub objects: HashMap<usize, ObjectInfo>,
    pub regions: Vec<Region>,
    pub unavailable: bool,
    suppressed: std::collections::HashSet<usize>,
}
impl Projection {
    pub fn suppressed(&self, object: usize) -> bool {
        self.suppressed.contains(&object)
    }
    pub fn content_replaced(&self, object: usize) -> bool {
        self.objects
            .get(&object)
            .is_some_and(|i| i.marked || i.group.is_some())
    }
    pub fn represented(&self, object: usize) -> bool {
        self.objects
            .get(&object)
            .is_some_and(|i| i.group.is_some() || i.blank)
    }
}
#[derive(Default)]
pub(crate) struct Budget {
    used: usize,
    mark_contract: Option<bool>,
}
impl Budget {
    pub(crate) fn charge(&mut self, n: usize) -> Result<()> {
        self.used = self.used.saturating_add(n);
        ensure!(
            self.used <= 2_000_000,
            "ActualText SDK projection exceeds work budget"
        );
        Ok(())
    }
}
#[derive(Clone, Copy, Default)]
struct Marks {
    present: bool,
    blank: bool,
    empty: bool,
}
impl Marks {
    fn combine(self, other: Self) -> Self {
        if !self.present {
            return other;
        }
        if !other.present {
            return self;
        }
        Self {
            present: true,
            blank: self.blank && other.blank,
            empty: self.empty && other.empty,
        }
    }
}
#[derive(Clone)]
struct MarkData {
    mcid: Option<i32>,
    actual: bool,
    text: Option<std::sync::Arc<str>>,
}
#[derive(Clone, Copy, Eq, PartialEq)]
struct MarkRef {
    handle: usize,
    namespace: usize,
}
#[derive(Clone, Copy)]
struct Affine([f64; 6]);
impl Affine {
    const IDENTITY: Self = Self([1., 0., 0., 1., 0., 0.]);
    fn after(self, local: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = local.0;
        Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ])
    }
    fn point(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
    fn bounds(self, rect: Bounds) -> Option<Bounds> {
        let p = [
            self.point(rect.x0, rect.y0),
            self.point(rect.x1, rect.y0),
            self.point(rect.x0, rect.y1),
            self.point(rect.x1, rect.y1),
        ];
        if !p.iter().all(|(x, y)| x.is_finite() && y.is_finite()) {
            return None;
        }
        Some(Bounds {
            x0: p.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
            y0: p.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
            x1: p.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
            y1: p.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
        })
    }
}
fn output_argument<T, R>(_: &impl Fn(*mut T) -> R) -> Box<std::mem::MaybeUninit<T>> {
    Box::new(std::mem::MaybeUninit::zeroed())
}
fn matrix(pdfium: &Pdfium, object: *mut c_void) -> Result<Affine> {
    let read = |out| pdfium.bindings().FPDFPageObj_GetMatrix(object.cast(), out);
    let mut value = output_argument(&read);
    ensure!(
        pdfium.bindings().is_true(read(value.as_mut_ptr())),
        "Failed to read ActualText object transform"
    );
    // SAFETY: the successful public SDK call initialized its FS_MATRIX.
    let value = unsafe { value.assume_init() };
    let values = [value.a, value.b, value.c, value.d, value.e, value.f].map(f64::from);
    ensure!(
        values.into_iter().all(f64::is_finite),
        "Invalid ActualText object transform"
    );
    Ok(Affine(values))
}
struct Candidate {
    text: String,
    invalid: bool,
    members: usize,
    bounds: Option<Bounds>,
    content: Option<tagged_table::Content>,
    font_size: Option<f64>,
    mixed_font_size: bool,
    origin: Option<(f64, f64)>,
    angle: Option<f64>,
    mixed_angle: bool,
    has_extent: bool,
}
impl Candidate {
    fn new(text: String, invalid: bool) -> Self {
        Self {
            text,
            invalid,
            members: 0,
            bounds: None,
            content: None,
            font_size: None,
            mixed_font_size: false,
            origin: None,
            angle: None,
            mixed_angle: false,
            has_extent: false,
        }
    }
}
struct Walk<'a> {
    pdfium: &'a Pdfium,
    plan: &'a Plan,
    budget: &'a mut Budget,
    objects: HashMap<usize, ObjectInfo>,
    groups: Vec<Candidate>,
    marks: HashMap<usize, MarkData>,
    scopes: HashMap<(usize, usize), usize>,
    scope_contexts: HashMap<(usize, usize), Vec<i32>>,
    scope_signatures: HashMap<crate::source_text::structure::MarkedSignature, Vec<usize>>,
    next_namespace: usize,
    marked_enabled: bool,
    unavailable: bool,
    tables: Option<&'a tagged_table::Membership>,
}
impl Walk<'_> {
    fn mark(&mut self, handle: usize) -> Result<MarkData> {
        if let Some(value) = self.marks.get(&handle) {
            return Ok(value.clone());
        }
        self.budget.charge(1)?;
        let b = self.pdfium.bindings();
        let mut mcid = 0;
        let mcid = b
            .is_true(b.FPDFPageObjMark_GetParamIntValue(handle as _, "MCID", &mut mcid))
            .then_some(mcid);
        let actual = b.FPDFPageObjMark_GetParamValueType(handle as _, "ActualText") as u32 != 0;
        let text = if actual {
            marked_bytes(self.pdfium, handle as _, self.budget)?.and_then(|bytes| {
                crate::source_text::unicode::decode_actual_bytes(&bytes)
                    .ok()
                    .map(std::sync::Arc::from)
            })
        } else {
            None
        };
        let value = MarkData { mcid, actual, text };
        self.marks.insert(handle, value.clone());
        Ok(value)
    }
    fn object(
        &mut self,
        object: *mut c_void,
        inherited: &[MarkRef],
        namespace: usize,
        parent_matrix: Affine,
        depth: usize,
    ) -> Result<()> {
        self.budget.charge(1)?;
        ensure!(
            !object.is_null() && depth <= 32,
            "Invalid or over-nested ActualText SDK object"
        );
        let b = self.pdfium.bindings();
        let count = b.FPDFPageObj_CountMarks(object.cast());
        ensure!(
            (0..=64).contains(&count),
            "ActualText content marks exceed budget"
        );
        self.budget.charge(count as usize)?;
        let mut own = Vec::with_capacity(count as usize);
        for i in 0..count {
            let mark = b.FPDFPageObj_GetMark(object.cast(), i as _);
            ensure!(!mark.is_null(), "Missing SDK content mark");
            own.push(mark as usize);
        }
        // Form parsers may expose inherited marks as a prefix. Preserve their
        // owning invocation instead of double-counting them as child scopes.
        let prefix =
            own.len() >= inherited.len() && own.iter().zip(inherited).all(|(a, b)| *a == b.handle);
        let mut context = inherited.to_vec();
        for handle in own
            .into_iter()
            .skip(if prefix { inherited.len() } else { 0 })
        {
            ensure!(
                !context.iter().any(|m| m.handle == handle),
                "Contradictory ActualText mark ancestry"
            );
            context.push(MarkRef { handle, namespace });
        }
        ensure!(
            context.len() <= 64,
            "Inherited ActualText marks exceed budget"
        );
        let mut group: Option<usize> = None;
        let mut marked = Marks::default();
        let mut mcids = Vec::new();
        let mut active_scopes = Vec::new();
        for reference in &context {
            let data = self.mark(reference.handle)?;
            if let Some(id) = data.mcid {
                mcids.push(id);
                if let Some(&candidate) = self.plan.ids.get(&id) {
                    if let Some(previous) = group {
                        if previous != candidate {
                            self.groups[previous].invalid = true;
                            self.groups[candidate].invalid = true;
                        }
                    } else {
                        group = Some(candidate);
                    }
                }
            }
            if data.actual {
                marked = marked.combine(Marks {
                    present: true,
                    empty: data.text.as_ref().is_some_and(|s| s.is_empty()),
                    blank: data
                        .text
                        .as_ref()
                        .is_some_and(|s| s.chars().all(char::is_whitespace)),
                });
                if let Some(text) = data.text.filter(|s| !s.is_empty()) {
                    if self.marked_enabled {
                        let key = (reference.namespace, reference.handle);
                        let id = if let Some(&id) = self.scopes.get(&key) {
                            if self.scope_contexts.get(&key) != Some(&mcids) {
                                self.groups[id].invalid = true;
                                self.unavailable = true;
                            }
                            id
                        } else {
                            let signature = crate::source_text::structure::MarkedSignature {
                                mcids: mcids.clone(),
                                text: text.to_string(),
                            };
                            self.budget.charge(mcids.len().saturating_add(text.len()))?;
                            let id = self.groups.len();
                            self.groups.push(Candidate::new(
                                text.to_string(),
                                !self.plan.marked_scopes.contains_key(&signature),
                            ));
                            self.scopes.insert(key, id);
                            self.scope_contexts.insert(key, mcids.clone());
                            self.scope_signatures.entry(signature).or_default().push(id);
                            id
                        };
                        active_scopes.push(id);
                    }
                }
            }
        }
        if let Some(g) = group {
            if marked.present {
                self.groups[g].invalid = true;
            }
            for &scope in &active_scopes {
                self.groups[scope].invalid = true;
            }
        } else if active_scopes.len() == 1 {
            group = Some(active_scopes[0]);
        } else if !active_scopes.is_empty() {
            self.unavailable = true;
            for &scope in &active_scopes {
                self.groups[scope].invalid = true;
            }
        }
        let kind = b.FPDFPageObj_GetType(object.cast()) as u32;
        if let Some(g) = group {
            let candidate = &mut self.groups[g];
            candidate.members += 1;
            let (mut l, mut bot, mut r, mut top) = (0f32, 0f32, 0f32, 0f32);
            let mut bounded = false;
            if b.is_true(b.FPDFPageObj_GetBounds(object.cast(), &mut l, &mut bot, &mut r, &mut top))
            {
                let values = [l, bot, r, top];
                if values.into_iter().all(f32::is_finite) && l <= r && bot <= top {
                    if let Some(bounds) = parent_matrix.bounds(Bounds {
                        x0: f64::from(l),
                        y0: f64::from(bot),
                        x1: f64::from(r),
                        y1: f64::from(top),
                    }) {
                        bounded = true;
                        let [a, b, c, d, _, _] = parent_matrix.0;
                        candidate.has_extent |=
                            kind != FPDF_PAGEOBJ_FORM && l < r && bot < top && a * d - b * c != 0.0;
                        candidate.bounds = Some(
                            candidate
                                .bounds
                                .map(|old| union(old, bounds))
                                .unwrap_or(bounds),
                        );
                    }
                }
            }
            if !bounded && kind != FPDF_PAGEOBJ_FORM {
                candidate.invalid = true;
            }
            if kind == FPDF_PAGEOBJ_TEXT {
                let transform = parent_matrix.after(matrix(self.pdfium, object)?);
                let mut size = 0f32;
                let mut baseline_sign = 1.0;
                let size = if b.is_true(b.FPDFTextObj_GetFontSize(object.cast(), &mut size)) {
                    if size.is_sign_negative() {
                        baseline_sign = -1.0;
                    }
                    let effective =
                        (f64::from(size).abs() * transform.0[2].hypot(transform.0[3])) as f32;
                    (effective.is_finite() && effective > 0.).then_some(f64::from(effective))
                } else {
                    None
                };
                if let Some(size) = size {
                    if candidate
                        .font_size
                        .is_some_and(|old| (old - size).abs() > 0.001 * old.max(size))
                    {
                        candidate.mixed_font_size = true;
                    } else {
                        candidate.font_size = Some(size);
                    }
                } else {
                    candidate.mixed_font_size = true;
                }
                candidate
                    .origin
                    .get_or_insert((transform.0[4], transform.0[5]));
                let angle = (transform.0[1] * baseline_sign)
                    .atan2(transform.0[0] * baseline_sign)
                    .to_degrees();
                if candidate.angle.is_some_and(|old| (old - angle).abs() > 0.1) {
                    candidate.mixed_angle = true;
                } else {
                    candidate.angle = Some(angle);
                }
            }
            let content = self
                .tables
                .map(|t| t.content(self.pdfium, object))
                .unwrap_or_default();
            candidate.content = Some(match candidate.content {
                None => content,
                Some(old) if old == content => old,
                _ => tagged_table::Content::default(),
            });
        }
        let info = ObjectInfo {
            group,
            marked: marked.present,
            blank: marked.blank,
            empty: marked.empty,
        };
        if let Some(previous) = self.objects.insert(object as usize, info) {
            // A shared SDK child handle cannot identify which invocation owns a
            // text record. Keep original records and make both scopes reviewable.
            self.unavailable = true;
            if let Some(g) = previous.group {
                self.groups[g].invalid = true;
            }
            if let Some(g) = group {
                self.groups[g].invalid = true;
            }
        }
        if kind == FPDF_PAGEOBJ_FORM {
            let count = b.FPDFFormObj_CountObjects(object.cast());
            ensure!(
                (0..=200_000).contains(&count),
                "Invalid ActualText Form size"
            );
            let transform = parent_matrix.after(matrix(self.pdfium, object)?);
            self.next_namespace += 1;
            let child_namespace = self.next_namespace;
            for i in 0..count {
                self.object(
                    b.FPDFFormObj_GetObject(object.cast(), i as _).cast(),
                    &context,
                    child_namespace,
                    transform,
                    depth + 1,
                )?;
            }
        }
        Ok(())
    }
}

// The pinned WASM wrapper calls GetParamKey from its BlobValue method and
// copies out_buflen bytes even when they exceed the supplied capacity. Use an
// explicit adapter operation and one fixed buffer. The companion adapter routes
// only this reserved key to the public BlobValue API and rejects oversize reads.
#[cfg(target_arch = "wasm32")]
pub(crate) fn marked_bytes(
    pdfium: &Pdfium,
    mark: *mut c_void,
    budget: &mut Budget,
) -> Result<Option<Vec<u8>>> {
    let b = pdfium.bindings();
    budget.charge(4)?;
    let mut bytes = vec![0u8; 4096];
    let mut needed = 0;
    if !b.is_true(b.FPDFPageObjMark_GetParamBlobValue(
        mark.cast(),
        "JadeBlob:ActualText",
        bytes.as_mut_ptr(),
        4096,
        &mut needed,
    )) || needed > 4096
    {
        return Ok(None);
    }
    budget.charge(needed as usize)?;
    bytes.truncate(needed as usize);
    Ok(Some(bytes))
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn marked_bytes(
    pdfium: &Pdfium,
    mark: *mut c_void,
    budget: &mut Budget,
) -> Result<Option<Vec<u8>>> {
    let b = pdfium.bindings();
    let mut needed = 0;
    if !b.is_true(b.FPDFPageObjMark_GetParamBlobValue(
        mark.cast(),
        "ActualText",
        std::ptr::null_mut(),
        0,
        &mut needed,
    )) || needed > 4096
    {
        return Ok(None);
    }
    budget.charge(needed as usize)?;
    let mut bytes = vec![0u8; needed as usize];
    let mut actual = 0;
    ensure!(
        b.is_true(b.FPDFPageObjMark_GetParamBlobValue(
            mark.cast(),
            "ActualText",
            bytes.as_mut_ptr(),
            needed,
            &mut actual
        )) && actual == needed,
        "SDK ActualText bytes changed"
    );
    Ok(Some(bytes))
}

fn union(a: Bounds, b: Bounds) -> Bounds {
    Bounds {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}
pub(crate) fn read(
    pdfium: &Pdfium,
    page: *mut c_void,
    plan: Option<&Plan>,
    tables: Option<&tagged_table::Membership>,
    budget: &mut Budget,
) -> Result<Projection> {
    let Some(plan) = plan else {
        return Ok(Projection::default());
    };
    if plan.groups.is_empty() && !plan.marked_actual {
        return Ok(Projection {
            unavailable: plan.unavailable,
            ..Projection::default()
        });
    }
    let b = pdfium.bindings();
    let marked_enabled = if plan.marked_scopes.is_empty() || plan.unavailable {
        false
    } else {
        if budget.mark_contract.is_none() {
            budget.mark_contract = Some(check_mark_contract(pdfium));
        }
        budget.mark_contract == Some(true)
    };
    let mut walk = Walk {
        pdfium,
        plan,
        budget,
        objects: HashMap::new(),
        groups: plan
            .groups
            .iter()
            .map(|g| Candidate::new(g.text.clone(), g.invalid))
            .collect(),
        marks: HashMap::new(),
        scopes: HashMap::new(),
        scope_contexts: HashMap::new(),
        scope_signatures: HashMap::new(),
        next_namespace: 0,
        marked_enabled,
        unavailable: !plan.marked_scopes.is_empty() && !plan.unavailable && !marked_enabled,
        tables,
    };
    let count = b.FPDFPage_CountObjects(page.cast());
    ensure!(
        (0..=200_000).contains(&count),
        "Invalid ActualText page object count"
    );
    for index in 0..count {
        walk.object(
            b.FPDFPage_GetObject(page.cast(), index).cast(),
            &[],
            0,
            Affine::IDENTITY,
            0,
        )?;
    }
    if marked_enabled {
        for (signature, expected) in &plan.marked_scopes {
            let actual = walk.scope_signatures.get(signature);
            if actual.map_or(0, Vec::len) != *expected {
                walk.unavailable = true;
                for &id in actual.into_iter().flatten() {
                    walk.groups[id].invalid = true;
                }
            }
        }
    }
    let mut result = Projection {
        objects: walk.objects,
        unavailable: plan.unavailable || walk.unavailable,
        ..Projection::default()
    };
    let mut regions = HashMap::new();
    for (group, candidate) in walk.groups.into_iter().enumerate() {
        if candidate.invalid || candidate.members == 0 {
            result.unavailable = true;
            continue;
        }
        if !candidate.text.is_empty() {
            let Some(bounds) = candidate.bounds else {
                result.unavailable = true;
                continue;
            };
            regions.insert(group, result.regions.len());
            result.regions.push(Region {
                text: candidate.text,
                bounds,
                content: candidate.content.unwrap_or_default(),
                anchor: !candidate.has_extent || bounds.width() == 0.0 || bounds.height() == 0.0,
                font_size: if candidate.mixed_font_size {
                    None
                } else {
                    candidate.font_size
                },
                origin: candidate.origin,
                angle: if candidate.mixed_angle {
                    0.0
                } else {
                    candidate.angle.unwrap_or(0.)
                },
            });
        } else {
            regions.insert(group, usize::MAX);
        }
    }
    for (&object, info) in &mut result.objects {
        if let Some(group) = info.group {
            info.group = regions.get(&group).copied();
            if info.group.is_some() {
                result.suppressed.insert(object);
            }
        }
        if info.empty {
            result.suppressed.insert(object);
        }
    }
    Ok(result)
}

fn check_mark_contract(pdfium: &Pdfium) -> bool {
    // Pinned PDFium clones retain content-mark items and each parsed BDC adds
    // a distinct item. Compare only public opaque handles while their page is
    // alive; never inspect SDK layouts or align source scopes by SDK ordinal.
    let b = pdfium.bindings();
    let bytes = crate::sdk_probes::marked_scope();
    let doc = b.FPDF_LoadMemDocument64(&bytes, None);
    if doc.is_null() {
        return false;
    }
    let page = b.FPDF_LoadPage(doc, 0);
    let result = if page.is_null() {
        false
    } else {
        (|| {
            if b.FPDFPage_CountObjects(page) != 4 {
                return false;
            }
            let mut handles = Vec::new();
            for i in 0..4 {
                let object = b.FPDFPage_GetObject(page, i);
                if object.is_null() || b.FPDFPageObj_CountMarks(object) < 1 {
                    return false;
                }
                let mark = b.FPDFPageObj_GetMark(object, 0);
                if mark.is_null() {
                    return false;
                }
                handles.push(mark);
            }
            handles[0] == handles[1] && handles[2] == handles[3] && handles[0] != handles[2]
        })()
    };
    if !page.is_null() {
        b.FPDF_ClosePage(page);
    }
    b.FPDF_CloseDocument(doc);
    result
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    fn inspect(bytes: &[u8], plan: &Plan, budget: &mut Budget) -> (bool, usize, usize) {
        let pdfium = crate::render::bind().unwrap();
        crate::input::with_raw_bytes(&pdfium, bytes, &crate::Limits::default(), |doc| {
            let page = pdfium.bindings().FPDF_LoadPage(doc.cast(), 0);
            assert!(!page.is_null());
            let result = read(&pdfium, page.cast(), Some(plan), None, budget);
            let summary = result.map(|p| (p.unavailable, p.regions.len(), p.suppressed.len()));
            pdfium.bindings().FPDF_ClosePage(page);
            summary
        })
        .unwrap()
    }
    #[test]
    fn public_scope_identity_is_live_checked_and_source_multiplicity_is_required() {
        let pdfium = crate::render::bind().unwrap();
        assert!(check_mark_contract(&pdfium));
        drop(pdfium);
        let bytes = crate::appearance_fixture::marked_actual_scopes(1., 0, "nested_mcid", true);
        let units = crate::page_units::from_bytes_for_text(&bytes, 1, 0..1);
        let plan = units.source_actual(0).unwrap();
        assert_eq!(plan.marked_scopes.len(), 1);
        let (signature, count) = plan.marked_scopes.iter().next().unwrap();
        assert_eq!(signature.mcids, vec![0]);
        assert_eq!(signature.text, "B");
        assert_eq!(*count, 2);
        assert_eq!(inspect(&bytes, plan, &mut Budget::default()), (false, 2, 4));
        for expected in [1, 3] {
            let mut altered = plan.clone();
            *altered.marked_scopes.values_mut().next().unwrap() = expected;
            assert_eq!(
                inspect(&bytes, &altered, &mut Budget::default()),
                (true, 0, 0)
            );
        }
        let mut unavailable = Budget {
            mark_contract: Some(false),
            ..Budget::default()
        };
        assert_eq!(inspect(&bytes, plan, &mut unavailable), (true, 0, 0));
    }
    #[test]
    fn ancestor_affine_applies_to_rectangles_and_anchors_without_expanding_them() {
        let transform = Affine([0., 2., -3., 0., 40., 25.]);
        let point = Bounds {
            x0: 10.,
            x1: 10.,
            y0: 5.,
            y1: 5.,
        };
        assert_eq!(
            transform.bounds(point).unwrap(),
            Bounds {
                x0: 25.,
                x1: 25.,
                y0: 45.,
                y1: 45.
            }
        );
        let rect = Bounds {
            x0: 10.,
            x1: 20.,
            y0: 5.,
            y1: 15.,
        };
        assert_eq!(
            transform.bounds(rect).unwrap(),
            Bounds {
                x0: -5.,
                x1: 25.,
                y0: 45.,
                y1: 65.
            }
        );
        let parent = Affine([2., 0., 0., 1., 40., 25.]);
        let local = Affine([1., 0., 0., 1., 10., 5.]);
        assert_eq!(parent.after(local).point(3., 4.), parent.point(13., 9.));
    }
}
