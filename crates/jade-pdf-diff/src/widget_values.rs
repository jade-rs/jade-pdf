//! Bounded widget strings and field flags, read before creating public snapshots.
//! Choice values use saved exports rather than pdfium-render's displayed labels.
use anyhow::{ensure, Context, Result};
use pdfium_render::prelude::Pdfium;
use std::{collections::HashMap, ffi::c_void, mem::MaybeUninit};

use crate::Limits;

#[derive(Default)]
pub(crate) struct WidgetValue {
    pub name: Option<String>,
    pub flags: u32,
    pub checked: Option<bool>,
    pub export_value: Option<String>,
    pub group_index: Option<u32>,
    /// Pdfium returns the first scalar export value for a multiple selection.
    pub value: Option<String>,
    pub selected: Vec<(usize, Option<String>)>,
}
pub(crate) type WidgetValues = HashMap<(usize, usize), WidgetValue>;

pub(crate) fn from_bytes(pdfium: &Pdfium, bytes: &[u8], limits: &Limits) -> Result<WidgetValues> {
    crate::input::with_raw_bytes(pdfium, bytes, limits, |document| {
        // SAFETY: with_raw_bytes owns a live document using these bindings and
        // closes it only after the callback has returned owned value snapshots.
        unsafe { read_document(pdfium, document, limits) }
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn from_file(
    pdfium: &Pdfium,
    path: &std::path::Path,
    limits: &Limits,
) -> Result<WidgetValues> {
    crate::input::with_raw_file(pdfium, path, limits, |document| {
        // SAFETY: same ownership contract as the byte-based wrapper above.
        unsafe { read_document(pdfium, document, limits) }
    })
}

fn zeroed_argument<T, R>(_: &impl Fn(*mut T) -> R) -> Box<MaybeUninit<T>> {
    Box::new(MaybeUninit::zeroed())
}

/// # Safety
/// document must be a live FPDF_DOCUMENT loaded with pdfium's bindings. No raw
/// document/page/annotation handles or borrowed data escape this function.
unsafe fn read_document(
    pdfium: &Pdfium,
    document: *mut c_void,
    limits: &Limits,
) -> Result<WidgetValues> {
    let bindings = pdfium.bindings();
    let initialize = |info| bindings.FPDFDOC_InitFormFillEnvironment(document.cast(), info);
    // Infer the generated C argument type from the public binding signature.
    // No private Rust wrapper layouts are read. A boxed zeroed C struct supplies
    // null callbacks; its address stays stable through ExitFormFillEnvironment.
    let mut info = zeroed_argument(&initialize);
    let ptr = info.as_mut_ptr();
    // SAFETY: this is the generated FPDF_FORMFILLINFO C struct, whose documented
    // version field is initialized before calling the SDK. All callbacks are None.
    unsafe { (*ptr).version = 2 };
    let form = initialize(ptr);
    ensure!(!form.is_null(), "Failed to initialize widget value reader");
    let result: Result<WidgetValues> = (|| {
        let mut values = HashMap::new();
        let mut bytes = 0u64;
        let mut items = 0usize;
        for p in 0..bindings.FPDF_GetPageCount(document.cast()) {
            let page = bindings.FPDF_LoadPage(document.cast(), p);
            ensure!(
                !page.is_null(),
                "Failed to load widget value page {}",
                p + 1
            );
            // Widget state/export APIs require a page view in this environment.
            // Pair this notification with BeforeClosePage even on read errors.
            bindings.FORM_OnAfterLoadPage(page, form);
            let page_result: Result<()> = (|| {
                let count = bindings.FPDFPage_GetAnnotCount(page);
                ensure!(
                    count >= 0 && count as usize <= limits.max_blocks_per_page,
                    "Widget count exceeds max_blocks_per_page"
                );
                for a in 0..count {
                    let annotation = bindings.FPDFPage_GetAnnot(page, a);
                    ensure!(!annotation.is_null(), "Failed to load widget annotation");
                    let value_result: Result<Option<WidgetValue>> = (|| {
                        let kind = bindings.FPDFAnnot_GetFormFieldType(form, annotation);
                        if kind <= 0 {
                            // Non-fields and unrecognized widgets remain covered by the
                            // page's interactive-review diagnostic, never a complete result.
                            return Ok(None);
                        }
                        charge_items(&mut items, 1)?;
                        let flags = bindings.FPDFAnnot_GetFormFieldFlags(form, annotation);
                        ensure!(flags >= 0, "Failed to read widget field flags");
                        let name = read_utf16(
                            |buffer, length| {
                                bindings
                                    .FPDFAnnot_GetFormFieldName(form, annotation, buffer, length)
                            },
                            limits,
                            &mut bytes,
                        )
                        .context("Failed to read widget name")?;
                        let value = read_utf16(
                            |buffer, length| {
                                bindings
                                    .FPDFAnnot_GetFormFieldValue(form, annotation, buffer, length)
                            },
                            limits,
                            &mut bytes,
                        )
                        .context("Failed to read widget value")?;
                        let (checked, export_value, group_index) = if kind == 2 || kind == 3 {
                            let index = bindings.FPDFAnnot_GetFormControlIndex(form, annotation);
                            ensure!(index >= 0, "Failed to read widget control index");
                            let export = read_utf16(
                                |buffer, length| {
                                    bindings.FPDFAnnot_GetFormFieldExportValue(
                                        form, annotation, buffer, length,
                                    )
                                },
                                limits,
                                &mut bytes,
                            )
                            .context("Failed to read widget export value")?;
                            (
                                Some(
                                    bindings
                                        .is_true(bindings.FPDFAnnot_IsChecked(form, annotation)),
                                ),
                                export,
                                Some(index as u32),
                            )
                        } else {
                            (None, None, None)
                        };
                        let mut selected = Vec::new();
                        if kind != 4 && kind != 5 {
                            return Ok(Some(WidgetValue {
                                name,
                                flags: flags as u32,
                                checked,
                                export_value,
                                group_index,
                                value,
                                selected,
                            }));
                        }
                        // /Opt is optional, including editable combos containing a
                        // custom value only. For this already-validated field handle,
                        // -1 means no /Opt dictionary (the SDK's documented sentinel).
                        let options = bindings.FPDFAnnot_GetOptionCount(form, annotation);
                        let options = if options == -1 { 0 } else { options };
                        ensure!(
                            options >= 0 && options as usize <= limits.max_blocks_per_page,
                            "Choice options exceed max_blocks_per_page"
                        );
                        charge_items(&mut items, options as usize)?;
                        for i in 0..options {
                            let label = read_utf16(
                                |buffer, length| {
                                    bindings.FPDFAnnot_GetOptionLabel(
                                        form, annotation, i, buffer, length,
                                    )
                                },
                                limits,
                                &mut bytes,
                            )?;
                            if bindings
                                .is_true(bindings.FPDFAnnot_IsOptionSelected(form, annotation, i))
                            {
                                selected.push((i as usize, label));
                            }
                        }
                        Ok(Some(WidgetValue {
                            name,
                            flags: flags as u32,
                            checked,
                            export_value,
                            group_index,
                            value,
                            selected,
                        }))
                    })();
                    bindings.FPDFPage_CloseAnnot(annotation);
                    if let Some(value) = value_result? {
                        values.insert((p as usize + 1, a as usize + 1), value);
                    }
                }
                Ok(())
            })();
            bindings.FORM_OnBeforeClosePage(page, form);
            bindings.FPDF_ClosePage(page);
            page_result?;
        }
        Ok(values)
    })();
    bindings.FPDFDOC_ExitFormFillEnvironment(form);
    result
}

// Bound aggregate SDK calls and snapshot storage even when every string is empty.
// This includes widgets and all choice options, including unselected options.
fn charge_items(used: &mut usize, count: usize) -> Result<()> {
    *used = used
        .checked_add(count)
        .context("Widget item budget overflow")?;
    ensure!(
        *used <= 100_000,
        "Widget and option count exceeds document limit of 100000"
    );
    Ok(())
}

fn read_utf16(
    mut getter: impl FnMut(*mut u16, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
    limits: &Limits,
    used: &mut u64,
) -> Result<Option<String>> {
    let length = getter(std::ptr::null_mut(), 0);
    // Pdfium returns two bytes for an empty string; zero means an SDK error.
    // Never turn a failed read into a missing/unchanged field value.
    ensure!(length != 0, "Pdfium failed to read widget string");
    ensure!(
        length.is_multiple_of(2) && length <= 4_000_002,
        "Widget string exceeds 2000000 UTF-16 units or has an invalid length"
    );
    #[allow(clippy::unnecessary_cast)] // c_ulong is 32 bits on Windows/WASM.
    let budget = length as u64;
    *used = used
        .checked_add(budget)
        .context("Widget string budget overflow")?;
    crate::scan::ensure_size(*used, limits)?;
    let mut buffer = vec![0u16; length as usize / 2];
    ensure!(
        getter(buffer.as_mut_ptr(), length) == length,
        "Widget string length changed while reading"
    );
    ensure!(
        buffer.last().copied().map(u16::from_le) == Some(0),
        "Widget string has no terminator"
    );
    buffer.pop();
    if cfg!(target_endian = "big") {
        for value in &mut buffer {
            *value = u16::from_le(*value);
        }
    }
    let text = String::from_utf16(&buffer).context("Invalid UTF-16 widget string")?;
    // UTF-8 can be larger than the UTF-16 buffer (for example Japanese text).
    *used = used
        .checked_add((text.len() as u64).saturating_sub(budget))
        .context("Widget string budget overflow")?;
    crate::scan::ensure_size(*used, limits)?;
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_widget_buffers_are_rejected_before_allocation_or_read() {
        for (length, limits) in [
            (4_000_004, Limits::default()),
            (
                2_000_000,
                Limits {
                    max_input_mb: 1,
                    ..Limits::default()
                },
            ),
        ] {
            let mut queries = 0;
            let mut used = 0;
            let result = read_utf16(
                |buffer, _| {
                    assert!(buffer.is_null());
                    queries += 1;
                    length
                },
                &limits,
                &mut used,
            );
            assert!(result.is_err());
            assert_eq!(queries, 1);
        }
    }

    #[test]
    fn failed_empty_odd_changed_and_unterminated_widget_buffers_are_distinguished() {
        for length in [0, 1, 3] {
            let result = read_utf16(
                |buffer, _| {
                    assert!(buffer.is_null());
                    length
                },
                &Limits::default(),
                &mut 0,
            );
            assert!(result.is_err());
        }
        let empty = read_utf16(
            |buffer, _| {
                if !buffer.is_null() {
                    unsafe {
                        buffer.write(0);
                    }
                }
                2
            },
            &Limits::default(),
            &mut 0,
        )
        .unwrap();
        assert_eq!(empty, Some(String::new()));
        let changed = read_utf16(
            |buffer, _| if buffer.is_null() { 4 } else { 2 },
            &Limits::default(),
            &mut 0,
        );
        assert!(changed.unwrap_err().to_string().contains("length changed"));
        let unterminated = read_utf16(
            |buffer, _| {
                if !buffer.is_null() {
                    unsafe {
                        buffer.write(65u16.to_le());
                    }
                }
                2
            },
            &Limits::default(),
            &mut 0,
        );
        assert!(unterminated
            .unwrap_err()
            .to_string()
            .contains("no terminator"));
    }

    #[test]
    fn aggregate_empty_widget_and_option_work_is_bounded() {
        let mut items = 0;
        charge_items(&mut items, 100_000).unwrap();
        assert!(charge_items(&mut items, 1).is_err());
        let mut overflow = usize::MAX;
        assert!(charge_items(&mut overflow, 1).is_err());
    }

    #[test]
    fn malformed_utf16_widget_values_are_not_silently_replaced() {
        let mut used = 0;
        let result = read_utf16(
            |buffer, _| {
                if !buffer.is_null() {
                    // SAFETY: read_utf16 requested exactly a four-byte output buffer.
                    unsafe {
                        buffer.write(0xdc00u16.to_le());
                        buffer.add(1).write(0);
                    }
                }
                4
            },
            &Limits::default(),
            &mut used,
        );
        assert!(result.unwrap_err().to_string().contains("Invalid UTF-16"));
    }
}
