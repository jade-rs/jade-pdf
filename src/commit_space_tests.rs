//! 実際の保存コピーをFIFOで停止し、監督死亡後の出力領域回収を固定する。
#![cfg(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "android",
    target_os = "ios",
    target_os = "freebsd"
))]
use crate::cli::Request;
use jade_pdf2docx_docx::ParseCancellation;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// Redやfixture起動失敗でも、所有する親と識別済みの監督群を停止するguard。
struct FixtureProcesses {
    /// 強制終了とwaitを所有するfixture親。
    parent: crate::worker::ChildGuard,
    /// 監督のPIDとcommand識別に使用する専用入力パス。
    input: PathBuf,
}

impl Drop for FixtureProcesses {
    /// 一意なfixtureパスをcommandに確認してから監督群を掃除し、親も回収する。
    fn drop(&mut self) {
        if let Ok(pid) = fs::read_to_string(self.input.with_extension("pid"))
            && supervisor_running(pid.trim(), &self.input)
        {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{}", pid.trim())])
                .status();
        }
    }
}

/// fixture固有のlogfile引数を照合し、別のPIDや終了済みの監督を除外する。
fn supervisor_running(pid: &str, input: &Path) -> bool {
    Command::new("ps")
        .args(["-p", pid, "-o", "command="])
        .output()
        .is_ok_and(|state| {
            let command = String::from_utf8_lossy(&state.stdout);
            command.contains("commit_space_tests::process_fixture")
                && command.contains(input.to_str().unwrap())
        })
}

/// 通常監督と同じ処理を実行する、子プロセス専用のfixture。
#[test]
fn process_fixture() {
    let Some(executable) = std::env::var_os("JADE_PDF2DOCX_TEST_COMMIT_EXECUTABLE") else {
        return;
    };
    let input =
        std::path::PathBuf::from(std::env::var_os("JADE_PDF2DOCX_TEST_COMMIT_INPUT").unwrap());
    let output =
        std::path::PathBuf::from(std::env::var_os("JADE_PDF2DOCX_TEST_COMMIT_OUTPUT").unwrap());
    let request = Request {
        input: input.clone(),
        input_sha256: None,
        publication_receipt: None,
        output,
        strict: false,
        debug: false,
        ocr: None,
        memory: None,
        timeout: std::env::var("JADE_PDF2DOCX_TEST_COMMIT_TIMEOUT_MS")
            .ok()
            .map(|value| Duration::from_millis(value.parse().unwrap()))
            .unwrap_or(Duration::from_secs(10)),
    };
    let cancellation = ParseCancellation::new();
    if std::env::var("JADE_PDF2DOCX_TEST_COMMIT_ROLE").is_ok_and(|role| role == "parent") {
        if std::env::var_os("JADE_PDF2DOCX_TEST_COMMIT_CANCEL_AFTER_PARTIAL").is_some() {
            let observed = cancellation.clone();
            let observed_input = input.clone();
            std::thread::spawn(move || {
                while !has_commit_partial(&observed_input) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                observed.cancel();
            });
        }
        let result = crate::supervisor::run(Path::new(&executable), &request, &cancellation);
        let outcome = match &result {
            Ok(()) => "success",
            Err(crate::worker::WorkerError::Timeout { .. }) => "timeout",
            Err(crate::worker::WorkerError::Interrupted) => "interrupted",
            Err(_) => "other failure",
        };
        fs::write(input.with_extension("result"), outcome).unwrap();
        if std::env::var_os("JADE_PDF2DOCX_TEST_CANCEL_AFTER_PUBLICATION").is_some() {
            assert!(
                cancellation.is_cancelled(),
                "publication cancellation was not injected"
            );
        }
        result.unwrap();
        return;
    }
    fs::write(input.with_extension("pid"), std::process::id().to_string()).unwrap();
    let _parent_lease = crate::supervisor::watch_parent(&cancellation).unwrap();
    let workspace = crate::supervisor::start_workspace(&cancellation).unwrap();
    fs::write(
        input.with_extension("workspace"),
        workspace
            .as_ref()
            .unwrap()
            .path()
            .as_os_str()
            .as_encoded_bytes(),
    )
    .unwrap();
    let observed = cancellation.clone();
    let observed_input = input.clone();
    std::thread::spawn(move || {
        while !observed.is_cancelled() {
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(observed_input.with_extension("cancelled"), b"").unwrap();
    });
    let result = crate::worker::run_conversion(Path::new(&executable), &request, &cancellation);
    // 通知待ちの停止は即時に戻るため、observer threadより先の終了も記録する。
    if cancellation.is_cancelled() {
        fs::write(input.with_extension("cancelled"), b"").unwrap();
    }
    result.unwrap();
}

/// 外側の通知fileを除外し、所有済みworkspace内の保存領域だけを観測する。
fn has_commit_partial(input: &Path) -> bool {
    let Ok(workspace) = fs::read_to_string(input.with_extension("workspace")) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(workspace) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(crate::workspace::COMMIT_PREFIX))
            && entry.path().is_dir()
            && has_partial(&entry.path())
    })
}

