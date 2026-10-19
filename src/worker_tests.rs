//! 実際の子プロセスとIPCで停止・回収を固定し、sleep時間による結果判定を避ける。
use crate::{
    cli::Request,
    worker::{ChildGuard, WorkerError, run_isolated, wait_worker},
};
use jade_pdf2docx_docx::{ParseCancellation, StagedDocxOutput};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    time::Duration,
};

/// fixtureの子だけに処理内容と一時出力パスを渡す環境変数。
const FIXTURE_PATH: &str = "JADE_PDF2DOCX_TEST_WORKER_OUTPUT";
/// fixtureの停止/成功/失敗を選ぶ環境変数。
const FIXTURE_MODE: &str = "JADE_PDF2DOCX_TEST_WORKER_MODE";
/// 子の準備完了を伝えるIPC marker。
const READY: &str = "JADE_PDF2DOCX_WORKER_READY";

/// 子専用のfixtureは部分ファイルを作り、ready送信後に永久停止または終了する。
#[test]
fn process_fixture() {
    let Some(path) = std::env::var_os(FIXTURE_PATH) else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    fs::write(&path, b"worker document").unwrap();
    fs::write(path.parent().unwrap().join("incomplete.tmp"), b"partial").unwrap();
    println!("{READY}");
    std::io::stdout().flush().unwrap();
    match std::env::var(FIXTURE_MODE).unwrap().as_str() {
        "hang" => loop {
            std::thread::park();
        },
        "fail" => std::process::exit(7),
        "success" => (),
        mode => panic!("unknown fixture mode: {mode}"),
    }
}

/// helper childのreadyを待ってから監督を開始する。
fn fixture(
    staged: &StagedDocxOutput,
    mode: &str,
) -> (ChildGuard, BufReader<std::process::ChildStdout>) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "worker_tests::process_fixture", "--nocapture"])
        .env(FIXTURE_PATH, staged.path())
        .env(FIXTURE_MODE, mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = ChildGuard::spawn(&mut command).unwrap();
    let mut reader = BufReader::new(child.child.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        assert!(
            reader.read_line(&mut line).unwrap() > 0,
            "child exited before ready"
        );
        if line.contains(READY) {
            break;
        }
    }
    (child, reader)
}

/// 期限境界ちょうどで永久停止childを強制終了・waitし、部分成果物を回収する。
#[test]
fn expired_worker_is_killed_reaped_and_never_committed() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    fs::write(&input, b"input pdf").unwrap();
    fs::write(&output, b"existing output").unwrap();
    let staged = StagedDocxOutput::new(&input, &output).unwrap();
    let workspace = staged.path().parent().unwrap().to_path_buf();
    let (mut child, _reader) = fixture(&staged, "hang");
    let limit = Duration::from_millis(100);
    let result = wait_worker(&mut child, limit, || limit, &ParseCancellation::new());
    assert!(matches!(result, Err(WorkerError::Timeout { timeout }) if timeout == limit));
    assert!(child.reaped);
    assert!(!child.child.try_wait().unwrap().unwrap().success());
    drop(child);
    drop(staged);
    assert!(!workspace.exists());
    assert_eq!(fs::read(input).unwrap(), b"input pdf");
    assert_eq!(fs::read(output).unwrap(), b"existing output");
}

/// 正常終了したchildだけを成功とし、完成成果物を公開できる。
#[test]
fn successful_worker_is_reaped_before_commit() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    fs::write(&input, b"input pdf").unwrap();
    let staged = StagedDocxOutput::new(&input, &output).unwrap();
    let workspace = staged.path().parent().unwrap().to_path_buf();
    let (mut child, _reader) = fixture(&staged, "success");
    wait_worker(
        &mut child,
        Duration::from_secs(30),
        || Duration::ZERO,
        &ParseCancellation::new(),
    )
    .unwrap();
    assert!(child.reaped);
    assert!(child.child.try_wait().unwrap().unwrap().success());
    staged.commit().unwrap();
    assert_eq!(fs::read(output).unwrap(), b"worker document");
    assert!(!workspace.exists());
}

/// 非ゼロ終了を型付き失敗にし、部分成果物を公開しない。
#[test]
fn failed_worker_is_reaped_and_never_committed() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    fs::write(&input, b"input pdf").unwrap();
    fs::write(&output, b"existing output").unwrap();
    let staged = StagedDocxOutput::new(&input, &output).unwrap();
    let workspace = staged.path().parent().unwrap().to_path_buf();
    let (mut child, _reader) = fixture(&staged, "fail");
    assert!(
        matches!(wait_worker(&mut child, Duration::from_secs(30), || Duration::ZERO, &ParseCancellation::new()), Err(WorkerError::Failed { status }) if status.code() == Some(7))
    );
    assert!(child.reaped);
    drop(staged);
    assert!(!workspace.exists());
    assert_eq!(fs::read(output).unwrap(), b"existing output");
}

/// 起動失敗も最終出力directoryを作らず、型付きI/Oエラーになる。
#[test]
fn spawn_failure_preserves_destination_directory() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    fs::write(&input, b"input pdf").unwrap();
    let request = Request {
        input,
        input_sha256: None,
        publication_receipt: None,
        output: base.path().join("new/output.docx"),
        strict: false,
        debug: false,
        ocr: None,
        memory: None,
        timeout: Duration::from_secs(30),
    };
    assert!(matches!(
        run_isolated(
            &base.path().join("no-such-worker"),
            &request,
            &ParseCancellation::new()
        ),
        Err(WorkerError::Io(_))
    ));
    assert!(!request.output.parent().unwrap().exists());
}

