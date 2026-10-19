//! 保存中の停止点を決定論的に選び、atomic公開と回収を検証する。
use super::StagedDocxOutput;
use crate::{
    ConvertError, ParseCancellation, ParseOptions,
    conversion_control::{self, Stage},
};
use std::fs;

/// 呼出元所有の保存領域での成功・開始前停止・不在領域を公開APIで固定する。
#[test]
fn externally_owned_commit_directory_keeps_publication_contract() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    fs::write(&input, b"input").unwrap();
    fs::write(&output, b"existing").unwrap();
    let directory = tempfile::tempdir_in(base.path()).unwrap();
    for mode in ["cancelled", "missing", "success"] {
        let staged = StagedDocxOutput::new(&input, &output).unwrap();
        let workspace = staged.path().parent().unwrap().to_owned();
        fs::write(staged.path(), b"completed").unwrap();
        let cancellation = ParseCancellation::new();
        if mode == "cancelled" {
            cancellation.cancel();
        }
        let missing = base.path().join("missing");
        let destination = if mode == "missing" {
            &missing
        } else {
            directory.path()
        };
        let result = staged.commit_in_directory_with_cancellation(destination, &cancellation);
        match mode {
            "cancelled" => assert!(matches!(
                result,
                Err(ConvertError::Parse(
                    jade_pdf2docx_parser::ParseError::Cancelled
                ))
            )),
            "missing" => assert!(matches!(result, Err(ConvertError::Io(_)))),
            _ => result.unwrap(),
        }
        assert!(!workspace.exists());
        assert_eq!(
            fs::read(&output).unwrap(),
            if mode == "success" {
                b"completed".as_slice()
            } else {
                b"existing".as_slice()
            }
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
        assert!(directory.path().exists());
    }
}

/// 各停止点で旧出力を保持し、worker領域と保存用一時ファイルを回収する。
#[test]
fn staged_commit_cancels_before_write_during_copy_and_before_publish() {
    for stop in [
        Stage::Start,
        Stage::BeforeWrite,
        Stage::Writing,
        Stage::BeforeCommit,
    ] {
        for length in [0, 1, 8192, 8193] {
            if length == 0 && stop == Stage::Writing {
                continue;
            }
            let base = tempfile::tempdir().unwrap();
            let input = base.path().join("input.pdf");
            let output = base.path().join("output.docx");
            fs::write(&input, b"input pdf").unwrap();
            fs::write(&output, b"existing output").unwrap();
            let staged = StagedDocxOutput::new(&input, &output).unwrap();
            let workspace = staged.path().parent().unwrap().to_path_buf();
            fs::write(staged.path(), vec![42; length]).unwrap();
            let cancellation = ParseCancellation::new();
            let options = ParseOptions {
                cancellation: Some(cancellation.clone()),
                ..Default::default()
            };
            let mut observed = false;
            let result = staged.commit_controlled(|stage| {
                if stage == stop {
                    observed = true;
                    cancellation.cancel();
                }
                conversion_control::check(&options)
            });
            assert!(observed, "missing stop: {stop:?}, length={length}");
            assert!(matches!(
                result,
                Err(ConvertError::Parse(
                    jade_pdf2docx_parser::ParseError::Cancelled
                ))
            ));
            assert_eq!(fs::read(input).unwrap(), b"input pdf");
            assert_eq!(fs::read(output).unwrap(), b"existing output");
            assert!(!workspace.exists());
            assert_eq!(fs::read_dir(base.path()).unwrap().count(), 2);
        }
    }
}

/// 最終確認を通過した停止要求では公開済み成果物を失敗扱いにしない。
#[test]
fn cancellation_after_final_check_does_not_reverse_successful_commit() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    fs::write(&input, b"input pdf").unwrap();
    let staged = StagedDocxOutput::new(&input, &output).unwrap();
    fs::write(staged.path(), b"completed docx").unwrap();
    let cancellation = ParseCancellation::new();
    staged
        .commit_controlled(|stage| {
            assert!(!cancellation.is_cancelled());
            if stage == Stage::BeforeCommit {
                cancellation.cancel();
            }
            Ok(())
        })
        .unwrap();
    assert!(cancellation.is_cancelled());
    assert_eq!(fs::read(output).unwrap(), b"completed docx");
}

/// 親による公開待ちでは同期済み候補だけを残し、最終出力を変更しない。
#[test]
fn prepared_document_leaves_final_output_untouched() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    fs::write(&input, b"input").unwrap();
    for existing in [false, true] {
        if existing {
            fs::write(&output, b"existing").unwrap();
        }
        for cancelled in [false, true] {
            let directory = tempfile::tempdir_in(base.path()).unwrap();
            let staged = StagedDocxOutput::new(&input, &output).unwrap();
            let workspace = staged.path().parent().unwrap().to_owned();
            fs::write(staged.path(), b"completed").unwrap();
            let cancellation = ParseCancellation::new();
            if cancelled {
                cancellation.cancel();
            }
            let result =
                staged.prepare_in_directory_with_cancellation(directory.path(), &cancellation);
            if cancelled {
                assert!(matches!(
                    result,
                    Err(ConvertError::Parse(
                        jade_pdf2docx_parser::ParseError::Cancelled
                    ))
                ));
                assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
            } else {
                assert_eq!(fs::read(result.unwrap()).unwrap(), b"completed");
                assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
            }
            assert!(!workspace.exists());
            if existing {
                assert_eq!(fs::read(&output).unwrap(), b"existing");
            } else {
                assert!(!output.exists());
            }
        }
    }
}

/// 準備用領域は呼出元の所有済みdirectoryに限定し、未作成の領域を作らない。
#[test]
fn preparation_rejects_missing_owned_directory() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    let missing = base.path().join("missing");
    fs::write(&input, b"input").unwrap();
    fs::write(&output, b"existing").unwrap();
    let staged = StagedDocxOutput::new(&input, &output).unwrap();
    fs::write(staged.path(), b"completed").unwrap();
    assert!(matches!(
        staged.prepare_in_directory_with_cancellation(&missing, &ParseCancellation::new()),
        Err(ConvertError::Io(_))
    ));
    assert!(!missing.exists());
    assert_eq!(fs::read(&output).unwrap(), b"existing");
}
