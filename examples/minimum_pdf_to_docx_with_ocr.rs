//! OCR を任意適用して最小 PDF -> DOCX 変換を実行する example。
//!
//! 実行例:
//! `cargo run --example minimum_pdf_to_docx_with_ocr -- input.pdf output/minimum.docx --ocr-mock "line1|line2"`
//! `cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- input.pdf output/minimum.docx --ocr-mock "line1|line2"`
//! `cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- input.pdf output/minimum.docx --ocr-image /path/to/input.ppm --ocr-parseq-model /path/to/model.onnx --ocr-charset /path/to/charset.yaml --ocr-min-confidence 0.5`
//! スキャン PDF（画像のみ）向け: `--ocr-from-pdf`（poppler の `pdftoppm` が PATH に必要）、本文だけの DOCX は `--clean-output`。
//! 自動判定: `--ocr-auto` は画像があり、抽出テキストがほぼ無いページだけ `--ocr-from-pdf` 相当を実行。
//!
//! 優先順位:
//! 1. `--ocr-image` が指定され、OCR 成功かつ非空行が得られた場合はその結果を使用。
//! 2. 1 が失敗/空の場合、`--ocr-from-pdf` または `--ocr-auto` 判定一致ページをラスタ化して OCR。
//! 3. 2 が失敗/空の場合、`--ocr-mock` があれば警告して明示された mock 行を使用。
//!
//! 認識失敗や診断ラベルだけの結果は保存前に拒否する。画像本文 OCR にはモデルと charset が必要。
//! 現在の feature は上流 ONNX runtime を有効化しておらず、実モデル推論の提供は別途必要。
//!
//! Phase 1（表）: `--ocr-table-json` でパイプライン JSON を読み HTML 表を取り込む。`--ocr-table-pipeline` は
//! `--ocr-image` または `--ocr-from-pdf`（先頭ページ PNG）と併用し、外部コマンドの stdout JSON から表を構築する。
//! Phase 2（表・幾何）: `--ocr-table-geometry-json` でセル bbox + テキスト bbox の JSON から [`jade_pdf2docx_ocr::parse_geometry_table_json`] 経由で表を構築する。
//!
//! `cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- input.pdf output/table.docx --ocr-table-geometry-json table_geometry.json --clean-output`
//! `cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- input.pdf output/table.docx --ocr-table-geometry-json table_geometry.json --clean-output`
//! `cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- input.pdf output/table.docx --ocr-table-geometry-json table_geometry.json --clean-output`
//! `cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- input.pdf output/table.docx --ocr-table-geometry-json table_geometry.json --clean-output`
//! `cargo run --features ocr --example minimum_pdf_to_docx_with_ocr -- input.pdf output/table.docx --ocr-table-geometry-json table_geometry.json --clean-output`

use std::{env, path::Path, path::PathBuf, process::ExitCode};

use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, OcrTable, convert_minimum_pdf_to_docx};

#[path = "support/minimum_pdf_to_docx_with_ocr_cli.rs"]
mod cli;

use cli::{
    CLI_HINT_USE_HELP_FOR_FULL_OPTIONS, CliArgs, CliParseAction, parse_cli_args, print_help,
    print_usage_to_stderr,
};

#[cfg(feature = "ocr")]
#[path = "support/ocr_text.rs"]
mod ocr_text;
#[cfg(feature = "ocr")]
use ocr_text::collect_optional_ocr_lines;

/// OCR と表の検証後に DOCX を保存する。
fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let parsed = match parse_cli_args(&args) {
        Ok(parsed) => parsed,
        Err(CliParseAction::ShowHelp) => {
            print_help();
            return ExitCode::SUCCESS;
        }
        Err(CliParseAction::ShowError(message)) => {
            eprintln!("{message}");
            eprintln!();
            print_usage_to_stderr();
            eprintln!("{CLI_HINT_USE_HELP_FOR_FULL_OPTIONS}");
            return ExitCode::from(2);
        }
    };

    let input_pdf_path = PathBuf::from(&parsed.input_pdf);
    let ocr_lines = match collect_optional_ocr_lines(&parsed, &input_pdf_path) {
        Ok(lines) => lines,
        Err(message) => {
            eprintln!("OCR failed: {message}");
            return ExitCode::from(1);
        }
    };
    let ocr_table = match resolve_ocr_table(&parsed, &input_pdf_path) {
        Ok(t) => t,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input_pdf_path.clone(),
        output_docx_path: PathBuf::from(parsed.output_docx),
        ocr_text_lines: ocr_lines.clone(),
        ocr_table: ocr_table.clone(),
        clean_output: parsed.clean_output,
    };

    match convert_minimum_pdf_to_docx(&request) {
        Ok(report) => {
            println!("converted: {}", report.source_pdf_path.display());
            println!("version: {}", report.source_pdf_version);
            println!("text preview: {}", report.source_text_preview);
            println!("output: {}", report.output_docx_path.display());
            if let Some(lines) = ocr_lines {
                println!("ocr lines used: {}", lines.len());
            }
            if let Some(ref table) = ocr_table {
                println!(
                    "ocr table: {} rows x {} cols",
                    table.row_count(),
                    table.col_count().unwrap_or(0)
                );
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("conversion failed: {err}");
            ExitCode::from(1)
        }
    }
}

