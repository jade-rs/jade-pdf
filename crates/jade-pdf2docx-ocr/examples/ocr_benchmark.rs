//! 固定画像を逐次認識し、初回と再利用時の実測内訳をTSVへ出力する。
use jade_pdf2docx_ocr::{OcrPageContent, OcrRequest, PageOcrSession};
use std::path::PathBuf;

/// 各画像に新しいセッションを割り当て、初回1回と再利用5回を比較する。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let assets = PathBuf::from(
        std::env::var_os("JADE_PDF2DOCX_OCR_ASSETS").ok_or("JADE_PDF2DOCX_OCR_ASSETS required")?,
    );
    let images: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if images.is_empty() {
        return Err("usage: ocr_benchmark <image>...".into());
    }
    println!("image\trun\ttotal_ms\tload_ms\trecognize_ms\tprocessing_ms\tloads\tchunks");
    for image in images {
        let request = OcrRequest::new(image.to_string_lossy())
            .with_image_path(&image)
            .with_parseq_paths(assets.join("parseq.onnx"), assets.join("NDLmoji.yaml"));
        let mut session = PageOcrSession::default();
        let mut original = None;
        for run in 0..6 {
            let (content, timings) = session.recognize_page_with_timings(&request)?;
            if let Some(expected) = &original {
                assert_eq!(&content, expected, "recognition changed between runs");
            } else {
                if let OcrPageContent::PositionedText(lines) = &content {
                    for line in lines {
                        eprintln!("{}\t{:?}\t{}", image.display(), line.bounds, line.text);
                    }
                }
                original = Some(content);
            }
            println!(
                "{}\t{run}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{}\t{}",
                image.display(),
                timings.total.as_secs_f64() * 1000.0,
                timings.model_load.as_secs_f64() * 1000.0,
                timings.inference.as_secs_f64() * 1000.0,
                timings.processing.as_secs_f64() * 1000.0,
                timings.model_load_count,
                timings.inference_count
            );
        }
    }
    Ok(())
}
