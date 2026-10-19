//! PDF をページ画像としてレンダリングし、視覚保持 DOCX を生成する example。
//!
//! 実行例:
//! `cargo run --features render --example render_pdf_to_docx -- input.pdf output.docx`

#[cfg(feature = "render")]
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(feature = "render")]
use jade_pdf2docx_docx::{RenderedPageImage, convert_rendered_png_pages_to_docx_bytes};

#[cfg(feature = "render")]
use jade_pdf2docx_pdf_raster::render_pdf_pages_to_png_with_size;

#[cfg(feature = "render")]
fn main() -> ExitCode {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        eprintln!(
            "usage: cargo run --features render --example render_pdf_to_docx -- <input.pdf> <output.docx>"
        );
        return ExitCode::from(2);
    }

    match render_pdf_to_docx(Path::new(&args[1]), Path::new(&args[2])) {
        Ok(report) => {
            println!("rendered: {}", report.input_pdf.display());
            println!("pages: {}", report.page_count);
            println!("output: {}", report.output_docx.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("render conversion failed: {err}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(feature = "render"))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "render_pdf_to_docx requires the `render` feature: cargo run --features render --example render_pdf_to_docx -- <input.pdf> <output.docx>"
    );
    std::process::ExitCode::from(2)
}

#[cfg(feature = "render")]
/// レンダリング変換の結果サマリ。
struct RenderReport {
    /// 入力 PDF。
    input_pdf: PathBuf,
    /// 出力 DOCX。
    output_docx: PathBuf,
    /// レンダリングしたページ数。
    page_count: usize,
}

#[cfg(feature = "render")]
/// PDF を PNG ページへラスタ化し、その画像を DOCX に貼り込む。
fn render_pdf_to_docx(input_pdf: &Path, output_docx: &Path) -> Result<RenderReport, String> {
    let temp_dir = unique_temp_dir("jade_pdf2docx_render_pdf_to_docx");
    fs::create_dir_all(&temp_dir).map_err(|e| format!("failed to create temp dir: {e}"))?;
    let result = render_pdf_to_docx_with_temp_dir(input_pdf, output_docx, &temp_dir);
    let _ = fs::remove_dir_all(&temp_dir);
    result
}

#[cfg(feature = "render")]
/// 指定された一時ディレクトリを使って PDF を視覚保持 DOCX に変換する。
fn render_pdf_to_docx_with_temp_dir(
    input_pdf: &Path,
    output_docx: &Path,
    temp_dir: &Path,
) -> Result<RenderReport, String> {
    let pages = render_pdf_pages_to_png_with_size(input_pdf, &temp_dir, 144)
        .map_err(|e| format!("failed to rasterize PDF: {e}"))?;
    let mut rendered_pages = Vec::with_capacity(pages.len());
    for page in &pages {
        let png_bytes = fs::read(&page.path)
            .map_err(|e| format!("failed to read rendered PNG {}: {e}", page.path.display()))?;
        rendered_pages.push(RenderedPageImage::new(
            png_bytes,
            page.display_width_px,
            page.display_height_px,
        ));
    }

    let docx_bytes = convert_rendered_png_pages_to_docx_bytes(&rendered_pages)
        .map_err(|e| format!("failed to build DOCX: {e}"))?;
    if let Some(parent) = output_docx.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("failed to create output dir: {e}"))?;
    }
    fs::write(output_docx, docx_bytes).map_err(|e| format!("failed to write DOCX: {e}"))?;

    Ok(RenderReport {
        input_pdf: input_pdf.to_path_buf(),
        output_docx: output_docx.to_path_buf(),
        page_count: pages.len(),
    })
}

#[cfg(feature = "render")]
/// 一時ディレクトリ名を作る。
fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    env::temp_dir().join(format!("{prefix}_{nanos}"))
}