/// 保存用一時ファイルに書込済みかを、既存出力と所有markerを除外して調べる。
fn has_partial(directory: &Path) -> bool {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(error) => panic!("cannot inspect copy directory: {error}"),
    };
    entries.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            return has_partial(&path);
        }
        path.file_name()
            .is_some_and(|name| name != "output.docx" && name != crate::workspace::MARKER)
            && fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
    })
}

/// コピー途中の監督SIGKILLでも既存出力を保持し、出力directoryの残骸を回収する。
#[test]
fn killed_supervisor_during_commit_removes_partial_output() {
    for existing_output in [true, false] {
        assert_shutdown_during_commit(Shutdown::SupervisorDeath, existing_output);
    }
}

/// 親のSIGKILLでDropが実行されなくても、I/Oが戻れば監督が保存領域を回収する。
#[test]
fn killed_parent_during_commit_releases_output_after_io_returns() {
    for existing_output in [true, false] {
        assert_shutdown_during_commit(Shutdown::ParentDeath, existing_output);
    }
}

/// 所有領域を作成済みでも通知前に親が死亡した場合、監督が保存領域を回収する。
#[test]
fn killed_parent_before_commit_grant_recovers_allocated_directory() {
    for existing_output in [true, false] {
        assert_shutdown_during_commit(Shutdown::ParentDeathBeforeGrant, existing_output);
    }
}

/// 保存領域のmarker未完成でも、親死亡後に所有済み外側領域とともに回収する。
#[test]
fn killed_parent_before_commit_marker_recovers_created_directory() {
    for existing_output in [true, false] {
        assert_shutdown_during_commit(Shutdown::ParentDeathBeforeMarker, existing_output);
    }
}

/// worker終了後に保存I/Oが戻らなくても、親の期限で停止・回収する。
#[test]
fn deadline_stops_blocked_commit_and_recovers_owned_output() {
    for existing_output in [true, false] {
        assert_shutdown_during_commit(Shutdown::Deadline, existing_output);
    }
}

/// FIFO書き込み側が停止中でも、期限で群全体を停止しコピーと所有領域を回収する。
#[test]
fn deadline_stops_commit_with_suspended_fifo_writer() {
    for existing_output in [true, false] {
        assert_shutdown_during_commit(Shutdown::SuspendedWriterDeadline, existing_output);
    }
}

/// 協調停止が保存I/Oで進まない場合も、期限で回収し中断理由を維持する。
#[test]
fn cancelled_blocked_commit_is_reaped_at_deadline() {
    for existing_output in [true, false] {
        assert_shutdown_during_commit(Shutdown::CancelledDeadline, existing_output);
    }
}

/// 公開後に監督が終了しなくても、出力更新と親の成功結果を一致させる。
#[test]
fn published_output_retains_success_when_supervisor_cannot_exit() {
    for existing_output in [true, false] {
        assert_shutdown_during_commit(Shutdown::PublicationDeadline, existing_output);
    }
}

/// 親が最終置換を完了した直後の停止要求でも、公開済み出力と成功結果を維持する。
#[test]
fn published_output_wins_late_parent_cancellation() {
    for existing_output in [true, false] {
        assert_shutdown_during_commit(Shutdown::PublicationCancelled, existing_output);
    }
}

/// 保存を停止させるfixtureの終了原因。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shutdown {
    /// 監督を直接強制終了する。
    SupervisorDeath,
    /// 親を保存コピー中に強制終了する。
    ParentDeath,
    /// 親を所有marker完成後・grant前に強制終了する。
    ParentDeathBeforeGrant,
    /// 親をmkdir成功後・保存領域のmarker書込前に強制終了する。
    ParentDeathBeforeMarker,
    /// 保存I/Oを停止したまま変換の期限を待つ。
    Deadline,
    /// FIFO書き込み側をSIGSTOPしたまま変換の期限を待つ。
    SuspendedWriterDeadline,
    /// コピー途中の停止要求後もI/Oを戻さず、親の期限で回収する。
    CancelledDeadline,
    /// 保存完了直後の監督を停止させ、親の期限との競合を観測する。
    PublicationDeadline,
    /// 親の最終置換直後に停止要求を注入する。
    PublicationCancelled,
}

