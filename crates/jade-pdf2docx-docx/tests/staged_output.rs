#![cfg(not(target_arch = "wasm32"))]
//! 隔離workerの成果物は正常終了後だけ公開し、中断時には一時領域を回収する。
use jade_pdf2docx_docx::{ConvertError, StagedDocxOutput};
use std::fs;

/// workerの部分DOCXと書込用一時ファイルを回収し、出力directoryを作らない。
#[test]
fn abandoning_staged_output_preserves_destination_and_removes_workspace() {
    for existing in [false, true] {
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
        fs::write(staged.path(), b"partial docx").unwrap();
        fs::write(workspace.join("worker-write.tmp"), b"partial write").unwrap();
        drop(staged);
        assert!(!workspace.exists());
        assert_eq!(fs::read(input).unwrap(), b"input pdf");
        if existing {
            assert_eq!(fs::read(output).unwrap(), b"existing output");
        } else {
            assert!(!output.parent().unwrap().exists());
        }
    }
}

/// 完成したworker成果物をchunk境界を越えてatomicに公開し、一時領域を回収する。
#[test]
fn committing_staged_output_preserves_all_bytes_and_cleans_workspace() {
    for length in [0, 1, 8191, 8192, 8193, 16385] {
        let base = tempfile::tempdir().unwrap();
        let input = base.path().join("input.pdf");
        let output = base.path().join("new/output.docx");
        fs::write(&input, b"input pdf").unwrap();
        let staged = StagedDocxOutput::new(&input, &output).unwrap();
        let workspace = staged.path().parent().unwrap().to_path_buf();
        let bytes = vec![42; length];
        fs::write(staged.path(), &bytes).unwrap();
        staged.commit().unwrap();
        assert_eq!(fs::read(&output).unwrap(), bytes);
        assert_eq!(fs::read(input).unwrap(), b"input pdf");
        assert!(!workspace.exists());
        assert_eq!(fs::read_dir(output.parent().unwrap()).unwrap().count(), 1);
    }
}

/// 入力aliasをworker起動前に拒否する。
#[test]
fn staged_output_rejects_input_alias() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    fs::write(&input, b"input pdf").unwrap();
    assert!(matches!(
        StagedDocxOutput::new(&input, &input),
        Err(ConvertError::OutputAliasesInput)
    ));
    let alias = base.path().join("alias.docx");
    fs::hard_link(&input, &alias).unwrap();
    assert!(matches!(
        StagedDocxOutput::new(&input, &alias),
        Err(ConvertError::OutputAliasesInput)
    ));
    assert_eq!(fs::read(input).unwrap(), b"input pdf");
}

/// 成果物のない正常終了を保存成功にせず、既存出力を保持する。
#[test]
fn missing_staged_document_preserves_existing_output() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    fs::write(&input, b"input pdf").unwrap();
    fs::write(&output, b"existing output").unwrap();
    let staged = StagedDocxOutput::new(&input, &output).unwrap();
    let workspace = staged.path().parent().unwrap().to_path_buf();
    assert!(matches!(staged.commit(), Err(ConvertError::Io(_))));
    assert_eq!(fs::read(output).unwrap(), b"existing output");
    assert!(!workspace.exists());
}

/// commit失敗時は対象directoryと入力を保持し、双方の一時ファイルを残さない。
#[test]
fn failed_staged_commit_cleans_all_temporary_files() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    fs::write(&input, b"input pdf").unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(output.join("keep"), b"existing output").unwrap();
    let staged = StagedDocxOutput::new(&input, &output).unwrap();
    let workspace = staged.path().parent().unwrap().to_path_buf();
    fs::write(staged.path(), b"completed docx").unwrap();
    assert!(matches!(staged.commit(), Err(ConvertError::Io(_))));
    assert_eq!(fs::read(output.join("keep")).unwrap(), b"existing output");
    assert_eq!(fs::read(input).unwrap(), b"input pdf");
    assert!(!workspace.exists());
    assert_eq!(fs::read_dir(base.path()).unwrap().count(), 2);
}

/// 将来の入力aliasも、worker領域を予約する前に拒否する。
#[test]
fn future_output_alias_is_rejected_before_worker_reservation() {
    for hardlink in [false, true] {
        let base = tempfile::tempdir().unwrap();
        let input = base.path().join("input.pdf");
        fs::write(&input, b"input pdf").unwrap();
        fs::create_dir(base.path().join("parent")).unwrap();
        let name = if hardlink {
            fs::hard_link(&input, base.path().join("alias.docx")).unwrap();
            "alias.docx"
        } else {
            "input.pdf"
        };
        let output = base.path().join("parent/missing/../..").join(name);
        assert!(matches!(
            StagedDocxOutput::new(&input, &output),
            Err(ConvertError::OutputAliasesInput)
        ));
        assert_eq!(fs::read(&input).unwrap(), b"input pdf");
        assert!(!base.path().join("parent/missing").exists());
    }
}
