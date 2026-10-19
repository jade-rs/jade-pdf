//! 外側領域のmarker完成前に親が失われても、作成した監督で回収する。
#![cfg(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "android",
    target_os = "ios",
    target_os = "freebsd"
))]
use crate::cli::Request;
use jade_pdf2docx_docx::ParseCancellation;
use std::{fs, path::Path, time::Duration};

/// 本番の親/監督処理を別プロセスで実行する専用fixture。
#[test]
fn process_fixture() {
    let Some(executable) = std::env::var_os("JADE_PDF2DOCX_TEST_STARTUP_EXECUTABLE") else {
        return;
    };
    let input =
        std::path::PathBuf::from(std::env::var_os("JADE_PDF2DOCX_TEST_STARTUP_INPUT").unwrap());
    let output =
        std::path::PathBuf::from(std::env::var_os("JADE_PDF2DOCX_TEST_STARTUP_OUTPUT").unwrap());
    let cancellation = ParseCancellation::new();
    if std::env::var("JADE_PDF2DOCX_TEST_STARTUP_ROLE").is_ok_and(|role| role == "parent") {
        let request = Request {
            input,
            input_sha256: None,
            publication_receipt: None,
            output,
            strict: false,
            debug: false,
            ocr: None,
            memory: None,
            timeout: Duration::from_secs(3),
        };
        crate::supervisor::run(Path::new(&executable), &request, &cancellation).unwrap();
        return;
    }
    fs::write(input.with_extension("pid"), std::process::id().to_string()).unwrap();
    let _parent_lease = crate::supervisor::watch_parent(&cancellation).unwrap();
    let _workspace = match crate::supervisor::start_workspace(&cancellation) {
        Ok(workspace) => workspace,
        Err(crate::worker::WorkerError::Interrupted) => return,
        Err(error) => panic!("startup failed: {error}"),
    };
    while !cancellation.is_cancelled() {
        std::thread::sleep(Duration::from_millis(10));
    }
}