/// 本番の親・監督・保存関数を使用し、コピーと所有通知の死亡位置をIPCで指定する。
fn assert_shutdown_during_commit(shutdown: Shutdown, existing_output: bool) {
    use std::os::unix::fs::PermissionsExt;
    let kill_parent = matches!(
        shutdown,
        Shutdown::ParentDeath
            | Shutdown::ParentDeathBeforeGrant
            | Shutdown::ParentDeathBeforeMarker
    );
    let before_grant = matches!(
        shutdown,
        Shutdown::ParentDeathBeforeGrant | Shutdown::ParentDeathBeforeMarker
    );
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output_dir = base.path().join("destination");
    let output = output_dir.join("output.docx");
    fs::write(&input, b"input").unwrap();
    if existing_output {
        fs::create_dir(&output_dir).unwrap();
        fs::write(&output, b"existing").unwrap();
    }
    let publication = matches!(
        shutdown,
        Shutdown::PublicationDeadline | Shutdown::PublicationCancelled
    );
    let expected_entries = if publication {
        1
    } else {
        usize::from(existing_output)
    };
    let executable = base.path().join("converter");
    fs::write(&executable, br##"#!/bin/sh
if [ "$1" = --jade-pdf2docx-conversion-supervisor ]; then
  export JADE_PDF2DOCX_TEST_COMMIT_EXECUTABLE="$0" JADE_PDF2DOCX_TEST_COMMIT_INPUT="$2" JADE_PDF2DOCX_TEST_COMMIT_OUTPUT="$3"
  export JADE_PDF2DOCX_TEST_COMMIT_ROLE=supervisor
  exec "$JADE_PDF2DOCX_TEST_COMMIT_TEST_BINARY" --exact commit_space_tests::process_fixture --nocapture --logfile "$2.log"
fi
if [ -n "$JADE_PDF2DOCX_TEST_COMMIT_ALLOCATION_CHECKPOINT" ] || [ -n "$JADE_PDF2DOCX_TEST_PUBLICATION_CHECKPOINT" ] || [ -n "$JADE_PDF2DOCX_TEST_COMMIT_CREATION_CHECKPOINT" ]; then
  printf partial > "$3"
  exit 0
fi
mkfifo "$3" || exit 8
(/bin/sh -c 'printf partial; if [ -n "$JADE_PDF2DOCX_TEST_SUSPEND_FIFO_WRITER" ]; then kill -STOP $$; fi; while [ ! -f "$1.release" ]; do sleep 0.01; done' fixture "$2" > "$3") </dev/null >/dev/null 2>/dev/null &
exit 0
"##).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    // fixtureのwrapperだけが読む値をscriptへ固定し、test runner全体の環境は変更しない。
    let binary = std::env::current_exe().unwrap();
    let script = fs::read_to_string(&executable).unwrap().replace(
        "$JADE_PDF2DOCX_TEST_COMMIT_TEST_BINARY",
        &binary.to_string_lossy(),
    );
    fs::write(&executable, script).unwrap();
    let log_path = input.with_extension("fixture-log");
    let log = fs::File::create(&log_path).unwrap();
    let mut command = Command::new(binary);
    command
        .args([
            "--exact",
            "commit_space_tests::process_fixture",
            "--nocapture",
        ])
        .env("JADE_PDF2DOCX_TEST_COMMIT_EXECUTABLE", executable)
        .env("JADE_PDF2DOCX_TEST_COMMIT_INPUT", &input)
        .env("JADE_PDF2DOCX_TEST_COMMIT_OUTPUT", &output)
        .env("JADE_PDF2DOCX_TEST_COMMIT_ROLE", "parent")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    let checkpoint = input.with_extension("allocated");
    if before_grant {
        command.env(
            if shutdown == Shutdown::ParentDeathBeforeMarker {
                "JADE_PDF2DOCX_TEST_COMMIT_CREATION_CHECKPOINT"
            } else {
                "JADE_PDF2DOCX_TEST_COMMIT_ALLOCATION_CHECKPOINT"
            },
            &checkpoint,
        );
    }
    let deadline_shutdown = matches!(
        shutdown,
        Shutdown::Deadline
            | Shutdown::SuspendedWriterDeadline
            | Shutdown::CancelledDeadline
            | Shutdown::PublicationDeadline
            | Shutdown::PublicationCancelled
    );
    if publication {
        command.env(
            "JADE_PDF2DOCX_TEST_PUBLICATION_CHECKPOINT",
            input.with_extension("published"),
        );
    }
    if shutdown == Shutdown::PublicationCancelled {
        command.env("JADE_PDF2DOCX_TEST_CANCEL_AFTER_PUBLICATION", "1");
    }
    if deadline_shutdown {
        command.env("JADE_PDF2DOCX_TEST_COMMIT_TIMEOUT_MS", "2000");
    }
    if shutdown == Shutdown::CancelledDeadline {
        command.env("JADE_PDF2DOCX_TEST_COMMIT_CANCEL_AFTER_PARTIAL", "1");
    }
    if shutdown == Shutdown::SuspendedWriterDeadline {
        command.env("JADE_PDF2DOCX_TEST_SUSPEND_FIFO_WRITER", "1");
    }
    let mut processes = FixtureProcesses {
        parent: crate::worker::ChildGuard::spawn(&mut command).unwrap(),
        input: input.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let reached = || {
        if publication {
            fs::read(&output).is_ok_and(|bytes| bytes == b"partial")
        } else if before_grant {
            checkpoint.exists()
        } else {
            has_commit_partial(&input)
        }
    };
    while !reached() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let partial = reached();
    let supervisor = fs::read_to_string(input.with_extension("pid")).unwrap_or_else(|error| {
        let log = fs::read_to_string(&log_path).unwrap_or_default();
        panic!(
            "fixture supervisor did not start: {error}; {}",
            log.chars().take(4000).collect::<String>()
        );
    });
    let target = if kill_parent {
        processes.parent.child.id().to_string()
    } else {
        supervisor.trim().to_owned()
    };
    if !deadline_shutdown {
        assert!(
            Command::new("/bin/kill")
                .args(["-KILL", &target])
                .status()
                .unwrap()
                .success()
        );
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = loop {
        if let Some(status) = processes.parent.child.try_wait().unwrap() {
            processes.parent.reaped = true;
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut observed_cancellation = true;
    if kill_parent {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !input.with_extension("cancelled").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        observed_cancellation = input.with_extension("cancelled").exists();
        fs::write(base.path().join("input.pdf.release"), b"").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while fs::read_dir(&output_dir).unwrap().count() > expected_entries
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let remaining = fs::read_dir(&output_dir).unwrap().count();
    let deadline = Instant::now() + Duration::from_secs(5);
    while supervisor_running(supervisor.trim(), &input) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let running = supervisor_running(supervisor.trim(), &input);
    let workspace = PathBuf::from(fs::read_to_string(input.with_extension("workspace")).unwrap());
    let workspace_remaining = workspace.exists();
    let fixture_log = if result.is_none() {
        fs::read_to_string(&log_path)
            .unwrap_or_default()
            .chars()
            .take(4000)
            .collect::<String>()
    } else {
        String::new()
    };
    // Redの残留観測を保持したまま、guardでfixtureだけを停止する。
    drop(processes);
    if deadline_shutdown {
        assert!(
            result.is_some(),
            "conversion deadline did not stop blocked commit; supervisor_running={running}; workspace_remaining={workspace_remaining}; fixture_log={fixture_log}"
        );
        assert_eq!(
            fs::read_to_string(input.with_extension("result")).unwrap(),
            if publication {
                "success"
            } else if shutdown == Shutdown::CancelledDeadline {
                "interrupted"
            } else {
                "timeout"
            }
        );
    }
    assert!(!running, "supervisor remained active after cleanup");
    assert!(
        !workspace_remaining,
        "worker workspace survived process death"
    );
    assert!(
        partial,
        "fixture never reached the selected commit checkpoint"
    );
    assert_eq!(
        result.expect("fixture parent did not exit").success(),
        publication
    );
    assert!(
        observed_cancellation,
        "supervisor never observed parent lease closure"
    );
    if publication {
        assert_eq!(fs::read(output).unwrap(), b"partial");
    } else if existing_output {
        assert_eq!(fs::read(output).unwrap(), b"existing");
    } else {
        assert!(!output.exists(), "partial output was published");
    }
    assert_eq!(fs::read(&input).unwrap(), b"input");
    assert_eq!(
        remaining, expected_entries,
        "atomic output temporary file survived process death"
    );
}
