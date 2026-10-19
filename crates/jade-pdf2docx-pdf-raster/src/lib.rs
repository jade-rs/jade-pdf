//! poppler の `pdftoppm` を用いて PDF をページ単位の PNG にラスタ化する。
//!
//! 実行環境に `pdftoppm`（macOS では `brew install poppler` 等）が必要です。

use std::{
    fmt::{Display, Formatter},
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

mod bounded;
pub use bounded::render_pdf_page_to_png_bounded;

/// ラスタ化済み PDF ページ画像を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterizedPdfPage {
    /// ページ番号（0 始まり）。
    pub page_index: usize,
    /// PNG ファイルのパス。
    pub path: PathBuf,
    /// PNG 幅（px）。
    pub width_px: u32,
    /// PNG 高さ（px）。
    pub height_px: u32,
    /// DOCX 表示用の幅（CSS px 相当）。
    pub display_width_px: u32,
    /// DOCX 表示用の高さ（CSS px 相当）。
    pub display_height_px: u32,
}

/// PDF ラスタ化時のエラー種別。
#[derive(Debug)]
pub enum PdfRasterError {
    /// 描画領域が指定の画素予算を超える、または寸法が不正。
    RasterBudget(String),
    /// 入出力エラー。
    Io(io::Error),
    /// `pdftoppm` が PATH 上に見つからない。
    PdftoppmNotFound,
    /// `pdftoppm` が非ゼロ終了した。
    PdftoppmFailed {
        /// 終了コード（シグナル等で取得できない場合は `None`）。
        code: Option<i32>,
        /// 標準エラーに近いメッセージ。
        stderr: String,
    },
    /// 出力 PNG が 1 枚も生成されなかった。
    NoPngOutput,
    /// 生成された PNG のヘッダを読めなかった。
    InvalidPng {
        /// 対象ファイル。
        path: PathBuf,
        /// 詳細メッセージ。
        message: String,
    },
}

