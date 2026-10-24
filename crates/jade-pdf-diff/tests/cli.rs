#[path = "support/report_fixture.rs"]
mod report_fixture;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
}

fn fixture(name: &str) -> std::path::PathBuf {
    report_fixture::path(name)
}

#[test]
fn exit_codes_distinguish_equal_changed_and_failed_comparisons() {
    let old = fixture("report-old.pdf");
    let new = fixture("report-new.pdf");
    for (right, expected) in [(&old, 0), (&new, 1)] {
        let output = cli().arg(&old).arg(right).output().unwrap();
        assert_eq!(output.status.code(), Some(expected));
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["schema_version"], 1);
    }
    let directory = tempfile::tempdir().unwrap();
    let invalid = directory.path().join("invalid.pdf");
    std::fs::write(&invalid, b"not a PDF").unwrap();
    let output = cli().arg(&old).arg(&invalid).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!output.stderr.is_empty());
    assert_eq!(cli().output().unwrap().status.code(), Some(2));
}

#[test]
fn license_notices_include_the_source_glyph_table_without_loading_pdfium() {
    let directory = tempfile::tempdir().unwrap();
    let cache = directory.path().join("not-created");
    let output = cli()
        .arg("--licenses")
        .env("PDFIUM_NO_AUTO_DOWNLOAD", "1")
        .env("JADE_PDFIUM_CACHE_DIR", &cache)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains(pdf_diff::PDFIUM_LICENSE_NOTICES));
    assert!(text.contains(pdf_diff::ADOBE_GLYPH_LIST_LICENSE));
    assert!(text.contains("Copyright 2002-2019 Adobe"));
    assert!(!cache.exists());
}
