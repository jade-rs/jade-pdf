//! 所有markerと排他leaseを検査し、明示パスの変換残骸を回収する。
#[cfg(unix)]
use crate::workspace::Workspace;
#[cfg(unix)]
use std::fs;
use std::{io, path::Path};

/// 生存する親/監督を妨げず、現行protocolの完全な所有証拠のある領域を削除する。
pub(crate) fn cleanup(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        Candidate::open(path)?.remove()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "workspace cleanup requires Unix file identity",
        ))
    }
}

/// 削除前にdirectoryとmarkerのinodeを再確認する回収候補。
#[cfg(unix)]
struct Candidate<'a> {
    /// 明示された絶対パス。
    path: &'a Path,
    /// 開く前に取得したdirectoryの同一性。
    directory: fs::Metadata,
    /// 完成markerと、その排他leaseの保持先。
    marker: fs::File,
    /// 開いた時点のmarker名。公開renameも候補の変化として拒否する。
    marker_name: &'static str,
}

#[cfg(unix)]
impl<'a> Candidate<'a> {
    /// directoryを照合して、現行版の完成markerを上限付きで開く。
    fn open(path: &'a Path) -> io::Result<Self> {
        let directory = fs::symlink_metadata(path)?;
        let (marker, marker_name) = Workspace::cleanup_marker(path)?;
        Ok(Self {
            path,
            directory,
            marker,
            marker_name,
        })
    }

    /// 終了直後のlease解放を最大1秒待ち、内容とinodeを再検証して回収する。
    fn remove(self) -> io::Result<()> {
        let started = std::time::Instant::now();
        let grace = std::time::Duration::from_secs(1);
        loop {
            match self.marker.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) => {
                    let remaining = grace.saturating_sub(started.elapsed());
                    if remaining.is_zero() {
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "workspace is in use",
                        ));
                    }
                    std::thread::sleep(remaining.min(std::time::Duration::from_millis(10)));
                }
                Err(fs::TryLockError::Error(error)) => return Err(error),
            }
        }
        let directory = fs::symlink_metadata(self.path)?;
        if !directory.is_dir() || !same_file(&directory, &self.directory) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "workspace changed during cleanup",
            ));
        }
        let (marker, marker_name) = Workspace::cleanup_marker(self.path)?;
        if marker_name != self.marker_name
            || !same_file(&marker.metadata()?, &self.marker.metadata()?)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "workspace changed during cleanup",
            ));
        }
        crate::workspace_removal::remove(self.path, self.marker_name)
    }
}

/// platformのdevice/inodeで、同じfileのdescriptorかを照合する。
#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(all(test, unix))]
#[path = "workspace_pending_tests.rs"]
mod pending_tests;

#[cfg(all(test, unix))]
#[path = "workspace_removal_process_tests.rs"]
mod removal_process_tests;

