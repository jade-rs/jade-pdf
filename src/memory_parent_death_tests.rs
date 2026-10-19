//! 親SIGKILL後も、process groupを離れた変換子孫をカーネルの群で停止する。
use crate::{
    cli::{MemoryOptions, Request},
    worker::ChildGuard,
};
use jade_pdf2docx_docx::ParseCancellation;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// このテストだけが作成した群と領域を、Redの失敗時にも残さない。
struct Remains {
    /// 親が作成した一つのcgroup。
    group: PathBuf,
    /// 通知を受けた所有workspace。
    workspace: PathBuf,
}

impl Drop for Remains {
    /// 所属子孫を停止してから空群と登録済み領域を回収する。
    fn drop(&mut self) {
        if self.group.exists() {
            let _ = fs::write(self.group.join("cgroup.kill"), b"1");
            let deadline = Instant::now() + Duration::from_secs(3);
            while fs::read_to_string(self.group.join("cgroup.events"))
                .is_ok_and(|events| events.lines().any(|line| line == "populated 1"))
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = crate::memory_group::cleanup(&self.group);
        }
        let _ = crate::workspace_cleanup::cleanup(&self.workspace);
    }
}

/// cgroup自身の全子孫が停止したかをカーネルに照会する。
fn populated(path: &std::path::Path) -> bool {
    fs::read_to_string(path.join("cgroup.events"))
        .unwrap()
        .lines()
        .any(|line| line == "populated 1")
}

/// 実親を失った監督が、setsidした子孫も停止し、入力/既存出力を保持する。
#[test]
#[ignore = "requires a delegated cgroup v2 memory controller"]
fn memory_parent_sigkill_stops_detached_descendant() {
    use std::os::unix::fs::PermissionsExt;
    let parent = PathBuf::from(std::env::var_os("JADE_PDF2DOCX_TEST_CGROUP_PARENT").unwrap());
    let before = fs::read_dir(&parent).unwrap().count();
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("input.pdf");
    let output = base.path().join("output.docx");
    let ready = base.path().join("ready");
    let wrapper = base.path().join("supervisor");
    fs::write(&input, b"original input").unwrap();
    fs::write(&output, b"existing output").unwrap();
    fs::write(&wrapper, b"#!/bin/sh\nexec \"$JADE_PDF2DOCX_PARENT_TEST_EXE\" --ignored --exact memory_parent_death_tests::memory_supervisor_fixture --nocapture\n").unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "memory_parent_death_tests::memory_parent_fixture",
            "--nocapture",
        ])
        .env(
            "JADE_PDF2DOCX_PARENT_TEST_EXE",
            std::env::current_exe().unwrap(),
        )
        .env("JADE_PDF2DOCX_PARENT_TEST_WRAPPER", &wrapper)
        .env("JADE_PDF2DOCX_PARENT_TEST_INPUT", &input)
        .env("JADE_PDF2DOCX_PARENT_TEST_OUTPUT", &output)
        .env("JADE_PDF2DOCX_PARENT_TEST_READY", &ready);
    let mut owner = ChildGuard::spawn(&mut command).unwrap();
    let prefix = format!("jade-pdf2docx-memory-v1-{}-", owner.child.id());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        assert!(
            owner.child.try_wait().unwrap().is_none(),
            "parent fixture failed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let workspace = PathBuf::from(fs::read_to_string(&ready).expect("descendant not ready"));
    let group = fs::read_dir(&parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&prefix)
        })
        .unwrap();
    let remains = Remains { group, workspace };
    assert!(populated(&remains.group));
    owner.child.kill().unwrap();
    owner.child.wait().unwrap();
    owner.reaped = true;
    let deadline = Instant::now() + Duration::from_secs(3);
    while populated(&remains.group) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !populated(&remains.group),
        "detached descendant survived parent SIGKILL"
    );
    if remains.workspace.exists() {
        crate::workspace_cleanup::cleanup(&remains.workspace).unwrap();
    }
    crate::memory_group::cleanup(&remains.group).unwrap();
    assert_eq!(fs::read(&input).unwrap(), b"original input");
    assert_eq!(fs::read(&output).unwrap(), b"existing output");
    assert!(!remains.workspace.exists());
    assert_eq!(fs::read_dir(parent).unwrap().count(), before);
}

/// 本番の親監督ループを実行し、外側のテストからSIGKILLするfixture。
#[test]
#[ignore = "internal subprocess fixture"]
fn memory_parent_fixture() {
    let Some(wrapper) = std::env::var_os("JADE_PDF2DOCX_PARENT_TEST_WRAPPER") else {
        return;
    };
    let request = Request {
        input: std::env::var_os("JADE_PDF2DOCX_PARENT_TEST_INPUT")
            .unwrap()
            .into(),
        input_sha256: None,
        publication_receipt: None,
        output: std::env::var_os("JADE_PDF2DOCX_PARENT_TEST_OUTPUT")
            .unwrap()
            .into(),
        strict: false,
        debug: false,
        ocr: None,
        timeout: Duration::from_secs(20),
        memory: Some(MemoryOptions {
            bytes: 128 * 1024 * 1024,
            parent: std::env::var_os("JADE_PDF2DOCX_TEST_CGROUP_PARENT")
                .unwrap()
                .into(),
        }),
    };
    let _ = crate::supervisor::run(
        std::path::Path::new(&wrapper),
        &request,
        &ParseCancellation::new(),
    );
}

/// 本番のleaseとworkspace初期化後に、専用process groupを離れる子を作る。
#[test]
#[ignore = "internal subprocess fixture"]
fn memory_supervisor_fixture() {
    use std::os::unix::process::CommandExt;
    if std::env::var_os("JADE_PDF2DOCX_PARENT_TEST_WRAPPER").is_none() {
        return;
    }
    let cancelled = ParseCancellation::new();
    let _lease = crate::supervisor::watch_parent(&cancelled).unwrap();
    let _workspace = crate::supervisor::start_workspace(&cancelled).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "memory_parent_death_tests::memory_detached_fixture",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: fork後のsetsidは参照もメモリ確保も行わない。
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    // 監督の通常ChildGuardでは所有しない、離脱した外部ツールの子孫を再現する。
    let _detached = command.spawn().unwrap();
    while !cancelled.is_cancelled() {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 生存leaseを保持してsetsid後の準備完了をatomicに通知する。
#[test]
#[ignore = "internal subprocess fixture"]
fn memory_detached_fixture() {
    let Some(ready) = std::env::var_os("JADE_PDF2DOCX_PARENT_TEST_READY") else {
        return;
    };
    let workspace = PathBuf::from(std::env::var_os(crate::workspace::ENV).unwrap());
    let _lease = crate::workspace::worker_lease(Some(&workspace)).unwrap();
    let ready = PathBuf::from(ready);
    let staged = ready.with_extension("pending");
    fs::write(&staged, workspace.as_os_str().as_encoded_bytes()).unwrap();
    fs::rename(staged, ready).unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// 正しく登録された同一uidの群でも、自分が所属しなければ停止権を取得しない。
#[test]
#[ignore = "requires a delegated cgroup v2 memory controller"]
fn memory_parent_guard_rejects_unrelated_registered_group() {
    let parent = PathBuf::from(std::env::var_os("JADE_PDF2DOCX_TEST_CGROUP_PARENT").unwrap());
    let group = crate::memory_group::MemoryGroup::new(&parent, 128 * 1024 * 1024).unwrap();
    let path = fs::canonicalize(group.path()).unwrap();
    assert!(crate::memory_group::ParentDeath::open(&path).is_err());
    assert!(path.is_dir());
}
