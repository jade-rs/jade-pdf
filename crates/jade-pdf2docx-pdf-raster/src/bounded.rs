//! ページの描画領域を制限し、切り抜きによる本文欠落を検出する。
use super::*;
use std::{io::Read, process::Stdio};

/// 1ページを元DPIで描画し、想定寸法を超えるPNGを拒否する。
/// 各辺+1画素の領域を予約することで、想定より大きいページの切り抜きを検出する。
/// パスはOS表現を保持する。子プロセスの期限と総RSSは呼出元で監督すること。
pub fn render_pdf_page_to_png_bounded(
    pdf_path: &Path,
    out_dir: &Path,
    dpi: u32,
    page: usize,
    width: u32,
    height: u32,
    max_pixels: u64,
) -> Result<PathBuf, PdfRasterError> {
    let valid_edge = |value: u32| value > 0 && value < i32::MAX as u32;
    let number = page
        .checked_add(1)
        .filter(|value| *value <= i32::MAX as usize);
    if dpi == 0
        || number.is_none()
        || !valid_edge(width)
        || !valid_edge(height)
        || (u64::from(width) + 1) * (u64::from(height) + 1) > max_pixels
    {
        return Err(PdfRasterError::RasterBudget(
            "invalid or exceeded raster budget".into(),
        ));
    }
    fs::create_dir_all(out_dir)?;
    let prefix = out_dir.join("page");
    let number = number.unwrap().to_string();
    let status = Command::new("pdftoppm")
        .args([
            "-png",
            "-cropbox",
            "-singlefile",
            "-r",
            &dpi.to_string(),
            "-f",
            &number,
            "-l",
            &number,
            "-W",
            &(width + 1).to_string(),
            "-H",
            &(height + 1).to_string(),
        ])
        .arg(pdf_path)
        .arg(&prefix)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                PdfRasterError::PdftoppmNotFound
            } else {
                PdfRasterError::Io(error)
            }
        })?;
    if !status.success() {
        return Err(PdfRasterError::PdftoppmFailed {
            code: status.code(),
            stderr: "bounded page raster failed".into(),
        });
    }
    let path = prefix.with_extension("png");
    let mut header = [0u8; 24];
    fs::File::open(&path)?.read_exact(&mut header)?;
    let (actual_width, actual_height) =
        png_dimensions(&header).ok_or_else(|| PdfRasterError::InvalidPng {
            path: path.clone(),
            message: "missing PNG signature or IHDR dimensions".into(),
        })?;
    if actual_width > width || actual_height > height {
        return Err(PdfRasterError::RasterBudget(format!(
            "raster dimension exceeded: expected <= {width}x{height}, got {actual_width}x{actual_height}"
        )));
    }
    Ok(path)
}
