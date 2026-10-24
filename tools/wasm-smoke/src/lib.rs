//! Browser runtime checks for the public byte-based APIs.
use wasm_bindgen::prelude::*;

#[path = "../../../crates/jade-pdf-diff/tests/support/appearance_fixture.rs"]
mod appearance_fixture;

#[wasm_bindgen]
pub fn appearance_cases() -> String {
    serde_json::to_string(appearance_fixture::CASES).unwrap()
}

#[wasm_bindgen]
pub fn appearance_case(name: &str) -> Result<String, JsValue> {
    if !appearance_fixture::CASES.contains(&name) {
        return Err(JsValue::from_str("Unknown appearance case"));
    }
    let old = appearance_fixture::fixture(name, false);
    let new = appearance_fixture::fixture(name, true);
    if let Some(mode) = name.strip_prefix("agl_") {
        let ir = pdf_diff::load_document_from_bytes(&new, &pdf_diff::Limits::default())
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let page = &ir.pages[0];
        let known = !matches!(mode, "alias" | "surrogate" | "lowercase" | "mixed_unknown");
        if page.source_unicode_accounting_unavailable == known {
            return Err(JsValue::from_str(
                "Original Adobe glyph name certainty mismatch",
            ));
        }
        if matches!(mode, "uacute" | "ugrave") {
            let expected = if mode == "uacute" { "ú" } else { "ù" };
            if page.text() != expected
                || page.source_unicode_scalars_unmatched != 0
                || page.sdk_unicode_mapping_errors != 0
            {
                return Err(JsValue::from_str("Supported Adobe glyph spelling mismatch"));
            }
        } else {
            // The pinned WASM Helvetica fallback lacks these glyphs. Native
            // host fallback can supply them; this is a guarded SDK omission,
            // not evidence that an unknown source name has a Unicode mapping.
            let mismatch = if mode == "multicomponent" {
                5
            } else {
                usize::from(known)
            };
            let layer = pdf_diff::text_layer_from_bytes(&new, 1);
            if !page.text().is_empty()
                || page.sdk_unicode_mapping_errors != 0
                || page.sdk_text_objects_without_characters != 1
                || page.source_text_characters_missing != 1
                || page.source_unicode_scalars_unmatched != mismatch
                || !layer.is_err_and(|e| e.to_string().contains("no character records"))
            {
                return Err(JsValue::from_str(
                    "Unavailable WASM font glyph was accepted as complete text",
                ));
            }
        }
    }
    if name == "actual_space_separator" {
        for (source, expected) in [(&old, "A B"), (&new, "A D")] {
            let ir = pdf_diff::load_document_from_bytes(source, &pdf_diff::Limits::default())
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            let layer = pdf_diff::text_layer_from_bytes(source, 1)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            if ir.pages[0].text() != expected
                || layer.len() != 1
                || layer[0].text != expected
                || layer[0].source_regions.len() != 1
                || (
                    layer[0].source_regions[0].start,
                    layer[0].source_regions[0].end,
                ) != (1, 2)
            {
                return Err(JsValue::from_str("Declared source separator was lost"));
            }
        }
    }
    if let Some(mode) = name.strip_prefix("marked_scope_") {
        let invalid = matches!(mode, "no_object" | "nested_actual");
        for (source, scalar) in [(&old, 'A'), (&new, 'B')] {
            let ir = pdf_diff::load_document_from_bytes(source, &pdf_diff::Limits::default())
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            let layer = pdf_diff::text_layer_from_bytes(source, 1);
            if invalid {
                if !ir.pages[0].source_unicode_accounting_unavailable
                    || !ir.pages[0].source_text_accounting_unavailable
                {
                    return Err(JsValue::from_str(
                        "Unverifiable marked source scope was accepted",
                    ));
                }
                if mode == "nested_actual"
                    && (ir.pages[0].text() != format!("{scalar} X\n{scalar} X")
                        || layer
                            .map_err(|e| JsValue::from_str(&e.to_string()))?
                            .iter()
                            .any(|r| r.source_text_region))
                {
                    return Err(JsValue::from_str(
                        "Conflicting marked scope did not retain SDK fallback",
                    ));
                }
            } else {
                let count = if mode == "reused_form" { 4 } else { 2 };
                if ir.pages[0].text().replace(char::is_whitespace, "")
                    != scalar.to_string().repeat(count)
                    || ir.pages[0].source_unicode_scalars_unmatched != 0
                    || ir.pages[0].source_unicode_accounting_unavailable
                {
                    return Err(JsValue::from_str(
                        "Marked source replacement count mismatch",
                    ));
                }
                let layer = layer.map_err(|e| JsValue::from_str(&e.to_string()))?;
                let regions: Vec<_> = layer.iter().flat_map(|r| &r.source_regions).collect();
                if regions.len() != count
                    || layer.iter().any(|r| {
                        !r.source_text_region
                            || r.source_regions
                                .iter()
                                .any(|s| s.start >= s.end || s.end > r.text.chars().count())
                    })
                    || regions.iter().any(|r| r.anchor)
                        != (mode.starts_with("zero") || mode == "mixed_anchor")
                {
                    return Err(JsValue::from_str(
                        "Source range geometry or scalar indices mismatch",
                    ));
                }
            }
        }
    }
    if let Some(mode) = name.strip_prefix("actual_structure_") {
        let valid = matches!(
            mode,
            "page"
                | "form"
                | "parent_form"
                | "vector"
                | "multiple"
                | "descendant"
                | "unicode"
                | "empty"
        );
        for (source, expected) in [
            (&old, "A"),
            (
                &new,
                if mode == "unicode" {
                    "日本語🦊"
                } else if mode == "empty" {
                    ""
                } else {
                    "B"
                },
            ),
        ] {
            let ir = pdf_diff::load_document_from_bytes(source, &pdf_diff::Limits::default())
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            let layer = pdf_diff::text_layer_from_bytes(source, 1)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            if valid {
                if ir.pages[0].text() != expected
                    || layer
                        .iter()
                        .any(|r| !r.source_text_region || r.chars.len() != r.text.chars().count())
                {
                    return Err(JsValue::from_str("Source ActualText projection mismatch"));
                }
            } else if !ir.pages[0].source_unicode_accounting_unavailable
                || layer.iter().any(|r| r.source_text_region)
            {
                return Err(JsValue::from_str("Ambiguous source ownership was accepted"));
            }
        }
    }
    if let Some(mode) = name.strip_prefix("oblique_") {
        for (source, expected) in [
            (&old, "ALPHA 12 BETA"),
            (
                &new,
                if mode == "edit" {
                    "ALPHA 98 BETA"
                } else {
                    "ALPHA 12 BETA"
                },
            ),
        ] {
            let doc = pdf_diff::load_document_from_bytes(source, &pdf_diff::Limits::default())
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            let layer = pdf_diff::text_layer_from_bytes(source, 1)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            if !doc.pages[0].blocks.iter().any(|b| b.text == expected)
                || !layer.iter().any(|r| r.text == expected)
                || (mode == "parallel" && !layer.iter().any(|r| r.text == "OTHER 77"))
                || layer
                    .iter()
                    .any(|r| r.text.chars().count() != r.chars.len())
            {
                return Err(JsValue::from_str("Oblique source word-group gold mismatch"));
            }
        }
    }
    if name.starts_with("angled_label_") {
        let reference =
            appearance_fixture::angled_label_layout(1.0, 0, name.contains("nested"), false);
        let baseline = pdf_diff::load_document_from_bytes(&reference, &pdf_diff::Limits::default())
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let layer = pdf_diff::text_layer_from_bytes(&reference, 1)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        for rotation in [0, 90, 180, 270] {
            let source = appearance_fixture::angled_label_layout(
                1.0,
                rotation,
                name.contains("nested"),
                false,
            );
            let current = pdf_diff::load_document_from_bytes(&source, &pdf_diff::Limits::default())
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            let mut page = serde_json::to_value(&current.pages[0]).unwrap();
            page["rotation"] = serde_json::json!(0);
            if page != serde_json::to_value(&baseline.pages[0]).unwrap() {
                return Err(JsValue::from_str(
                    "Display rotation altered oblique text payload",
                ));
            }
            let runs = pdf_diff::text_layer_from_bytes(&source, 1)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            if serde_json::to_value(&runs).unwrap() != serde_json::to_value(&layer).unwrap() {
                return Err(JsValue::from_str(
                    "Display rotation altered character spans",
                ));
            }
        }
    }
    if name.starts_with("unobserved_") {
        for source in [&old, &new] {
            let doc = pdf_diff::load_document_from_bytes(source, &pdf_diff::Limits::default())
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            let mixed = name.ends_with("mixed");
            if doc.pages[0].sdk_text_objects_without_characters != 1
                || doc.pages[0].text() != if mixed { "C" } else { "" }
            {
                return Err(JsValue::from_str(
                    "Unobserved text object diagnostic missing",
                ));
            }
            let result = pdf_diff::text_layer_from_bytes(source, 1);
            if mixed {
                let layer = result.map_err(|e| JsValue::from_str(&e.to_string()))?;
                if layer.len() != 1
                    || layer[0].sdk_text_objects_without_characters != 1
                    || layer[0].text != "C"
                {
                    return Err(JsValue::from_str("Layer lost unobserved text diagnostic"));
                }
            } else if !result
                .err()
                .is_some_and(|e| e.to_string().contains("no character records"))
            {
                return Err(JsValue::from_str("Empty uncertain layer was accepted"));
            }
        }
    }
    if name.starts_with("tagged_table_") || name == "crop_table_identity" {
        for (source, expected_price) in [
            (&old, "10"),
            (
                &new,
                if matches!(
                    name,
                    "tagged_table_edit"
                        | "tagged_table_spans_edit"
                        | "tagged_table_stale_edit"
                        | "tagged_table_shear_edit"
                ) {
                    "11"
                } else {
                    "10"
                },
            ),
        ] {
            let doc = pdf_diff::load_document_from_bytes(source, &pdf_diff::Limits::default())
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            let expected = if name.starts_with("tagged_table_spans_")
                || name.starts_with("tagged_table_stale_")
            {
                format!("Heading across the table\nName\ncategory\nTotals\nQty\nPrice\nApple\n5\n{expected_price}\nPear\n2\n6\nFooter across the table")
            } else {
                format!("A heading spanning both table columns\nItem\nPrice\nApples\n{expected_price}\nA footer spanning both table columns")
            };
            let layer = pdf_diff::text_layer_from_bytes(source, 1)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            let needs_review =
                name.starts_with("tagged_table_stale_") || name == "tagged_table_diagonal";
            if doc.pages[0].reading_order_requires_review != needs_review
                || layer
                    .iter()
                    .any(|r| r.reading_order_requires_review != needs_review)
            {
                return Err(JsValue::from_str(
                    "Tagged reading-order diagnostic mismatch",
                ));
            }
            if name == "tagged_table_stale_ambiguous" || name == "tagged_table_diagonal" {
                let unit = if std::ptr::eq(source, &new) {
                    64.0
                } else {
                    1.0
                };
                let control = if name == "tagged_table_diagonal" {
                    appearance_fixture::tagged_table_layout(
                        unit,
                        0,
                        "baseline-diagonal",
                        false,
                        false,
                    )
                } else {
                    appearance_fixture::spanning_tagged_table_layout(
                        unit,
                        0,
                        "stale-two-orphans",
                        false,
                        false,
                        false,
                    )
                };
                let untagged =
                    pdf_diff::load_document_from_bytes(&control, &pdf_diff::Limits::default())
                        .map_err(|e| JsValue::from_str(&e.to_string()))?;
                let untagged_layer = pdf_diff::text_layer_from_bytes(&control, 1)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                let mut actual = serde_json::to_value(&doc).unwrap();
                actual["pages"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("reading_order_requires_review");
                let mut actual_layer = serde_json::to_value(&layer).unwrap();
                for run in actual_layer.as_array_mut().unwrap() {
                    run.as_object_mut()
                        .unwrap()
                        .remove("reading_order_requires_review");
                }
                if actual != serde_json::to_value(untagged).unwrap()
                    || actual_layer != serde_json::to_value(untagged_layer).unwrap()
                {
                    return Err(JsValue::from_str(
                        "Ambiguous orphan altered geometric payload",
                    ));
                }
            } else if doc.pages[0].text() != expected
                || layer
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
                    != expected
            {
                return Err(JsValue::from_str(
                    "Independent tagged-table cell-order gold mismatch",
                ));
            }
        }
    }
    if name.starts_with("construction_") {
        for (source, unit) in [(&old, 1.0), (&new, 64.0)] {
            let doc = pdf_diff::load_document_from_bytes(source, &pdf_diff::Limits::default())
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            for number in [1, 2, 1] {
                let layer = pdf_diff::text_layer_from_bytes(source, number)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                let compact: String = layer
                    .iter()
                    .flat_map(|r| r.text.chars())
                    .filter(|c| !c.is_whitespace())
                    .collect();
                if compact != "||"
                    || doc.pages[number - 1]
                        .text()
                        .split_whitespace()
                        .collect::<String>()
                        != "||"
                {
                    return Err(JsValue::from_str(
                        "Independent construction-piece gold mismatch",
                    ));
                }
                let scale = if number == 1 { unit } else { 1.0 };
                let reference = pdf_diff::text_layer_from_bytes(&old, number)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                if reference.len() != layer.len()
                    || reference.iter().zip(&layer).any(|(a, b)| {
                        a.chars.len() != b.chars.len()
                            || [
                                (a.bounds.x0, b.bounds.x0),
                                (a.bounds.y0, b.bounds.y0),
                                (a.bounds.x1, b.bounds.x1),
                                (a.bounds.y1, b.bounds.y1),
                                (a.size, b.size),
                            ]
                            .into_iter()
                            .any(|(x, y)| (x - y * scale).abs() > 0.001)
                            || a.chars.iter().zip(&b.chars).any(|(x, y)| {
                                (x.x0 - y.x0 * scale).abs() > 0.001
                                    || (x.x1 - y.x1 * scale).abs() > 0.001
                            })
                    })
                {
                    return Err(JsValue::from_str(
                        "Physical construction-piece geometry mismatch",
                    ));
                }
            }
        }
    }
    if name == "space_units" {
        let a = pdf_diff::text_layer_from_bytes(&old, 1)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let b = pdf_diff::text_layer_from_bytes(&new, 1)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        if a.len() != 1
            || b.len() != 1
            || a[0].text != "Alpha Beta"
            || b[0].text != a[0].text
            || a[0].chars.len() != b[0].chars.len()
            || a[0].chars.iter().zip(&b[0].chars).any(|(x, y)| {
                (x.x0 - 64.0 * y.x0).abs() > 0.001 || (x.x1 - 64.0 * y.x1).abs() > 0.001
            })
        {
            return Err(JsValue::from_str(
                "Generated-space physical geometry mismatch",
            ));
        }
    }
    if name.starts_with("tounicode_override") {
        let ir = pdf_diff::load_document_from_bytes(&old, &pdf_diff::Limits::default())
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        if ir.pages[0].text() != "ó€éê" || ir.pages[0].sdk_unicode_mapping_errors != 0 {
            return Err(JsValue::from_str(
                "ToUnicode override full-page gold mismatch",
            ));
        }
        let layer = pdf_diff::text_layer_from_bytes(&old, 1)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        if layer.iter().map(|r| r.text.as_str()).collect::<String>() != "ó€éê" {
            return Err(JsValue::from_str("ToUnicode text-layer gold mismatch"));
        }
    }
    let mut options = pdf_diff::DiffOptions::default();
    if name == "budget" {
        options.visual.as_mut().unwrap().max_total_pixels = 720_000;
    }
    if name.starts_with("oversized_widget_") {
        let error = pdf_diff::diff_bytes(
            "old",
            "new",
            &old,
            &new,
            &pdf_diff::Limits::default(),
            &options,
        )
        .unwrap_err();
        let same = pdf_diff::diff_bytes(
            "new",
            "same",
            &new,
            &new,
            &pdf_diff::Limits::default(),
            &options,
        )
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
        return Ok(serde_json::json!({"rejected":true, "error":format!("{error:#}"),
            "same_complete":same.is_comparison_complete(),"same_source":same.comparison.unwrap().identical_source}).to_string());
    }
    let diff = pdf_diff::diff_bytes(
        "old",
        "new",
        &old,
        &new,
        &pdf_diff::Limits::default(),
        &options,
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let same = pdf_diff::diff_bytes(
        "old",
        "same",
        &old,
        &old,
        &pdf_diff::Limits::default(),
        &options,
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&serde_json::json!({"complete":diff.is_comparison_complete(),
        "text_complete":diff.is_text_comparison_complete(),"diff":diff,
        "same_complete":same.is_comparison_complete(),"same_source":same.comparison.unwrap().identical_source}))
        .map_err(|e|JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn inspect(bytes: &[u8]) -> Result<String, JsValue> {
    let doc = pdf_diff::load_document_from_bytes(bytes, &pdf_diff::Limits::default())
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&doc).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn compare(old: &[u8], new: &[u8]) -> Result<String, JsValue> {
    let diff = pdf_diff::diff_bytes(
        "old",
        "new",
        old,
        new,
        &pdf_diff::Limits::default(),
        &pdf_diff::DiffOptions::default(),
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&diff).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn render(bytes: &[u8], page: usize, width: u32) -> Result<Vec<u8>, JsValue> {
    pdf_diff::render_page_from_bytes(bytes, page, width)
        .map(|p| p.png)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn render_with_info(bytes: &[u8], page: usize, width: u32) -> Result<String, JsValue> {
    let page = pdf_diff::render_page_from_bytes(bytes, page, width)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&serde_json::json!({
        "width": page.width, "height": page.height,
        "viewport": page.viewport, "png": page.png,
    }))
    .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn layer(bytes: &[u8], page: usize) -> Result<String, JsValue> {
    let runs = pdf_diff::text_layer_from_bytes(bytes, page)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&runs).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn render_at_dpi(bytes: &[u8], page: usize, dpi: u32) -> Result<Vec<u8>, JsValue> {
    pdf_diff::render_page_at_dpi_from_bytes(bytes, page, dpi)
        .map(|p| p.png)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn compare_with_budget(old: &[u8], new: &[u8], budget: usize) -> Result<String, JsValue> {
    let options = pdf_diff::DiffOptions {
        max_pairwise: budget,
        ..pdf_diff::DiffOptions::default()
    };
    let diff = pdf_diff::diff_bytes(
        "old",
        "new",
        old,
        new,
        &pdf_diff::Limits::default(),
        &options,
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(
        &serde_json::json!({"complete":diff.is_text_comparison_complete(),"diff":diff}),
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))
}

fn worker_params(
    params: &str,
    allowed: &[&str],
) -> Result<serde_json::Map<String, serde_json::Value>, JsValue> {
    let value: serde_json::Value =
        serde_json::from_str(params).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| JsValue::from_str("Worker parameters must be an object"))?;
    if object.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(JsValue::from_str("Unknown PDF operation parameter"));
    }
    Ok(object.clone())
}

fn worker_number(
    params: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    default: usize,
) -> Result<usize, JsValue> {
    match params.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| JsValue::from_str("Invalid page or render dimension")),
    }
}

#[wasm_bindgen]
pub fn worker_json(
    operation: &str,
    first: &[u8],
    second: &[u8],
    params: &str,
) -> Result<String, JsValue> {
    let allowed: &[&str] = match operation {
        "inspect" => &["limits"],
        "compare" => &["limits", "options", "old_name", "new_name"],
        "textLayer" => &["page"],
        _ => return Err(JsValue::from_str("Unknown JSON operation")),
    };
    let p = worker_params(params, allowed)?;
    let limits: pdf_diff::Limits = p
        .get("limits")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| JsValue::from_str(&e.to_string()))?
        .unwrap_or_default();
    match operation {
        "inspect" => {
            let doc = pdf_diff::load_document_from_bytes(first, &limits)
                .map_err(|e| JsValue::from_str(&format!("{e:#}")))?;
            serde_json::to_string(&doc).map_err(|e| JsValue::from_str(&e.to_string()))
        }
        "compare" => {
            let options: pdf_diff::DiffOptions = p
                .get("options")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| JsValue::from_str(&e.to_string()))?
                .unwrap_or_default();
            let name = |key| -> Result<&str, JsValue> {
                match p.get(key) {
                    None => Ok(if key == "old_name" { "old" } else { "new" }),
                    Some(v) => v
                        .as_str()
                        .ok_or_else(|| JsValue::from_str("Invalid document name")),
                }
            };
            let diff = pdf_diff::diff_bytes(
                name("old_name")?,
                name("new_name")?,
                first,
                second,
                &limits,
                &options,
            )
            .map_err(|e| JsValue::from_str(&format!("{e:#}")))?;
            serde_json::to_string(&diff).map_err(|e| JsValue::from_str(&e.to_string()))
        }
        _ => {
            let runs = pdf_diff::text_layer_from_bytes(first, worker_number(&p, "page", 1)?)
                .map_err(|e| JsValue::from_str(&format!("{e:#}")))?;
            serde_json::to_string(&runs).map_err(|e| JsValue::from_str(&e.to_string()))
        }
    }
}

#[wasm_bindgen]
pub fn worker_render(bytes: &[u8], params: &str, dpi: bool) -> Result<js_sys::Object, JsValue> {
    let p = worker_params(
        params,
        if dpi {
            &["page", "dpi"]
        } else {
            &["page", "width"]
        },
    )?;
    let page = worker_number(&p, "page", 1)?;
    let dimension = u32::try_from(worker_number(
        &p,
        if dpi { "dpi" } else { "width" },
        if dpi { 72 } else { 400 },
    )?)
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let rendered = if dpi {
        pdf_diff::render_page_at_dpi_from_bytes(bytes, page, dimension)
    } else {
        pdf_diff::render_page_from_bytes(bytes, page, dimension)
    }
    .map_err(|e| JsValue::from_str(&format!("{e:#}")))?;
    let metadata=serde_json::to_string(&serde_json::json!({"width":rendered.width,"height":rendered.height,"viewport":rendered.viewport}))
        .map_err(|e|JsValue::from_str(&e.to_string()))?;
    let result = js_sys::Object::new();
    js_sys::Reflect::set(
        &result,
        &JsValue::from_str("png"),
        &js_sys::Uint8Array::from(rendered.png.as_slice()),
    )?;
    js_sys::Reflect::set(
        &result,
        &JsValue::from_str("metadata"),
        &JsValue::from_str(&metadata),
    )?;
    Ok(result)
}

#[wasm_bindgen]
pub fn worker_fixture(name: &str, revised: bool) -> Result<Vec<u8>, JsValue> {
    if !appearance_fixture::CASES.contains(&name) {
        return Err(JsValue::from_str("Unknown fixture"));
    }
    Ok(appearance_fixture::fixture(name, revised))
}

#[wasm_bindgen]
pub fn worker_defaults() -> String {
    serde_json::json!({"limits":pdf_diff::Limits::default(),"options":pdf_diff::DiffOptions::default()}).to_string()
}

#[path = "../../../crates/jade-pdf-diff/tests/support/report_fixture.rs"]
mod report_fixture;
#[path = "../../../crates/jade-pdf-diff/src/sdk_probes.rs"]
#[allow(dead_code)]
mod sdk_probes;

/// 製品と同じ ABI 検証入力をブラウザ監査へ渡す。
#[wasm_bindgen]
pub fn contract_probe() -> Vec<u8> {
    sdk_probes::character_box()
}

/// 比較検証用の入力を保存済みファイルなしで生成する。
#[wasm_bindgen]
pub fn generated_report(revised: bool) -> Vec<u8> {
    report_fixture::bytes(revised)
}
