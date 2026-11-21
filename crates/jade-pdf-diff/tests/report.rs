//! 不正な入力ファイルと存在しないパスのエラーを検証する。
use std::io::{Error as IoError, ErrorKind};

use pdf_diff::{load_document_blocking, Limits};

#[test]
fn rejects_files_that_are_not_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("not.pdf");
    std::fs::write(&path, b"this is not a pdf").unwrap();
    assert!(load_document_blocking(&path, &Limits::default()).is_err());
}

/// OSや表示言語に依存せず、対象パスとファイル未発見の型付き原因を返す。
#[test]
fn reports_a_missing_file() {
    let directory = tempfile::tempdir().unwrap();
    for path in [
        directory.path().join("none.pdf"),
        directory
            .path()
            .join("未作成のディレクトリ")
            .join("none.pdf"),
    ] {
        let error = load_document_blocking(&path, &Limits::default()).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains(&path.display().to_string()),
            "unexpected error: {message}"
        );
        assert_eq!(
            error
                .downcast_ref::<IoError>()
                .expect("Missing original I/O error")
                .kind(),
            ErrorKind::NotFound
        );
    }
}