#[cfg(feature = "ocr")]
fn resolve_ocr_table(parsed: &CliArgs, input_pdf: &Path) -> Result<Option<OcrTable>, String> {
    use std::fs;
    use std::path::Path;

    use jade_pdf2docx_docx::ocr_table_from_html;
    use jade_pdf2docx_ocr::{
        parse_geometry_table_json, parse_table_pipeline_json, run_table_pipeline_command,
    };
    use jade_pdf2docx_pdf_raster::{pdftoppm_available, render_pdf_pages_to_png};

    if let Some(path) = parsed.ocr_table_json.as_deref() {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("failed to read --ocr-table-json {path}: {e}"))?;
        let out = parse_table_pipeline_json(&raw)
            .map_err(|e| format!("failed to parse table pipeline JSON: {e}"))?;
        let t = ocr_table_from_html(&out.html)
            .map_err(|e| format!("failed to convert HTML table: {e}"))?;
        t.validate()
            .map_err(|e| format!("invalid table after HTML parse: {e}"))?;
        return Ok(Some(t));
    }

    if let Some(path) = parsed.ocr_table_geometry_json.as_deref() {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("failed to read --ocr-table-geometry-json {path}: {e}"))?;
        let t = parse_geometry_table_json(&raw)
            .map_err(|e| format!("failed to parse geometry table JSON: {e}"))?;
        t.validate()
            .map_err(|e| format!("invalid table after geometry match: {e}"))?;
        return Ok(Some(t));
    }

    if let Some(exe) = parsed.ocr_table_pipeline.as_deref() {
        let out = if let Some(image_path) = parsed.ocr_image.as_deref() {
            run_table_pipeline_command(Path::new(exe), &[], Path::new(image_path))
                .map_err(|e| format!("table pipeline failed: {e}"))?
        } else if parsed.ocr_from_pdf {
            if !pdftoppm_available() {
                return Err(
                    "pdftoppm not found; install poppler for --ocr-from-pdf with --ocr-table-pipeline"
                        .to_string(),
                );
            }
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let tmp = std::env::temp_dir().join(format!("jade_pdf2docx_ocr_table_pdf_{nanos}"));
            fs::create_dir_all(&tmp).map_err(|e| format!("failed to create temp dir: {e}"))?;
            let pages = render_pdf_pages_to_png(input_pdf, &tmp, parsed.ocr_pdf_dpi)
                .map_err(|e| format!("failed to rasterize PDF for table pipeline: {e}"))?;
            let first = pages
                .first()
                .ok_or_else(|| "PDF produced no raster pages for table pipeline".to_string())?;
            let pipeline_out = run_table_pipeline_command(Path::new(exe), &[], first)
                .map_err(|e| format!("table pipeline failed: {e}"));
            let _ = fs::remove_dir_all(&tmp);
            pipeline_out?
        } else {
            return Err("internal error: --ocr-table-pipeline without image source".to_string());
        };

        let t = ocr_table_from_html(&out.html)
            .map_err(|e| format!("failed to convert HTML table: {e}"))?;
        t.validate()
            .map_err(|e| format!("invalid table after HTML parse: {e}"))?;
        return Ok(Some(t));
    }

    Ok(None)
}

#[cfg(not(feature = "ocr"))]
fn resolve_ocr_table(_parsed: &CliArgs, _input_pdf: &Path) -> Result<Option<OcrTable>, String> {
    Ok(None)
}

/// OCR feature が無効な場合は本文 OCR を実行しない。
#[cfg(not(feature = "ocr"))]
fn collect_optional_ocr_lines(
    _parsed: &CliArgs,
    _input_pdf: &Path,
) -> Result<Option<Vec<String>>, String> {
    Ok(None)
}