#[cfg(all(test, unix))]
#[path = "workspace_cleanup_wait_tests.rs"]
mod wait_tests;

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::worker::ChildGuard;
    use std::{
        process::Command,
        time::{Duration, Instant},
    };

    /// 作成者またはworkerの生存leaseを実プロセスで保持する。
    #[test]
    fn lease_fixture() {
        let Some(path) = std::env::var_os("JADE_PDF2DOCX_TEST_CLEANUP_PATH") else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        let role = std::env::var("JADE_PDF2DOCX_TEST_CLEANUP_ROLE").unwrap();
        let _workspace = match role.as_str() {
            "creator" | "creator-pending" => {
                Some(Workspace::create_registered(&path, || Ok(())).unwrap())
            }
            "parent" => Some(Workspace::recover_registered(&path).unwrap()),
            _ => None,
        };
        let _lease = if _workspace.is_some() {
            None
        } else {
            crate::workspace::worker_lease(Some(&path)).unwrap()
        };
        std::fs::write(
            std::env::var_os("JADE_PDF2DOCX_TEST_CLEANUP_READY").unwrap(),
            b"ready",
        )
        .unwrap();
        loop {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// fixtureを起動し、共有lease取得の通知まで有限時間で待つ。
    pub(super) fn holder(path: &Path, ready: &Path, role: &str) -> ChildGuard {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "workspace_cleanup::tests::lease_fixture",
                "--nocapture",
            ])
            .env("JADE_PDF2DOCX_TEST_CLEANUP_PATH", path)
            .env("JADE_PDF2DOCX_TEST_CLEANUP_READY", ready)
            .env("JADE_PDF2DOCX_TEST_CLEANUP_ROLE", role)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if role == "creator-pending" {
            command.env("JADE_PDF2DOCX_TEST_WORKSPACE_MARKER_CHECKPOINT", ready);
        }
        let child = ChildGuard::spawn(&mut command).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists(), "lease fixture failed to start");
        child
    }

    /// 監督・親・workerの各生存leaseで拒否し、全員SIGKILL後だけ回収する。
    #[test]
    fn creator_and_worker_death_release_cleanup_lease() {
        let base = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(base.path()).unwrap();
        let path = root.join("jade-pdf2docx-supervised-dead-processes");
        let mut creator = holder(&path, &root.join("creator-ready"), "creator");
        fs::write(path.join("partial"), b"partial").unwrap();
        assert_eq!(
            cleanup(&path).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let mut parent = holder(&path, &root.join("parent-ready"), "parent");
        creator.child.kill().unwrap();
        creator.child.wait().unwrap();
        creator.reaped = true;
        assert_eq!(
            cleanup(&path).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let mut worker = holder(&path, &root.join("worker-ready"), "worker");
        parent.child.kill().unwrap();
        parent.child.wait().unwrap();
        parent.reaped = true;
        assert_eq!(
            cleanup(&path).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(fs::read(path.join("partial")).unwrap(), b"partial");
        worker.child.kill().unwrap();
        worker.child.wait().unwrap();
        worker.reaped = true;
        cleanup(&path).unwrap();
        assert!(!path.exists());
    }

    /// workerは親の回収guardと異なり、終了時にdirectoryを削除しない。
    #[test]
    fn worker_lease_is_non_destructive_and_optional() {
        assert!(crate::workspace::worker_lease(None).unwrap().is_none());
        let base = tempfile::tempdir().unwrap();
        let path = fs::canonicalize(base.path())
            .unwrap()
            .join("jade-pdf2docx-supervised-worker");
        let workspace = Workspace::create_registered(&path, || Ok(())).unwrap();
        let lease = crate::workspace::worker_lease(Some(&path)).unwrap();
        drop(lease);
        assert!(path.is_dir());
        assert_eq!(
            cleanup(&path).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(workspace);
        assert!(!path.exists());
    }

    /// 回収候補取得後にdirectoryが入れ替わっても、新しいforeign領域を削除しない。
    #[cfg(unix)]
    #[test]
    fn cleanup_rejects_replaced_directory_and_marker() {
        let base = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(base.path()).unwrap();
        let path = root.join("jade-pdf2docx-supervised-replaced");
        for directory_replaced in [true, false] {
            let mut creator = holder(
                &path,
                &root.join(if directory_replaced {
                    "first-ready"
                } else {
                    "second-ready"
                }),
                "creator",
            );
            creator.child.kill().unwrap();
            creator.child.wait().unwrap();
            creator.reaped = true;
            let candidate = Candidate::open(&path).unwrap();
            let kept = root.join("kept");
            if directory_replaced {
                fs::rename(&path, &kept).unwrap();
                fs::create_dir(&path).unwrap();
            } else {
                fs::rename(path.join(crate::workspace::MARKER), &kept).unwrap();
                fs::write(path.join(crate::workspace::MARKER), b"replacement").unwrap();
            }
            fs::write(path.join("keep"), b"foreign").unwrap();
            assert_eq!(
                candidate.remove().unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(fs::read(path.join("keep")).unwrap(), b"foreign");
            fs::remove_dir_all(&path).unwrap();
            if directory_replaced {
                fs::remove_dir_all(&kept).unwrap();
            } else {
                fs::remove_file(&kept).unwrap();
            }
        }
    }
}
