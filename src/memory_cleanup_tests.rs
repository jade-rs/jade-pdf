//! 明示した空cgroupだけを、生存leaseと登録情報で判定して回収する。
use crate::memory_group;

/// 普通のdirectoryやsymlinkを回収対象と認めず、内容を保持する。
#[test]
fn memory_cleanup_rejects_ordinary_directory() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("jade-pdf2docx-memory-v1-1-2-3");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep"), b"foreign").unwrap();
    assert!(memory_group::cleanup(&path).is_err());
    assert_eq!(std::fs::read(path.join("keep")).unwrap(), b"foreign");
    #[cfg(unix)]
    {
        let link = base.path().join("jade-pdf2docx-memory-v1-4-5-6");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(memory_group::cleanup(&link).is_err());
        assert!(link.is_symlink());
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        fs, io,
        path::PathBuf,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    /// fixtureと、その終了後に残った自分の群だけをテスト終了時に回収する。
    struct Owner {
        /// 生存leaseを保持するfixture。
        child: Child,
        /// fixtureが作成した専用cgroup。
        path: PathBuf,
    }

    impl Drop for Owner {
        /// assertion失敗時もfixtureと所属子孫を残さない。
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            if self.path.is_dir() {
                let _ = fs::write(self.path.join("cgroup.kill"), b"1");
                let deadline = Instant::now() + Duration::from_secs(3);
                while self.path.exists() && Instant::now() < deadline {
                    if memory_group::cleanup(&self.path).is_ok() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let _ = fs::remove_dir(&self.path);
            }
        }
    }

    /// 実カーネル検証の委譲先を必須にする。
    fn parent() -> PathBuf {
        std::env::var_os("JADE_PDF2DOCX_TEST_CGROUP_PARENT")
            .unwrap()
            .into()
    }

    /// 初期化完了まで有限時間で待ち、生存中の所有者を返す。
    fn owner(populated: bool) -> Owner {
        let base = tempfile::tempdir().unwrap();
        let ready = base.path().join("ready");
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "memory_cleanup_tests::linux::memory_cleanup_owner_fixture",
            ])
            .env("JADE_PDF2DOCX_MEMORY_CLEANUP_READY", &ready)
            .env(
                "JADE_PDF2DOCX_MEMORY_CLEANUP_POPULATED",
                if populated { "1" } else { "0" },
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut owner = Owner {
            child,
            path: PathBuf::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < deadline {
            assert!(owner.child.try_wait().unwrap().is_none(), "fixture failed");
            std::thread::sleep(Duration::from_millis(10));
        }
        owner.path = fs::read_to_string(&ready)
            .expect("fixture not ready")
            .into();
        owner
    }

    /// 起動直前の空群も保護し、所有者SIGKILL後にだけ明示回収できる。
    #[test]
    #[ignore = "requires a delegated cgroup v2 memory controller"]
    fn memory_cleanup_empty_owner_sigkill() {
        let mut owner = owner(false);
        assert_eq!(
            memory_group::cleanup(&owner.path).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(owner.path.exists());
        owner.child.kill().unwrap();
        owner.child.wait().unwrap();
        memory_group::cleanup(&owner.path).unwrap();
        assert!(!owner.path.exists());
    }

    /// 親のleaseが消えても、変換プロセスが残る群は停止せず回収を拒否する。
    #[test]
    #[ignore = "requires a delegated cgroup v2 memory controller"]
    fn memory_cleanup_populated_group_is_preserved() {
        let mut owner = owner(true);
        owner.child.kill().unwrap();
        owner.child.wait().unwrap();
        assert_eq!(
            memory_group::cleanup(&owner.path).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let events = fs::read_to_string(owner.path.join("cgroup.events")).unwrap();
        assert!(events.lines().any(|line| line == "populated 1"));
    }

    /// 名前だけ似た未登録群と旧版の群を回収対象にしない。
    #[test]
    #[ignore = "requires a delegated cgroup v2 memory controller"]
    fn memory_cleanup_foreign_and_unregistered_groups_are_preserved() {
        for name in [
            "foreign",
            "jade-pdf2docx-1-2-3",
            "jade-pdf2docx-memory-v1-1-2-3",
            "jade-pdf2docx-memory-v1-invalid",
        ] {
            let path = parent().join(name);
            fs::create_dir(&path).unwrap();
            assert!(memory_group::cleanup(&path).is_err(), "{name}");
            assert!(path.is_dir());
            fs::remove_dir(path).unwrap();
        }
    }

    /// 登録後でも必須設定が変更された群を、推測して回収しない。
    #[test]
    #[ignore = "requires a delegated cgroup v2 memory controller"]
    fn memory_cleanup_changed_registration_is_preserved() {
        for (control, value) in [
            ("memory.max", "max"),
            ("memory.swap.max", "max"),
            ("memory.oom.group", "0"),
            ("cgroup.max.depth", "max"),
        ] {
            let mut owner = owner(false);
            owner.child.kill().unwrap();
            owner.child.wait().unwrap();
            let original = fs::read(owner.path.join(control)).unwrap();
            fs::write(owner.path.join(control), value).unwrap();
            assert!(memory_group::cleanup(&owner.path).is_err(), "{control}");
            assert!(owner.path.is_dir());
            fs::write(owner.path.join(control), original).unwrap();
        }
    }

    /// 所有者の生存中は空でも登録済みの群を保持するfixture。
    #[test]
    #[ignore = "internal subprocess fixture"]
    fn memory_cleanup_owner_fixture() {
        let Some(ready) = std::env::var_os("JADE_PDF2DOCX_MEMORY_CLEANUP_READY") else {
            return;
        };
        let group = memory_group::MemoryGroup::new(&parent(), 64 * 1024 * 1024).unwrap();
        let _child = if std::env::var("JADE_PDF2DOCX_MEMORY_CLEANUP_POPULATED").unwrap() == "1" {
            let mut command = Command::new("/bin/sleep");
            command.arg("60");
            group.configure(&mut command).unwrap();
            Some(command.spawn().unwrap())
        } else {
            None
        };
        let path = fs::canonicalize(group.path()).unwrap();
        // 読み手が作成直後の空ファイルを見ないよう、準備完了をatomicに公開する。
        let ready = PathBuf::from(ready);
        let staged = ready.with_extension("pending");
        fs::write(&staged, path.as_os_str().as_encoded_bytes()).unwrap();
        fs::rename(staged, ready).unwrap();
        loop {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
