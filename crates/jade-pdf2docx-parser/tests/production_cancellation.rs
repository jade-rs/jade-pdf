//! 公開解析入口のキャンセルと、停止状態の共有・解析間の独立性を固定する。
use jade_pdf2docx_parser::{ParseCancellation, ParseError, ParseOptions, parse_pdf_with_options};

/// file入口はキャンセル済みならFileを開く前に型付きエラーを返す。
#[test]
fn cancelled_file_does_not_open_missing_input() {
    let directory =
        std::env::temp_dir().join(format!("jade-pdf2docx-cancel-{}", std::process::id()));
    let path = directory.join("missing.pdf");
    let cancellation = ParseCancellation::new();
    cancellation.cancel();
    let options = ParseOptions {
        cancellation: Some(cancellation),
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_with_options(&path, &options),
        Err(ParseError::Cancelled)
    ));
}
