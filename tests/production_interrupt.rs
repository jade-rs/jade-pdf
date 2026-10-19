#![cfg(unix)]
//! 親・process groupへの停止signalでworkerの回収と一時領域の破棄を検証する。
use std::{
    fs,
    io::Write,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// 失敗するRedでもテスト所有の親・監督・workerを残さないguard。
struct CliProcesses {
    /// 起動したCLIの親。
    parent: Option<Child>,
    /// 観測したworkerのPID。
    worker: Option<String>,
    /// 一時領域とworkerを所有する監督のPID。
    supervisor: Option<String>,
    /// worker識別に使うテスト専用入力パス。
    input: PathBuf,
}

/// PIDが現在もテストのworkerを指すかを確認する。
fn owns_worker(pid: &str, input: &Path) -> bool {
    owns_process(pid, input, "--jade-pdf2docx-conversion-worker")
}

/// PID再利用を避け、テスト固有入力を持つ内部プロセスだけを識別する。
fn owns_process(pid: &str, input: &Path, flag: &str) -> bool {
    let result = Command::new("ps")
        .args(["-p", pid, "-o", "command="])
        .output()
        .unwrap();
    let command = String::from_utf8_lossy(&result.stdout);
    command.contains(flag) && command.contains(input.to_str().unwrap())
}

impl CliProcesses {
    /// Redで残ったテスト所有のworker・監督だけを強制終了する。
    fn stop_leftovers(&self) {
        for (pid, flag) in [
            (&self.worker, "--jade-pdf2docx-conversion-worker"),
            (&self.supervisor, "--jade-pdf2docx-conversion-supervisor"),
        ] {
            if let Some(pid) = pid
                && owns_process(pid, &self.input, flag)
            {
                let _ = Command::new("kill")
                    .args(["-KILL", pid])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }
}

impl Drop for CliProcesses {
    /// assertionや監視の失敗でも、自分で起動したプロセスを回収する。
    fn drop(&mut self) {
        self.stop_leftovers();
        if let Some(parent) = &mut self.parent {
            let _ = parent.kill();
            let _ = parent.wait();
        }
    }
}

/// 指定した親の直接childからテスト固有入力を持つ内部プロセスを探す。
fn find_process(parent: u32, input: &Path, flag: &str) -> Option<String> {
    let result = Command::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .output()
        .unwrap();
    String::from_utf8_lossy(&result.stdout)
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?;
            let ppid: u32 = fields.next()?.parse().ok()?;
            (ppid == parent && line.contains(flag) && line.contains(input.to_str().unwrap()))
                .then(|| pid.to_owned())
        })
}

/// SIGINTで停止中のworkerを回収し、既存出力または未作成の出力親を保持する。
#[test]
fn parent_sigint_reaps_worker_and_removes_staging() {
    assert_interrupt_cleanup(false, "-INT");
}

/// 端末のCtrl+Cと同じprocess groupへのSIGINTでも親が回収する。
#[test]
fn process_group_sigint_reaps_worker_and_removes_staging() {
    assert_interrupt_cleanup(true, "-INT");
}

/// サービス停止のSIGTERMを親が受けてworkerと一時領域を回収する。
#[test]
fn parent_sigterm_reaps_worker_and_removes_staging() {
    assert_interrupt_cleanup(false, "-TERM");
}

/// process group全体へのSIGTERMでも終了分類と回収が一致する。
#[test]
fn process_group_sigterm_reaps_worker_and_removes_staging() {
    assert_interrupt_cleanup(true, "-TERM");
}

/// セッション終了のSIGHUPを親が受けてworkerと一時領域を回収する。
#[test]
fn parent_sighup_reaps_worker_and_removes_staging() {
    assert_interrupt_cleanup(false, "-HUP");
}

/// process group全体へのSIGHUPでも終了分類と回収が一致する。
#[test]
fn process_group_sighup_reaps_worker_and_removes_staging() {
    assert_interrupt_cleanup(true, "-HUP");
}

/// nohupでSIGHUP無視を継承しても起動でき、SIGTERMで回収できる。
#[test]
fn nohup_parent_sigterm_reaps_worker_and_removes_staging() {
    assert_interrupt_cleanup_with_launcher(false, "-TERM", Some("nohup"), false);
}

/// 親が生存したまま監督だけが強制終了してもworkerと一時領域を回収する。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
#[test]
fn killed_supervisor_does_not_leave_worker_or_staging() {
    assert_interrupt_cleanup_with_launcher(false, "-KILL", None, true);
}

/// 呼び出し側の群を強制終了しても、別群の監督が親切断から回収する。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
#[test]
fn killed_parent_group_does_not_leave_worker_or_staging() {
    assert_interrupt_cleanup(true, "-KILL");
}

/// 監督とworkerの群を強制終了しても、生きた親が一時領域を回収する。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
#[test]
fn killed_supervisor_group_does_not_leave_staging() {
    assert_interrupt_cleanup_with_launcher(true, "-KILL", None, true);
}

/// 親がSIGKILLでDropを実行できなくてもworkerと一時領域を回収する。
#[test]
fn killed_parent_does_not_leave_worker_or_staging() {
    assert_interrupt_cleanup(false, "-KILL");
}

/// 欠落・不正な初期通信は開始せず、初期通信直後の親切断も副作用を残さない。
#[test]
fn supervisor_without_valid_parent_greeting_has_no_side_effects() {
    for (greeting, expected) in [
        (&b""[..], 1),
        (&b"JADE_PDF2DOCX"[..], 1),
        (&b"BAD-LEASE-1\n"[..], 1),
        (&b"JADE-PDF2DOCX-LEASE-1\n"[..], 130),
    ] {
        let base = tempfile::tempdir().unwrap();
        let input = base.path().join("blocked.pdf");
        let output = base.path().join("new/output.docx");
        assert!(
            Command::new("mkfifo")
                .arg(&input)
                .status()
                .unwrap()
                .success()
        );
        let parent = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
            .arg("--jade-pdf2docx-conversion-supervisor")
            .args([input.as_os_str(), output.as_os_str()])
            .env("TMPDIR", base.path())
            .env("TMP", base.path())
            .env("TEMP", base.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = parent.id();
        let mut processes = CliProcesses {
            parent: Some(parent),
            worker: None,
            supervisor: None,
            input: input.clone(),
        };
        processes
            .parent
            .as_mut()
            .unwrap()
            .stdin
            .take()
            .unwrap()
            .write_all(greeting)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            processes.worker = find_process(pid, &input, "--jade-pdf2docx-conversion-worker");
            if let Some(status) = processes.parent.as_mut().unwrap().try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.and_then(|status| status.code()), Some(expected));
        assert!(!output.parent().unwrap().exists());
        assert_eq!(fs::read_dir(base.path()).unwrap().count(), 1);
    }
}

/// 親のみとprocess groupの両方で同じ出力保持・回収条件を確認する。
fn assert_interrupt_cleanup(group: bool, signal: &str) {
    assert_interrupt_cleanup_with_launcher(group, signal, None, false);
}

/// signal設定を継承するlauncherの有無も含めて停止と回収を検証する。
fn assert_interrupt_cleanup_with_launcher(
    group: bool,
    signal: &str,
    launcher: Option<&str>,
    kill_supervisor: bool,
) {
    for existing in [false, true] {
        let base = tempfile::tempdir().unwrap();
        let input = base.path().join("blocked.pdf");
        let output = if existing {
            base.path().join("output.docx")
        } else {
            base.path().join("new/output.docx")
        };
        assert!(
            Command::new("mkfifo")
                .arg(&input)
                .status()
                .unwrap()
                .success()
        );
        if existing {
            fs::write(&output, b"existing output").unwrap();
        }
        let mut command = if let Some(launcher) = launcher {
            let mut command = Command::new(launcher);
            command.arg(env!("CARGO_BIN_EXE_jade-pdf2docx"));
            command
        } else {
            Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        };
        if group {
            command.process_group(0);
        }
        let parent = command
            .args([input.as_os_str(), output.as_os_str()])
            .args(["--timeout-ms", "300000"])
            .env("TMPDIR", base.path())
            .env("TMP", base.path())
            .env("TEMP", base.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = parent.id();
        let mut processes = CliProcesses {
            parent: Some(parent),
            worker: None,
            supervisor: None,
            input: input.clone(),
        };
        let ready_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            processes.supervisor =
                find_process(pid, &input, "--jade-pdf2docx-conversion-supervisor");
            processes.worker = processes.supervisor.as_ref().and_then(|supervisor| {
                find_process(
                    supervisor.parse().unwrap(),
                    &input,
                    "--jade-pdf2docx-conversion-worker",
                )
            });
            if processes.worker.is_some() {
                break;
            }
            if processes
                .parent
                .as_mut()
                .unwrap()
                .try_wait()
                .unwrap()
                .is_some()
            {
                let result = processes.parent.take().unwrap().wait_with_output().unwrap();
                panic!(
                    "CLI exited before worker start: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }
            assert!(Instant::now() < ready_deadline, "worker did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let signal_target = if kill_supervisor {
            let supervisor = processes.supervisor.as_ref().unwrap();
            if group {
                format!("-{supervisor}")
            } else {
                supervisor.clone()
            }
        } else if group {
            format!("-{pid}")
        } else {
            pid.to_string()
        };
        assert!(
            Command::new("kill")
                .args([signal, "--", &signal_target])
                .status()
                .unwrap()
                .success()
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = processes.parent.as_mut().unwrap().try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        if signal == "-KILL" {
            let recovery_deadline = Instant::now() + Duration::from_secs(5);
            while owns_worker(processes.worker.as_ref().unwrap(), &input)
                || owns_process(
                    processes.supervisor.as_ref().unwrap(),
                    &input,
                    "--jade-pdf2docx-conversion-supervisor",
                )
                || fs::read_dir(base.path()).unwrap().count() != if existing { 2 } else { 1 }
            {
                if Instant::now() >= recovery_deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let worker_remains = owns_worker(processes.worker.as_ref().unwrap(), &input);
        let supervisor_remains = owns_process(
            processes.supervisor.as_ref().unwrap(),
            &input,
            "--jade-pdf2docx-conversion-supervisor",
        );
        let entries = fs::read_dir(base.path()).unwrap().count();
        let preserved = if existing {
            fs::read(&output).unwrap() == b"existing output"
        } else {
            !output.parent().unwrap().exists()
        };
        // 観測後にRedの残留workerを終了し、継承されたstderr pipeも閉じる。
        processes.stop_leftovers();
        let mut parent = processes.parent.take().unwrap();
        if status.is_none() {
            parent.kill().unwrap();
        }
        let result = parent.wait_with_output().unwrap();
        if signal == "-KILL" && !kill_supervisor {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(result.status.signal(), Some(9));
        } else if kill_supervisor {
            assert_eq!(result.status.code(), Some(1));
            assert!(String::from_utf8_lossy(&result.stderr).contains("conversion failed"));
        } else {
            assert_eq!(
                result.status.code(),
                Some(130),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(String::from_utf8_lossy(&result.stderr).contains("conversion interrupted"));
        }
        assert!(result.stdout.is_empty());
        assert!(!worker_remains, "worker survived {signal}");
        assert!(!supervisor_remains, "supervisor survived {signal}");
        assert!(preserved);
        assert_eq!(
            entries,
            if existing { 2 } else { 1 },
            "staging files survived {signal}"
        );
        assert!(input.exists());
    }
}
