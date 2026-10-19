#![cfg(not(target_arch = "wasm32"))]
//! 親の停止要求でもworker成果物を公開せず、一時領域を回収する。
use jade_pdf2docx_docx::{ConvertError, ParseCancellation, StagedDocxOutput};
use std::fs;

/// 停止済みなら成果物の有無にかかわらず既存出力と未作成directoryを保持する。
#[test]
fn cancelled_commit_preserves_output_and_cleans_workspace() {
    for existing in [false, true] {
        for document in [false, true] {
            let base = tempfile::tempdir().unwrap();
            let input = base.path().join("input.pdf");
            let output = if existing {
                base.path().join("output.docx")
            } else {
                base.path().join("new/output.docx")
            };
            fs::write(&input, b"input pdf").unwrap();
            if existing {
                fs::write(&output, b"existing output").unwrap();
            }
            let staged = StagedDocxOutput::new(&input, &output).unwrap();
            let workspace = staged.path().parent().unwrap().to_path_buf();
            if document {
                fs::write(staged.path(), b"completed docx").unwrap();
            }
            let cancellation = ParseCancellation::new();
            cancellation.cancel();
            assert!(matches!(
                staged.commit_with_cancellation(&cancellation),
                Err(ConvertError::Parse(
                    jade_pdf2docx_parser::ParseError::Cancelled
                ))
            ));
            assert!(!workspace.exists());
            assert_eq!(fs::read(input).unwrap(), b"input pdf");
            if existing {
                assert_eq!(fs::read(output).unwrap(), b"existing output");
            } else {
                assert!(!output.parent().unwrap().exists());
            }
        }
    }
}

/// 未停止ならchunk境界と空成果物を含め全byteを公開する。
#[test]
fn uncancelled_commit_publishes_all_bytes() {
    for length in [0, 1, 8192, 8193] {
        let base = tempfile::tempdir().unwrap();
        let input = base.path().join("input.pdf");
        let output = base.path().join("output.docx");
        fs::write(&input, b"input pdf").unwrap();
        let staged = StagedDocxOutput::new(&input, &output).unwrap();
        let workspace = staged.path().parent().unwrap().to_path_buf();
        let bytes = vec![42; length];
        fs::write(staged.path(), &bytes).unwrap();
        staged
            .commit_with_cancellation(&ParseCancellation::new())
            .unwrap();
        assert_eq!(fs::read(output).unwrap(), bytes);
        assert!(!workspace.exists());
    }
}
