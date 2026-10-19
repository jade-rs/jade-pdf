//! 自動・明示回収の途中でSIGKILLしても所有markerから回収を再開できる回帰。
use crate::{worker::ChildGuard, workspace::Workspace};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

/// 回収開始後のcheckpointで、親からのSIGKILLを待つfixture。
#[test]
fn cleanup_fixture() {
    let Some(path) = std::env::var_os("JADE_PDF2DOCX_TEST_REMOVAL_WORKSPACE") else {
        return;
    };
    let path = PathBuf::from(path);
    let mut workspace = Workspace::create_registered(&path, || Ok(())).unwrap();
    for name in ["first", "second", "third"] {
        fs::write(path.join(name), b"partial").unwrap();
    }
    if std::env::var_os("JADE_PDF2DOCX_TEST_REMOVAL_EXPLICIT").is_some() {
        if let Workspace::Recovery { lease, .. } = &mut workspace {
            drop(lease.take());
        }
        super::cleanup(&path).unwrap();
    } else {
        drop(workspace);
    }
}

/// 回収者が作業fileを一つ消した直後に死亡しても、残存fileとmarkerを回収できる。
#[test]
fn sigkill_during_automatic_and_explicit_cleanup_is_recoverable() {
    let base = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(base.path()).unwrap();
    for explicit in [false, true] {
        let path = root.join(format!("jade-pdf2docx-supervised-removal-{explicit}"));
        let ready = root.join(format!("ready-{explicit}"));
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "workspace_cleanup::removal_process_tests::cleanup_fixture",
            ])
            .env("JADE_PDF2DOCX_TEST_REMOVAL_WORKSPACE", &path)
            .env("JADE_PDF2DOCX_TEST_REMOVAL_CHECKPOINT", &ready)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if explicit {
            command.env("JADE_PDF2DOCX_TEST_REMOVAL_EXPLICIT", "1");
        }
        let mut child = ChildGuard::spawn(&mut command).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            ready.exists(),
            "cleanup fixture did not reach payload removal"
        );
        assert!(Workspace::cleanup_marker(&path).is_ok());
        assert_eq!(fs::read_dir(&path).unwrap().count(), 3);
        child.child.kill().unwrap();
        child.child.wait().unwrap();
        child.reaped = true;
        super::cleanup(&path).unwrap();
        assert!(!path.exists());
    }
}
