use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// 一意なサフィックスを生成する。
pub fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time error")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("{pid}_{now}_{seq}")
}

/// 一時的な DOCX 出力パスを生成する。
#[allow(dead_code)]
pub fn temp_output_docx_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "jade_pdf2docx_example_minimum_{}.docx",
        unique_suffix()
    ))
}

/// 一時的な PPM 画像パスを生成する。
#[cfg(feature = "ocr")]
#[allow(dead_code)]
pub fn temp_output_ppm_path() -> PathBuf {
    std::env::temp_dir().join(format!("jade_pdf2docx_example_ocr_{}.ppm", unique_suffix()))
}

/// `--help` の期待出力スナップショット。
#[allow(dead_code)]
pub fn expected_minimum_ocr_help_output() -> &'static str {
    include_str!("../../examples/support/minimum_pdf_to_docx_with_ocr_help.txt")
}

/// テスト用の単純な白黒 PPM を生成する。
#[cfg(feature = "ocr")]
#[allow(dead_code)]
pub fn write_test_ppm_with_band(
    path: &std::path::Path,
    width: usize,
    height: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
) {
    let mut body = format!("P3\n{width} {height}\n255\n");
    for y in 0..height {
        for x in 0..width {
            let in_band = x >= x0 && x < x1 && y >= y0 && y < y1;
            if in_band {
                body.push_str("0 0 0\n");
            } else {
                body.push_str("255 255 255\n");
            }
        }
    }
    std::fs::write(path, body).expect("failed to write ppm");
}
