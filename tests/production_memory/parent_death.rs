//! 通常CLIの親死亡から残骸回収・実PDF再変換までを実プロセスで検証する。
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// テストが直接起動した子を失敗時にも停止し、zombieを残さない。
struct Process(
    /// このテストがwaitする子。
    Child,
);

impl Drop for Process {
    /// 正常終了済みでもwaitし、残存時はkillする。
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// このテストが観測した専用群と領域だけを失敗時も回収する。
struct Remains {
    /// 親CLIが作成したcgroup。
    group: PathBuf,
    /// 同じ変換の登録済みworkspace。
    workspace: PathBuf,
}

impl Drop for Remains {
    /// Redでも子孫を停止してから通常CLIで回収を試みる。
    fn drop(&mut self) {
        if self.group.exists() {
            let _ = fs::write(self.group.join("cgroup.kill"), b"1");
            wait_until(|| !populated(&self.group));
            let _ = cleanup("--cleanup-memory-cgroup", &self.group);
        }
        if self.workspace.exists() {
            let _ = cleanup("--cleanup-workspace", &self.workspace);
        }
    }
}

/// boundedな待機により、準備通知・停止の失敗をテスト結果にする。
fn wait_until(mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

/// 元cgroupのカーネル通知だけで子孫の終了を確認する。
fn populated(group: &Path) -> bool {
    fs::read_to_string(group.join("cgroup.events"))
        .is_ok_and(|events| events.lines().any(|line| line == "populated 1"))
}

/// 回収APIで代用せず、配布する通常CLIの入口を使う。
fn cleanup(flag: &str, path: &Path) -> std::io::Result<std::process::Output> {
    Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .arg(flag)
        .arg(path)
        .output()
}

/// workspaceの共有leaseを保持し、atomic通知後も生存し続ける離脱子孫。
#[test]
#[ignore = "internal subprocess fixture"]
fn detached_fixture() {
    let Some(workspace) = std::env::var_os("JADE_PDF2DOCX_DEATH_TEST_WORKSPACE") else {
        return;
    };
    let marker =
        fs::File::open(Path::new(&workspace).join(".jade-pdf2docx-workspace-owner")).unwrap();
    marker.lock_shared().unwrap();
    let ready = PathBuf::from(std::env::var_os("JADE_PDF2DOCX_DEATH_TEST_READY").unwrap());
    fs::write(ready.with_extension("pending"), b"ready").unwrap();
    fs::rename(ready.with_extension("pending"), ready).unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
