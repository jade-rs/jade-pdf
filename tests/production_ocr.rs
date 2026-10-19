//! 通常CLIの監督経路を通るOCR回帰。
use std::{fs, path::Path, process::Command};

/// 通常CLIへ明示OCRのモデル・文字セットを渡す。
fn conversion(input: &Path, output: &Path, assets: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"));
    command
        .arg(input)
        .arg(output)
        .arg("--ocr")
        .arg("--ocr-model")
        .arg(assets.join("parseq.onnx"))
        .arg("--ocr-charset")
        .arg(assets.join("NDLmoji.yaml"));
    command
}

/// 実runtimeがないbuildでは明示OCRを拒否し、既存出力を保持する。
#[cfg(not(feature = "ocr-onnx"))]
#[test]
fn explicit_ocr_requires_runtime_feature() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.pdf");
    let output = directory.path().join("output.docx");
    fs::write(&input, b"%PDF-1.4\n").unwrap();
    fs::write(&output, b"keep").unwrap();
    let result = conversion(&input, &output, directory.path())
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("ocr-onnx"),
        "{:?}",
        result
    );
    assert_eq!(fs::read(&output).unwrap(), b"keep");
}