/// 期限を過ぎて観測した正常終了も成功公開しない。
#[test]
fn expired_deadline_precedes_successful_status() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    fs::write(&input, b"input pdf").unwrap();
    let staged = StagedDocxOutput::new(&input, &base.path().join("new/output.docx")).unwrap();
    let (mut child, _reader) = fixture(&staged, "success");
    child.child.wait().unwrap();
    assert!(matches!(
        wait_worker(
            &mut child,
            Duration::from_millis(1),
            || { Duration::from_millis(2) },
            &ParseCancellation::new()
        ),
        Err(WorkerError::Timeout { .. })
    ));
    assert!(child.reaped);
    assert!(!base.path().join("new").exists());
}

/// 監視中の停止要求でchildを回収し、既存出力と一時領域を保持・回収する。
#[test]
fn interruption_kills_reaps_and_discards_worker_output() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    fs::write(&input, b"input pdf").unwrap();
    fs::write(&output, b"existing output").unwrap();
    let staged = StagedDocxOutput::new(&input, &output).unwrap();
    let workspace = staged.path().parent().unwrap().to_path_buf();
    let (mut child, _reader) = fixture(&staged, "hang");
    let cancellation = ParseCancellation::new();
    let result = wait_worker(
        &mut child,
        Duration::from_secs(30),
        || {
            cancellation.cancel();
            Duration::ZERO
        },
        &cancellation,
    );
    assert!(matches!(result, Err(WorkerError::Interrupted)));
    assert!(child.reaped);
    assert!(!child.child.try_wait().unwrap().unwrap().success());
    drop(child);
    drop(staged);
    assert!(!workspace.exists());
    assert_eq!(fs::read(input).unwrap(), b"input pdf");
    assert_eq!(fs::read(output).unwrap(), b"existing output");
}

/// 停止要求は観測済みの期限超過・終了成功より先に判定する。
#[test]
fn interruption_precedes_timeout_and_successful_status() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    fs::write(&input, b"input pdf").unwrap();
    let staged = StagedDocxOutput::new(&input, &base.path().join("new/output.docx")).unwrap();
    let (mut child, _reader) = fixture(&staged, "success");
    child.child.wait().unwrap();
    let cancellation = ParseCancellation::new();
    cancellation.cancel();
    assert!(matches!(
        wait_worker(
            &mut child,
            Duration::from_millis(1),
            || Duration::from_millis(2),
            &cancellation
        ),
        Err(WorkerError::Interrupted)
    ));
    assert!(child.reaped);
    assert!(!base.path().join("new").exists());
}

/// 起動前の停止なら一時領域・出力directoryを作らずworkerを起動しない。
#[test]
fn cancelled_request_is_rejected_before_worker_spawn() {
    let base = tempfile::tempdir().unwrap();
    let request = Request {
        input: base.path().join("input.pdf"),
        input_sha256: None,
        publication_receipt: None,
        output: base.path().join("new/output.docx"),
        strict: false,
        debug: false,
        ocr: None,
        memory: None,
        timeout: Duration::from_secs(30),
    };
    let cancellation = ParseCancellation::new();
    cancellation.cancel();
    assert!(matches!(
        run_isolated(&base.path().join("no-such-worker"), &request, &cancellation),
        Err(WorkerError::Interrupted)
    ));
    assert_eq!(fs::read_dir(base.path()).unwrap().count(), 0);
}

/// 親の受付より先に観測したworkerのSIGINT終了も中断として扱う。
#[cfg(unix)]
#[test]
fn worker_sigint_is_interruption_before_parent_handler_observes_signal() {
    assert_worker_signal("-INT", 2, true);
}

/// 親の受付前にworkerがSIGTERM終了しても利用者の中断として扱う。
#[cfg(unix)]
#[test]
fn worker_sigterm_is_interruption_before_parent_handler_observes_signal() {
    assert_worker_signal("-TERM", 15, true);
}

/// 親の受付前にworkerがSIGHUP終了しても利用者の中断として扱う。
#[cfg(unix)]
#[test]
fn worker_sighup_is_interruption_before_parent_handler_observes_signal() {
    assert_worker_signal("-HUP", 1, true);
}

/// 中断対象でないSIGKILLはworker障害として報告し、出力を保持する。
#[cfg(unix)]
#[test]
fn worker_sigkill_remains_a_failure() {
    assert_worker_signal("-KILL", 9, false);
}

/// 実子processのsignal終了分類・回収と保存抑止を確認する。
#[cfg(unix)]
fn assert_worker_signal(signal: &str, number: i32, interruption: bool) {
    use std::os::unix::process::ExitStatusExt;
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    fs::write(&input, b"input pdf").unwrap();
    fs::write(&output, b"existing output").unwrap();
    let staged = StagedDocxOutput::new(&input, &output).unwrap();
    let workspace = staged.path().parent().unwrap().to_path_buf();
    let (mut child, _reader) = fixture(&staged, "hang");
    assert!(
        Command::new("kill")
            .args([signal, &child.child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(child.child.wait().unwrap().signal(), Some(number));
    let result = wait_worker(
        &mut child,
        Duration::from_secs(30),
        || Duration::ZERO,
        &ParseCancellation::new(),
    );
    if interruption {
        assert!(
            matches!(result, Err(WorkerError::Interrupted)),
            "{result:?}"
        );
    } else {
        assert!(
            matches!(result, Err(WorkerError::Failed { status }) if status.signal() == Some(number))
        );
    }
    assert!(child.reaped);
    drop(staged);
    assert_eq!(fs::read(input).unwrap(), b"input pdf");
    assert_eq!(fs::read(output).unwrap(), b"existing output");
    assert!(!workspace.exists());
}
