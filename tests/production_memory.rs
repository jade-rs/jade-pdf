//! 委譲された実cgroup内で通常CLIの公開・OOM・回収を検証する。
#![cfg(target_os = "linux")]
use std::{fs, io::Read, path::PathBuf, process::Command};
#[path = "production_memory/parent_death.rs"]
mod parent_death;

/// CIが用意する委譲先を必須とし、制限の検証を黙って省略しない。
fn parent() -> PathBuf {
    std::env::var_os("JADE_PDF2DOCX_TEST_CGROUP_PARENT")
        .expect("JADE_PDF2DOCX_TEST_CGROUP_PARENT required")
        .into()
}

/// 通常CLIも生存leaseを拒否し、lease解放後は指定した空群だけを回収する。
#[test]
#[ignore = "requires a delegated cgroup v2 memory controller"]
fn cleanup_memory_cgroup_requires_inactive_registered_group() {
    let parent = parent();
    let before = fs::read_dir(&parent).unwrap().count();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = parent.join(format!(
        "jade-pdf2docx-memory-v1-{}-{stamp}-0",
        std::process::id()
    ));
    fs::create_dir(&path).unwrap();
    let _owned = EmptyGroup(path.clone());
    let lease = fs::File::open(&path).unwrap();
    lease.lock().unwrap();
    for (control, value) in [
        ("memory.max", "67108864"),
        ("memory.swap.max", "0"),
        ("memory.oom.group", "1"),
        ("cgroup.max.depth", "0"),
    ] {
        fs::write(path.join(control), value).unwrap();
    }
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .arg("--cleanup-memory-cgroup")
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("owner is alive"),
        "{result:?}"
    );
    assert!(path.is_dir());
    drop(lease);
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .arg("--cleanup-memory-cgroup")
        .arg(&path)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert!(!path.exists());
    assert_eq!(fs::read_dir(parent).unwrap().count(), before);
}

/// テスト自身が作成した空群だけを、assertion失敗時も残さない。
struct EmptyGroup(
    /// このテストだけが作成した群の絶対パス。
    PathBuf,
);

impl Drop for EmptyGroup {
    /// cgroupのcontrol fileへ再帰削除を使わない。
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}