impl Display for PdfRasterError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfRasterError::RasterBudget(message) => write!(f, "{message}"),
            PdfRasterError::Io(err) => write!(f, "io error: {err}"),
            PdfRasterError::PdftoppmNotFound => {
                write!(f, "pdftoppm not found (install poppler-utils / poppler)")
            }
            PdfRasterError::PdftoppmFailed { code, stderr } => {
                write!(f, "pdftoppm failed (code={code:?}): {stderr}")
            }
            PdfRasterError::NoPngOutput => write!(f, "pdftoppm produced no png output"),
            PdfRasterError::InvalidPng { path, message } => {
                write!(f, "invalid png {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for PdfRasterError {}

impl From<io::Error> for PdfRasterError {
    fn from(value: io::Error) -> Self {
        PdfRasterError::Io(value)
    }
}

/// `pdftoppm` が実行可能かどうかを返す。
pub fn pdftoppm_available() -> bool {
    Command::new("pdftoppm")
        .arg("-v")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// PDF の全ページを PNG に変換し、生成されたファイルパスを **ページ順** で返す。
///
/// `out_dir` は空であることを推奨する。存在しない場合は作成する。
/// 出力ファイル名は `pdftoppm` の規則に従う（例: `prefix-01.png`）。
pub fn render_pdf_pages_to_png(
    pdf_path: &Path,
    out_dir: &Path,
    dpi: u32,
) -> Result<Vec<PathBuf>, PdfRasterError> {
    render_pdf_pages_to_png_with_range(pdf_path, out_dir, dpi, None)
}

/// PDF の指定ページだけを PNG に変換し、指定順でファイルパスを返す。
///
/// `page_indices` は 0 始まり。重複ページは 1 回だけ処理する。
pub fn render_selected_pdf_pages_to_png(
    pdf_path: &Path,
    out_dir: &Path,
    dpi: u32,
    page_indices: &[usize],
) -> Result<Vec<PathBuf>, PdfRasterError> {
    let page_indices = normalize_selected_page_indices(page_indices);
    if page_indices.is_empty() {
        return Ok(Vec::new());
    }
    fs::create_dir_all(out_dir)?;
    let mut rendered = Vec::new();
    for page_index in page_indices {
        let page_dir = out_dir.join(format!("selected-{}", page_index + 1));
        let mut pages = render_pdf_pages_to_png_with_range(
            pdf_path,
            &page_dir,
            dpi,
            Some((page_index + 1, page_index + 1)),
        )?;
        rendered.append(&mut pages);
    }
    Ok(rendered)
}

fn render_pdf_pages_to_png_with_range(
    pdf_path: &Path,
    out_dir: &Path,
    dpi: u32,
    page_range: Option<(usize, usize)>,
) -> Result<Vec<PathBuf>, PdfRasterError> {
    if dpi == 0 {
        return Err(PdfRasterError::PdftoppmFailed {
            code: None,
            stderr: "dpi must be greater than 0".to_string(),
        });
    }
    fs::create_dir_all(out_dir)?;
    let prefix = out_dir.join("page");
    let args = pdftoppm_args(pdf_path, &prefix, dpi, page_range);
    let status = Command::new("pdftoppm").args(&args).output().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            PdfRasterError::PdftoppmNotFound
        } else {
            PdfRasterError::Io(e)
        }
    })?;
    if !status.status.success() {
        return Err(PdfRasterError::PdftoppmFailed {
            code: status.status.code(),
            stderr: String::from_utf8_lossy(&status.stderr).trim().to_string(),
        });
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(out_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_pdftoppm_page_png(p, &prefix))
        .collect();
    sort_pdftoppm_page_pngs(&mut paths);
    if paths.is_empty() {
        return Err(PdfRasterError::NoPngOutput);
    }
    Ok(paths)
}

/// `pdftoppm` に渡す引数列を組み立てる。
fn pdftoppm_args(
    pdf_path: &Path,
    prefix: &Path,
    dpi: u32,
    page_range: Option<(usize, usize)>,
) -> Vec<String> {
    let mut args = vec![
        "-png".to_string(),
        "-cropbox".to_string(),
        "-r".to_string(),
        dpi.to_string(),
    ];
    if let Some((first_page, last_page)) = page_range {
        args.extend([
            "-f".to_string(),
            first_page.to_string(),
            "-l".to_string(),
            last_page.to_string(),
        ]);
    }
    args.push(pdf_path.to_string_lossy().to_string());
    args.push(prefix.to_string_lossy().to_string());
    args
}

/// 指定ページ列を昇順・重複なしに正規化する。
fn normalize_selected_page_indices(page_indices: &[usize]) -> Vec<usize> {
    let mut out = page_indices.to_vec();
    out.sort_unstable();
    out.dedup();
    out
}

/// `pdftoppm` が今回の prefix で出した PNG か判定する。
fn is_pdftoppm_page_png(path: &Path, prefix: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    if !ext.eq_ignore_ascii_case("png") {
        return false;
    }
    let Some(file_stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let Some(prefix_stem) = prefix.file_name().and_then(|stem| stem.to_str()) else {
        return false;
    };
    file_stem
        .strip_prefix(prefix_stem)
        .is_some_and(|suffix| suffix.starts_with('-'))
}

/// `pdftoppm` の `page-1.png`, `page-10.png`, `page-2.png` をページ番号順へ並べる。
fn sort_pdftoppm_page_pngs(paths: &mut [PathBuf]) {
    paths.sort_by(|a, b| {
        pdftoppm_page_number(a)
            .cmp(&pdftoppm_page_number(b))
            .then(a.cmp(b))
    });
}

/// `pdftoppm` 出力ファイル名末尾のページ番号を読む。
fn pdftoppm_page_number(path: &Path) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?;
    let page = stem.rsplit_once('-')?.1;
    page.parse::<u32>().ok()
}

/// PDF の全ページを PNG に変換し、PNG 寸法も含めてページ順で返す。
pub fn render_pdf_pages_to_png_with_size(
    pdf_path: &Path,
    out_dir: &Path,
    dpi: u32,
) -> Result<Vec<RasterizedPdfPage>, PdfRasterError> {
    render_pdf_pages_to_png(pdf_path, out_dir, dpi)?
        .into_iter()
        .enumerate()
        .map(|(page_index, path)| {
            let bytes = fs::read(&path)?;
            let (width_px, height_px) =
                png_dimensions(&bytes).ok_or_else(|| PdfRasterError::InvalidPng {
                    path: path.clone(),
                    message: "missing PNG signature or IHDR dimensions".to_string(),
                })?;
            Ok(RasterizedPdfPage {
                page_index,
                path,
                width_px,
                height_px,
                display_width_px: raster_px_to_display_px(width_px, dpi),
                display_height_px: raster_px_to_display_px(height_px, dpi),
            })
        })
        .collect()
}

/// raster DPI の pixel 寸法を 96dpi 表示寸法へ変換する。
pub fn raster_px_to_display_px(px: u32, dpi: u32) -> u32 {
    if dpi == 0 {
        return 0;
    }
    (((px as u64) * 96 + (dpi as u64 / 2)) / dpi as u64)
        .max(1)
        .min(u32::MAX as u64) as u32
}

/// PNG の IHDR から幅と高さを読む。
pub fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_minimal_pdf(path: &Path) {
        let mut f = fs::File::create(path).expect("create pdf");
        // 1 ページの最小 PDF（テキストなし）
        let pdf = br"%PDF-1.4
1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj
3 0 obj<</Type/Page/MediaBox[0 0 200 200]/Parent 2 0 R>>endobj
xref
0 4
0000000000 65535 f 
0000000010 00000 n 
0000000053 00000 n 
0000000100 00000 n 
trailer<</Size 4/Root 1 0 R>>
startxref
178
%%EOF
";
        f.write_all(pdf).expect("write pdf");
    }

    #[test]
    fn render_pdf_pages_to_png_requires_pdftoppm() {
        if !pdftoppm_available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "jade_pdf2docx_pdf_raster_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        let pdf_path = dir.join("minimal.pdf");
        write_minimal_pdf(&pdf_path);
        let out = dir.join("out");
        let pages = render_pdf_pages_to_png(&pdf_path, &out, 72).expect("rasterize");
        assert_eq!(pages.len(), 1);
        assert!(pages[0].exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_selected_pdf_pages_to_png_returns_empty_for_empty_selection() {
        let dir = std::env::temp_dir().join(format!(
            "jade_pdf2docx_pdf_raster_empty_selection_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let pages = render_selected_pdf_pages_to_png(Path::new("missing.pdf"), &dir, 72, &[])
            .expect("empty selection should not invoke pdftoppm");

        assert!(pages.is_empty());
        assert!(!dir.exists(), "empty selection should not create temp dirs");
    }

    #[test]
    fn normalize_selected_page_indices_sorts_and_dedupes() {
        assert_eq!(
            normalize_selected_page_indices(&[3, 1, 3, 0, 1]),
            vec![0, 1, 3]
        );
    }

    #[test]
    fn pdftoppm_args_include_one_based_page_range() {
        let args = pdftoppm_args(
            Path::new("input.pdf"),
            Path::new("/tmp/page"),
            150,
            Some((2, 2)),
        );

        assert_eq!(
            args,
            vec![
                "-png",
                "-cropbox",
                "-r",
                "150",
                "-f",
                "2",
                "-l",
                "2",
                "input.pdf",
                "/tmp/page"
            ]
        );
    }

    #[test]
    fn png_dimensions_reads_ihdr_size() {
        let bytes = tiny_png();
        assert_eq!(png_dimensions(&bytes), Some((1, 1)));
    }

    #[test]
    fn png_dimensions_rejects_non_png() {
        assert_eq!(png_dimensions(b"not png"), None);
    }

    #[test]
    fn raster_px_to_display_px_normalizes_pdftoppm_dpi() {
        assert_eq!(raster_px_to_display_px(1600, 192), 800);
        assert_eq!(raster_px_to_display_px(1200, 144), 800);
        assert_eq!(raster_px_to_display_px(800, 96), 800);
    }

    #[test]
    fn sort_pdftoppm_page_pngs_uses_numeric_page_order() {
        let mut paths = vec![
            PathBuf::from("page-10.png"),
            PathBuf::from("page-2.png"),
            PathBuf::from("page-1.png"),
        ];
        sort_pdftoppm_page_pngs(&mut paths);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("page-1.png"),
                PathBuf::from("page-2.png"),
                PathBuf::from("page-10.png")
            ]
        );
    }

    #[test]
    fn is_pdftoppm_page_png_rejects_unrelated_pngs() {
        let prefix = Path::new("/tmp/out/page");
        assert!(is_pdftoppm_page_png(
            Path::new("/tmp/out/page-1.png"),
            prefix
        ));
        assert!(!is_pdftoppm_page_png(
            Path::new("/tmp/out/other-1.png"),
            prefix
        ));
        assert!(!is_pdftoppm_page_png(
            Path::new("/tmp/out/page-1.ppm"),
            prefix
        ));
    }

    fn tiny_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xff, 0xff, 0x3f, 0x00, 0x05, 0xfe, 0x02, 0xfe, 0xdc, 0xcc, 0x59,
            0xe7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ]
    }
}
