//! 明示Pdfiumの不備を自動取得や別cacheで隠さないことを独立プロセスで確認する。
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::process::Command;

#[allow(dead_code)]
#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// 自動取得側の版更新時にもoffline配布のpinを再検討させる。
#[test]
fn bundled_release_matches_automatic_backend() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/pdfium_lock.json");
    let lock: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        lock["release"],
        format!("chromium/{}", pdfium_auto::PDFIUM_VERSION)
    );
}

/// 環境変数を他testと共有せず、native fallbackが必要なfixtureを解析する。
#[test]
fn explicit_library_child() {
    if std::env::var_os("JADE_PDF2DOCX_TEST_EXPLICIT_PDFIUM").is_none() {
        return;
    }
    let input = PathBuf::from(std::env::var_os("JADE_PDF2DOCX_TEST_EXPLICIT_PDFIUM").unwrap());
    let error =
        jade_pdf2docx_parser::parse_pdf(&input).expect_err("explicit invalid library must fail");
    assert!(
        error.to_string().contains("PDFIUM_LIB_PATH"),
        "explicit override must be identified without automatic download: {error}"
    );
    let cache = std::env::var_os("PDFIUM_AUTO_CACHE_DIR").unwrap();
    assert!(
        !PathBuf::from(cache).exists(),
        "must not create automatic cache"
    );
}

/// 欠落・空指定・directory・不正binaryをnative backendが明示エラーにする。
#[test]
fn invalid_explicit_library_never_downloads() {
    let directory = std::env::temp_dir().join(format!(
        "jade-pdf2docx-pdfium-offline-{}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let input = directory.join("input.pdf");
    std::fs::write(
        &input,
        fixture::pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F 5 0 R >> >> >>".into(),
            fixture::stream("BT /F 12 Tf 10 80 Td (offline Pdfium) Tj ET"),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        ]),
    )
    .unwrap();
    let invalid = directory.join("invalid library");
    std::fs::write(&invalid, b"not a dynamic library").unwrap();
    let cases = [
        directory.join("missing"),
        PathBuf::new(),
        directory.clone(),
        invalid,
    ];
    for path in cases {
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "explicit_library_child", "--nocapture"])
            .env("JADE_PDF2DOCX_TEST_EXPLICIT_PDFIUM", &input)
            .env("PDFIUM_LIB_PATH", &path)
            .env(
                "PDFIUM_AUTO_CACHE_DIR",
                directory.join("never-create-cache"),
            )
            .env("HTTPS_PROXY", "http://127.0.0.1:1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "path {path:?}: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}
