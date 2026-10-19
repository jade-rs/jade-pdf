//! 最小 PDF -> DOCX 変換を実行する example。
//!
//! 実行例:
//! `cargo run --example minimum_pdf_to_docx -- input.pdf output/minimum.docx`
//! `cargo run --example minimum_pdf_to_docx -- input.pdf output/minimum.docx --clean-output`

use std::{env, path::PathBuf, process::ExitCode};

use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 || args.len() > 4 || args.get(3).is_some_and(|arg| arg != "--clean-output") {
        eprintln!(
            "usage: cargo run --example minimum_pdf_to_docx -- <input.pdf> <output.docx> [--clean-output]"
        );
        return ExitCode::from(2);
    }
    let clean_output = args.get(3).is_some_and(|arg| arg == "--clean-output");

    let request = MinimumPdfToDocxRequest {
        input_pdf_path: PathBuf::from(&args[1]),
        output_docx_path: PathBuf::from(&args[2]),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output,
    };

    match convert_minimum_pdf_to_docx(&request) {
        Ok(report) => {
            println!("converted: {}", report.source_pdf_path.display());
            println!("version: {}", report.source_pdf_version);
            println!("text preview: {}", report.source_text_preview);
            println!("output: {}", report.output_docx_path.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("conversion failed: {err}");
            ExitCode::from(1)
        }
    }
}
