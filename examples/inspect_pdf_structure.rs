//! PDF 解析結果の構造抽出状況を確認する example。
//!
//! 実行例:
//! `cargo run --example inspect_pdf_structure -- input.pdf`

use std::{env, path::PathBuf, process::ExitCode};

use jade_pdf2docx_docx::infer_ocr_table_from_pdf_graphics;
use jade_pdf2docx_parser::parse_pdf;

fn main() -> ExitCode {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 2 {
        eprintln!("usage: cargo run --example inspect_pdf_structure -- <input.pdf>");
        return ExitCode::from(2);
    }
    let parsed = match parse_pdf(&PathBuf::from(&args[1])) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("parse failed: {err}");
            return ExitCode::from(1);
        }
    };
    println!("pages={}", parsed.page_count);
    println!("graphics={}", parsed.graphics.len());
    println!("text_elements={}", parsed.text_elements.len());
    println!(
        "inferred_table={:?}",
        infer_ocr_table_from_pdf_graphics(&parsed).map(|table| {
            (
                table.row_count(),
                table.col_count().unwrap_or(0),
                table
                    .rows
                    .iter()
                    .flatten()
                    .filter(|cell| !cell.trim().is_empty())
                    .count(),
            )
        })
    );
    for text in parsed.text_elements.iter().take(120) {
        println!(
            "page={} x={:.1}-{:.1} y={:.1}-{:.1} text={}",
            text.page, text.bounds.x0, text.bounds.x1, text.bounds.y0, text.bounds.y1, text.text
        );
    }
    for graphic in parsed.graphics.iter().take(120) {
        println!(
            "graphic page={} x={:.1}-{:.1} y={:.1}-{:.1} width={:.1}",
            graphic.page,
            graphic.bounds.x0,
            graphic.bounds.x1,
            graphic.bounds.y0,
            graphic.bounds.y1,
            graphic.stroke_width
        );
    }
    ExitCode::SUCCESS
}
