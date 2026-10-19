//! 不正な入力ファイルと存在しないパスのエラーを検証する。
use std::path::Path;

use pdf_diff::{load_document_blocking, Limits};

#[test]
fn rejects_files_that_are_not_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not.pdf");
    std::fs::write(&path, b"this is not a pdf").unwrap();
    assert!(load_document_blocking(&path, &Limits::default()).is_err());
}

#[test]
fn reports_a_missing_file() {
    let error =
        load_document_blocking(Path::new("/nonexistent/none.pdf"), &Limits::default()).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("none.pdf") && message.contains("No such file"),
        "unexpected error: {message}"
    );
}
