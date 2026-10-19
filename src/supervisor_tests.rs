//! 親leaseの初期通信・切断・I/O異常を変換開始より先に固定する。
use crate::supervisor::{GREETING, observe_parent, validate_lease};
use jade_pdf2docx_docx::ParseCancellation;
use std::io::{self, Cursor, Read};

/// shell fixtureも実監督と同じ完成marker/親の引受け確認を経て保存要求を出す。
#[cfg(unix)]
pub(crate) const WORKSPACE_HANDSHAKE: &str = "mkdir \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE\" || exit 7\nprintf 'JADE-PDF2DOCX-WORKSPACE-3\\n%s' \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE\" > \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE/workspace-marker-pending\"\nmv \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE/workspace-marker-pending\" \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE/.jade-pdf2docx-workspace-owner\"\nwhile [ ! -f \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE/workspace-ack\" ]; do sleep 0.01; done\n";

/// 完全な初期通信だけを受け入れ、欠落・不正内容を拒否する。
#[test]
fn lease_requires_complete_matching_greeting() {
    validate_lease(&mut Cursor::new(GREETING)).unwrap();
    for end in 0..GREETING.len() {
        assert_eq!(
            validate_lease(&mut Cursor::new(&GREETING[..end]))
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
    let mut malformed = GREETING.to_vec();
    malformed[0] ^= 1;
    assert_eq!(
        validate_lease(&mut Cursor::new(malformed))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
}

/// EOFで親の消滅を知らせ、予期しない追加データも停止として扱う。
#[test]
fn closed_or_corrupt_lease_cancels_conversion() {
    for bytes in [&[][..], &[1][..]] {
        let cancellation = ParseCancellation::new();
        observe_parent(Cursor::new(bytes), &cancellation);
        assert!(cancellation.is_cancelled());
    }
}

/// 最初のInterruptedだけを再試行し、次のI/O障害では停止するreader。
struct FailingReader {
    /// 最初の読み込みかどうか。
    first: bool,
}

impl Read for FailingReader {
    /// 割り込みを一度返し、その後はBrokenPipeで親の監視不能を表す。
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        let kind = if std::mem::take(&mut self.first) {
            io::ErrorKind::Interrupted
        } else {
            io::ErrorKind::BrokenPipe
        };
        Err(kind.into())
    }
}

/// Interruptedは再試行し、監視不能は停止へ変換する。
#[test]
fn lease_read_error_cancels_after_interrupted_retry() {
    let cancellation = ParseCancellation::new();
    observe_parent(FailingReader { first: true }, &cancellation);
    assert!(cancellation.is_cancelled());
}

/// 保存開始前・所有通知後の成功終了を、親の公開成功と誤認しない。
#[cfg(unix)]
#[test]
fn supervisor_success_without_parent_publication_is_rejected() {
    use crate::{cli::Request, worker::WorkerError};
    use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
    for existing in [false, true] {
        for request_commit in [false, true] {
            let base = tempfile::tempdir().unwrap();
            let input = base.path().join("input.pdf");
            let output = base.path().join("destination/output.docx");
            fs::write(&input, b"input").unwrap();
            if existing {
                fs::create_dir(output.parent().unwrap()).unwrap();
                fs::write(&output, b"existing").unwrap();
            }
            let executable = base.path().join("supervisor-fixture");
            let script = if request_commit {
                format!(
                    "#!/bin/sh\nread greeting\nprintf '%s' \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE\" > \"$2.workspace\"\n{WORKSPACE_HANDSHAKE}touch \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE/commit-request\"\nwhile [ ! -f \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE/commit-grant\" ]; do sleep 0.01; done\nexit 0\n"
                )
            } else {
                "#!/bin/sh\nread greeting\nprintf '%s' \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE\" > \"$2.workspace\"\nexit 0\n".to_owned()
            };
            fs::write(&executable, script).unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            let request = Request {
                input: input.clone(),
                input_sha256: None,
                publication_receipt: None,
                output: output.clone(),
                strict: false,
                debug: false,
                ocr: None,
                memory: None,
                timeout: Duration::from_secs(2),
            };
            let result = crate::supervisor::run(&executable, &request, &ParseCancellation::new());
            assert!(
                matches!(result, Err(WorkerError::UnpublishedOutput)),
                "unpublished supervisor success was accepted: {result:?}"
            );
            if existing {
                assert_eq!(fs::read(&output).unwrap(), b"existing");
            } else {
                assert!(!output.exists());
            }
            if output.parent().unwrap().exists() {
                assert_eq!(
                    fs::read_dir(output.parent().unwrap()).unwrap().count(),
                    usize::from(existing)
                );
            }
            let workspace = fs::read_to_string(input.with_extension("pdf.workspace")).unwrap();
            assert!(!std::path::Path::new(&workspace).exists());
            assert_eq!(fs::read(&input).unwrap(), b"input");
        }
    }
}

/// 監督が失敗終了しても同じ群で動く子孫を残さず、既存出力を保持する。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
#[test]
fn failed_supervisor_stops_its_remaining_process_group() {
    use crate::{cli::Request, worker::WorkerError};
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        process::Command,
        time::{Duration, Instant},
    };
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("supervisor-fixture");
    let marker = directory.path().join("descendant-pid");
    let output = directory.path().join("output.docx");
    fs::write(&output, b"original").unwrap();
    fs::write(&executable, b"#!/bin/sh\nread greeting\n/bin/sh -c 'while :; do sleep 1; done' \"$2\" </dev/null >/dev/null 2>/dev/null &\necho $! > \"$2\"\nexit 7\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let request = Request {
        input: marker.clone(),
        input_sha256: None,
        publication_receipt: None,
        output: output.clone(),
        strict: false,
        debug: false,
        ocr: None,
        memory: None,
        timeout: Duration::from_secs(10),
    };
    let result = crate::supervisor::run(&executable, &request, &ParseCancellation::new());
    assert!(matches!(result, Err(WorkerError::Failed {status}) if status.code()==Some(7)));
    let pid = fs::read_to_string(&marker).unwrap();
    let pid = pid.trim();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut remains = false;
    while Instant::now() < deadline {
        let status = Command::new("ps")
            .args(["-p", pid, "-o", "command="])
            .output()
            .unwrap();
        remains = String::from_utf8_lossy(&status.stdout).contains(marker.to_str().unwrap());
        if !remains {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if remains {
        let _ = Command::new("kill").args(["-KILL", pid]).status();
    }
    assert!(!remains, "descendant survived failed supervisor");
    assert_eq!(fs::read(&output).unwrap(), b"original");
}

/// 監督の起動後にworkerへ到達しなくても、親の期限で監督と子孫を回収する。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
#[test]
fn deadline_stops_unresponsive_supervisor_and_descendant() {
    assert_unresponsive_supervisor(false);
}

/// 保存領域準備が失敗した早期returnでも、停止に応じない監督を期限で回収する。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
#[test]
fn output_setup_error_reaps_unresponsive_supervisor_at_deadline() {
    assert_unresponsive_supervisor(true);
}

/// 無応答の実fixtureで通常待機と早期失敗時のDropの期限を同じ条件で確認する。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
fn assert_unresponsive_supervisor(output_setup_error: bool) {
    use crate::{cli::Request, worker::WorkerError};
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        process::Command,
        time::{Duration, Instant},
    };
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("supervisor-fixture");
    let marker = directory.path().join("descendant-pid");
    let output = directory.path().join("output.docx");
    fs::write(&output, b"original").unwrap();
    let commit_request = if output_setup_error {
        format!(
            "{WORKSPACE_HANDSHAKE}printf request > \"$JADE_PDF2DOCX_SUPERVISOR_WORKSPACE/commit-request\"\n"
        )
    } else {
        String::new()
    };
    fs::write(&executable, format!("#!/bin/sh\necho $$ > \"$2.supervisor\"\nread greeting\n/bin/sh -c 'while :; do sleep 1; done' \"$2\" </dev/null >/dev/null 2>/dev/null &\necho $! > \"$2\"\n{commit_request}wait\n")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let request = Request {
        input: marker.clone(),
        input_sha256: None,
        publication_receipt: None,
        output: if output_setup_error {
            output.join("nested.docx")
        } else {
            output.clone()
        },
        strict: false,
        debug: false,
        ocr: None,
        memory: None,
        // 準備失敗のケースは、子の起動・要求通知より先の短い期限を避ける。
        // 無応答の回収期限とfixture全体の5秒上限は引き続き検査する。
        timeout: if output_setup_error {
            Duration::from_secs(2)
        } else {
            Duration::from_millis(500)
        },
    };
    let started = Instant::now();
    let limit = request.timeout;
    let (sender, receiver) = std::sync::mpsc::channel();
    let runner = std::thread::spawn(move || {
        sender
            .send(crate::supervisor::run(
                &executable,
                &request,
                &ParseCancellation::new(),
            ))
            .unwrap();
    });
    let result = receiver.recv_timeout(Duration::from_secs(5));
    if result.is_err() {
        // Redでも所有するfixture群を停止し、runnerを回収してから失敗させる。
        let pid = fs::read_to_string(marker.with_extension("supervisor")).unwrap();
        let state = Command::new("ps")
            .args(["-p", pid.trim(), "-o", "command="])
            .output()
            .unwrap();
        if String::from_utf8_lossy(&state.stdout).contains(marker.to_str().unwrap()) {
            assert!(
                Command::new("/bin/kill")
                    .args(["-KILL", "--", &format!("-{}", pid.trim())])
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    runner.join().unwrap();
    let result = result.expect("supervisor exceeded conversion deadline");
    if output_setup_error {
        assert!(matches!(result, Err(WorkerError::Io(_))), "{result:?}");
    } else {
        assert!(matches!(result, Err(WorkerError::Timeout { timeout }) if timeout == limit));
    }
    assert!(started.elapsed() < Duration::from_secs(5));
    let pid = fs::read_to_string(&marker).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut remains = true;
    while remains && Instant::now() < deadline {
        let state = Command::new("ps")
            .args(["-p", pid.trim(), "-o", "command="])
            .output()
            .unwrap();
        remains = String::from_utf8_lossy(&state.stdout).contains(marker.to_str().unwrap());
        if remains {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    assert!(!remains, "descendant survived deadline");
    assert_eq!(fs::read(output).unwrap(), b"original");
}

/// 未公開の正常終了とsignalも、期限・停止を成功へ取り消す根拠にしない。
#[cfg(unix)]
#[test]
fn unpublished_exit_preserves_deadline_and_interruption() {
    use crate::{supervisor::unpublished_exit, worker::WorkerError};
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus, time::Duration};
    let timeout = Duration::from_millis(123);
    // 正常・異常・中断終了と、OSによるsignal終了を同じ期限契約で検査する。
    for raw in [0, 7 << 8, 130 << 8, 1, 2, 15, 9] {
        let status = ExitStatus::from_raw(raw);
        assert!(matches!(
            unpublished_exit(status, false, Some(timeout)),
            WorkerError::Timeout { timeout: actual } if actual == timeout
        ));
        for expired in [None, Some(timeout)] {
            assert!(matches!(
                unpublished_exit(status, true, expired),
                WorkerError::Interrupted
            ));
        }
    }
    let error = unpublished_exit(ExitStatus::from_raw(0), false, None);
    assert!(matches!(error, WorkerError::UnpublishedOutput));
    assert_eq!(
        error.to_string(),
        "supervisor exited successfully before output publication"
    );
    for raw in [130 << 8, 1, 2, 15] {
        assert!(matches!(
            unpublished_exit(ExitStatus::from_raw(raw), false, None),
            WorkerError::Interrupted
        ));
    }
    for raw in [7 << 8, 9] {
        assert!(matches!(
            unpublished_exit(ExitStatus::from_raw(raw), false, None),
            WorkerError::Failed { status } if status == ExitStatus::from_raw(raw)
        ));
    }
}
