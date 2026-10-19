//! メモリ制限失敗時の入力・既存出力・子孫の保護を実監督で固定する。
#![cfg(unix)]
use crate::{
    cli::{MemoryOptions, Request},
    worker::WorkerError,
};
use jade_pdf2docx_docx::ParseCancellation;
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

/// 通常directoryでの制限指定は監督・出力directoryの作成より先に失敗する。
#[test]
fn memory_setup_failure_does_not_start_conversion() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let marker = base.path().join("input.pdf.started");
    let executable = base.path().join("supervisor");
    fs::write(&input, b"original").unwrap();
    fs::write(&executable, "#!/bin/sh\ntouch \"$2.started\"\nexit 7\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let output = base.path().join("not-created/out.docx");
    let request = Request {
        input: input.clone(),
        input_sha256: None,
        publication_receipt: None,
        output: output.clone(),
        strict: false,
        debug: false,
        timeout: Duration::from_secs(5),
        ocr: None,
        memory: Some(MemoryOptions {
            bytes: 64 * 1024 * 1024,
            parent: base.path().to_owned(),
        }),
    };
    let result = crate::supervisor::run(&executable, &request, &ParseCancellation::new());
    assert!(matches!(result, Err(WorkerError::Io(_))), "{result:?}");
    assert!(!marker.exists());
    assert!(!output.parent().unwrap().exists());
    assert_eq!(fs::read(input).unwrap(), b"original");
}

/// 合算OOMは容量超過として報告し、元PDFと既存DOCX・委譲先を保持する。
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a delegated cgroup v2 memory controller"]
fn memory_oom_preserves_existing_output_and_reaps_conversion() {
    let parent = std::path::PathBuf::from(
        std::env::var_os("JADE_PDF2DOCX_TEST_CGROUP_PARENT").expect("cgroup parent required"),
    );
    let count = fs::read_dir(&parent).unwrap().count();
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("destination/out.docx");
    fs::write(&input, b"original").unwrap();
    fs::create_dir(output.parent().unwrap()).unwrap();
    fs::write(&output, b"existing").unwrap();
    let executable = base.path().join("supervisor");
    let test_exe = std::env::current_exe().unwrap();
    // 専用の環境変数へOSパスを渡し、shellへパスを埋め込まない。
    fs::write(&executable, format!("#!/bin/sh\nread greeting\n{}\"$JADE_PDF2DOCX_MEMORY_TEST_EXECUTABLE\" --ignored --exact memory_group_tests::memory_holder --nocapture &\n\"$JADE_PDF2DOCX_MEMORY_TEST_EXECUTABLE\" --ignored --exact memory_group_tests::memory_holder --nocapture &\nwait\n", crate::supervisor_tests::WORKSPACE_HANDSHAKE)).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    // 自分自身を専用fixtureとして起動し、並行テストの環境を変更しない。
    let status = std::process::Command::new(test_exe)
        .args([
            "--ignored",
            "--exact",
            "memory_supervisor_tests::memory_supervisor_fixture",
            "--nocapture",
        ])
        .env(
            "JADE_PDF2DOCX_MEMORY_TEST_EXECUTABLE",
            std::env::current_exe().unwrap(),
        )
        .env("JADE_PDF2DOCX_MEMORY_SCRIPT", &executable)
        .env("JADE_PDF2DOCX_MEMORY_INPUT", &input)
        .env("JADE_PDF2DOCX_MEMORY_OUTPUT", &output)
        .env("JADE_PDF2DOCX_TEST_HOLD", "1")
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read(&input).unwrap(), b"original");
    assert_eq!(fs::read(&output).unwrap(), b"existing");
    assert_eq!(fs::read_dir(output.parent().unwrap()).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&parent).unwrap().count(), count);
}

/// 実監督へ明示した上限を渡す子プロセス専用fixture。
#[cfg(target_os = "linux")]
#[test]
#[ignore = "internal subprocess fixture"]
fn memory_supervisor_fixture() {
    let Some(executable) = std::env::var_os("JADE_PDF2DOCX_MEMORY_SCRIPT") else {
        return;
    };
    let bytes = std::env::var("JADE_PDF2DOCX_MEMORY_LIMIT_BYTES")
        .map(|value| value.parse().unwrap())
        .unwrap_or(128 * 1024 * 1024);
    let request = Request {
        input: std::env::var_os("JADE_PDF2DOCX_MEMORY_INPUT")
            .unwrap()
            .into(),
        input_sha256: None,
        publication_receipt: None,
        output: std::env::var_os("JADE_PDF2DOCX_MEMORY_OUTPUT")
            .unwrap()
            .into(),
        strict: false,
        debug: false,
        timeout: Duration::from_secs(15),
        ocr: None,
        memory: Some(MemoryOptions {
            bytes,
            parent: std::env::var_os("JADE_PDF2DOCX_TEST_CGROUP_PARENT")
                .unwrap()
                .into(),
        }),
    };
    let result = crate::supervisor::run(
        std::path::Path::new(&executable),
        &request,
        &ParseCancellation::new(),
    );
    assert!(
        matches!(result, Err(WorkerError::MemoryLimit { bytes: actual }) if actual == bytes),
        "{result:?}"
    );
}

/// exec中のOOMで初期pipeが閉じても、容量超過の診断を失わない。
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a delegated cgroup v2 memory controller"]
fn memory_oom_before_greeting_is_reported() {
    let parent = std::env::var_os("JADE_PDF2DOCX_TEST_CGROUP_PARENT").unwrap();
    let count = fs::read_dir(&parent).unwrap().count();
    let base = tempfile::tempdir().unwrap();
    for _ in 0..8 {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "memory_supervisor_tests::memory_supervisor_fixture",
                "--nocapture",
            ])
            .env("JADE_PDF2DOCX_MEMORY_SCRIPT", "/bin/true")
            .env("JADE_PDF2DOCX_MEMORY_LIMIT_BYTES", "1")
            .env("JADE_PDF2DOCX_MEMORY_INPUT", base.path().join("input.pdf"))
            .env(
                "JADE_PDF2DOCX_MEMORY_OUTPUT",
                base.path().join("output.docx"),
            )
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(fs::read_dir(base.path()).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), count);
    }
}
