use std::path::PathBuf;

use js_sys::{Array, Uint8Array};
use wasm_bindgen::prelude::*;

/// PDF バイト列を最小 DOCX バイト列へ変換する。
#[wasm_bindgen(js_name = convertPdfToDocx)]
pub fn convert_pdf_to_docx(pdf_bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes("browser-input.pdf", pdf_bytes)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

/// 検出した品質欠落を拒否する strict PDF → DOCX 変換入口。
#[wasm_bindgen(js_name = convertPdfToDocxStrict)]
pub fn convert_pdf_to_docx_strict(pdf_bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes_with_options(
        "browser-input.pdf",
        pdf_bytes,
        &jade_pdf2docx_parser::ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .map_err(|error| JsValue::from_str(&error.to_string()))
}

/// レンダリング済み PNG ページ列を DOCX バイト列へ変換する。
#[wasm_bindgen(js_name = convertRenderedPngPagesToDocx)]
pub fn convert_rendered_png_pages_to_docx(
    png_pages: Array,
    widths: Array,
    heights: Array,
) -> Result<Vec<u8>, JsValue> {
    if png_pages.length() != widths.length() || png_pages.length() != heights.length() {
        return Err(JsValue::from_str(
            "png_pages, widths, and heights must have the same length",
        ));
    }
    let mut pages = Vec::new();
    for index in 0..png_pages.length() {
        let bytes = Uint8Array::new(&png_pages.get(index)).to_vec();
        let width = widths
            .get(index)
            .as_f64()
            .ok_or_else(|| JsValue::from_str("width must be a number"))?
            .round() as u32;
        let height = heights
            .get(index)
            .as_f64()
            .ok_or_else(|| JsValue::from_str("height must be a number"))?
            .round() as u32;
        pages.push(jade_pdf2docx_docx::RenderedPageImage::new(
            bytes, width, height,
        ));
    }
    jade_pdf2docx_docx::convert_rendered_png_pages_to_docx_bytes(&pages)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

/// PDF バイト列を解析し、playground 表示用 JSON を返す。
#[wasm_bindgen(js_name = inspectPdfJson)]
pub fn inspect_pdf_json(pdf_bytes: &[u8]) -> Result<String, JsValue> {
    let parsed =
        jade_pdf2docx_parser::parse_pdf_bytes(PathBuf::from("browser-input.pdf"), pdf_bytes)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let inferred_tables = jade_pdf2docx_docx::infer_ocr_tables_from_pdf_graphics(&parsed);
    let inferred_table = inferred_tables.iter().max_by_key(|table| {
        let non_empty = table
            .rows
            .iter()
            .flatten()
            .filter(|cell| !cell.trim().is_empty())
            .count();
        (non_empty, table.row_count(), table.col_count().unwrap_or(0))
    });
    let inferred_rows = inferred_table.map_or(0, |table| table.row_count());
    let inferred_cols = inferred_table
        .as_ref()
        .and_then(|table| table.col_count())
        .unwrap_or(0);
    let inferred_non_empty_cells = inferred_table.map_or(0, |table| {
        table
            .rows
            .iter()
            .flatten()
            .filter(|cell| !cell.trim().is_empty())
            .count()
    });
    Ok(format!(
        "{{\"build\":\"browser-structured-tables-20260516\",\"pdf_version\":\"{}.{}\",\"byte_len\":{},\"page_count\":{},\"graphics_count\":{},\"text_elements_count\":{},\"image_elements_count\":{},\"inferred_table_count\":{},\"inferred_table_rows\":{},\"inferred_table_cols\":{},\"inferred_non_empty_cells\":{},\"text_preview\":{},\"quality_issues\":{}}}",
        parsed.version.major,
        parsed.version.minor,
        parsed.byte_len,
        parsed.page_count,
        parsed.graphics.len(),
        parsed.text_elements.len(),
        parsed.image_elements.len(),
        inferred_tables.len(),
        inferred_rows,
        inferred_cols,
        inferred_non_empty_cells,
        json_string(&parsed.text_preview),
        quality_json(&parsed.quality),
    ))
}

/// 品質欠落の object 番号・原因または0始まりのページ番号を JSON 配列として返す。
fn quality_json(report: &jade_pdf2docx_parser::QualityReport) -> String {
    let issues = report
        .issues
        .iter()
        .map(|issue| match issue {
            jade_pdf2docx_parser::QualityIssue::MissingPageGeometry { page } => format!(
                "{{\"kind\":\"missing_page_geometry\",\"page\":{page}}}"
            ),
            jade_pdf2docx_parser::QualityIssue::UnsupportedFont { page, object_number, reason } => format!(
                "{{\"kind\":\"unsupported_font\",\"page\":{page},\"object_number\":{},\"reason\":{}}}",
                object_number.map_or_else(|| "null".to_string(), |number| number.to_string()), json_string(reason.code()),
            ),
            jade_pdf2docx_parser::QualityIssue::UnsupportedXObject { page, object_number, reason } => format!(
                "{{\"kind\":\"unsupported_xobject\",\"page\":{page},\"object_number\":{object_number},\"reason\":{}}}",
                json_string(reason.code()),
            ),
            jade_pdf2docx_parser::QualityIssue::UnsupportedClippingPath { page } => format!("{{\"kind\":\"unsupported_clipping_path\",\"page\":{page}}}"),
            jade_pdf2docx_parser::QualityIssue::UnsupportedInlineImage { page } => format!(
                "{{\"kind\":\"unsupported_inline_image\",\"page\":{page}}}"
            ),
            jade_pdf2docx_parser::QualityIssue::ImageReplaced {
                object_number,
                reason,
            } => format!(
                "{{\"kind\":\"image_replaced\",\"object_number\":{},\"reason\":{}}}",
                object_number.map_or_else(|| "null".to_string(), |number| number.to_string()),
                json_string(reason.code()),
            ),
            jade_pdf2docx_parser::QualityIssue::ImageRepaired { object_number, damaged_rows } => format!(
                "{{\"kind\":\"image_repaired\",\"object_number\":{},\"damaged_rows\":{damaged_rows}}}",
                object_number.map_or_else(|| "null".to_string(), |number| number.to_string()),
            ),
            jade_pdf2docx_parser::QualityIssue::OcrRequired { page } => {
                format!("{{\"kind\":\"ocr_required\",\"page\":{page}}}")
            }
        })
        .collect::<Vec<_>>();
    format!("[{}]", issues.join(","))
}

/// JSON 文字列リテラル用に最小エスケープする。
fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// browser inspection でも白置換の object と原因を JSON に残す。
    #[test]
    fn inspection_exposes_image_replacement() {
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Page /MediaBox [0 0 100 100] >> endobj\n2 0 obj << /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n";
        let json = inspect_pdf_json(pdf).unwrap();
        assert!(json.contains("\"quality_issues\":[{\"kind\":\"image_replaced\",\"object_number\":2,\"reason\":\"unsupported_filter\"}]"), "{json}");
    }

    /// 品質欠落がない inspection は空の配列で返す。
    #[test]
    fn inspection_reports_empty_quality_array_for_empty_page() {
        let json = inspect_pdf_json(
            b"%PDF-1.7\n1 0 obj << /Type /Page /MediaBox [0 0 100 100] >> endobj\n",
        )
        .unwrap();
        assert!(json.contains("\"quality_issues\":[]"), "{json}");
    }
}
