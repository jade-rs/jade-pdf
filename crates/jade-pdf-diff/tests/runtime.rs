//! Exercise SDK selection in child processes, without mutating parent env vars.
use std::process::{Command, Output};
#[path = "support/report_fixture.rs"]
mod report_fixture;

fn cli(cache: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pdf-diff"));
    command
        .arg("--ir")
        .arg(report_fixture::path("report-old.pdf"))
        .env("JADE_PDFIUM_CACHE_DIR", cache)
        .env_remove("PDFIUM_LIB_PATH")
        .env_remove("PDFIUM_AUTO_CACHE_DIR");
    command
}

fn accepted(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ir: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!ir["pages"].as_array().unwrap().is_empty());
}

#[test]
fn explicit_invalid_sdk_path_never_falls_back_to_a_download() {
    let cache = tempfile::tempdir().unwrap();
    let output = cli(cache.path())
        .env("PDFIUM_LIB_PATH", cache.path().join("missing-library"))
        .env("PDFIUM_NO_AUTO_DOWNLOAD", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Cannot open Pdfium library"));
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
}

#[test]
fn offline_cold_cache_uses_embedded_sdk_or_returns_an_explicit_error() {
    let cache = tempfile::tempdir().unwrap();
    let output = cli(cache.path())
        .env("PDFIUM_NO_AUTO_DOWNLOAD", "1")
        .output()
        .unwrap();
    if cfg!(feature = "bundled-pdfium") {
        accepted(output);
    } else {
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("automatic download is disabled"));
    }
}

#[test]
#[ignore = "downloads the pinned SDK into an isolated cold cache; run explicitly in runtime CI"]
fn cold_download_is_concurrent_and_a_corrupted_cache_is_repaired() {
    let cache = tempfile::tempdir().unwrap();
    let first = cli(cache.path())
        .env_remove("PDFIUM_NO_AUTO_DOWNLOAD")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let second = cli(cache.path())
        .env_remove("PDFIUM_NO_AUTO_DOWNLOAD")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    accepted(first.wait_with_output().unwrap());
    accepted(second.wait_with_output().unwrap());
    let version = cache
        .path()
        .join(format!("pdfium-{}", pdf_diff::PDFIUM_VERSION));
    let directory = std::fs::read_dir(version)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let library = std::fs::read_dir(directory)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.file_name().unwrap() != "install.lock")
        .unwrap();
    let original = std::fs::read(&library).unwrap();
    let mut corrupted = original.clone();
    corrupted[100] ^= 1; // Same size: a length-only cache check cannot detect this.
    std::fs::write(&library, corrupted).unwrap();
    if !cfg!(feature = "bundled-pdfium") {
        let output = cli(cache.path())
            .env("PDFIUM_NO_AUTO_DOWNLOAD", "1")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    accepted(
        cli(cache.path())
            .env_remove("PDFIUM_NO_AUTO_DOWNLOAD")
            .output()
            .unwrap(),
    );
    assert_eq!(std::fs::read(library).unwrap(), original);
}
