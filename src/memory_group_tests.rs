//! Linuxカーネルによる合算メモリ制限と起動前の拒否を固定する。
use crate::memory_group::MemoryGroup;

/// 通常directoryをcgroupと誤認せず、内容を変更しない。
#[test]
fn ordinary_directory_is_rejected_without_mutation() {
    let parent = tempfile::tempdir().unwrap();
    std::fs::write(parent.path().join("keep"), b"existing").unwrap();
    assert!(MemoryGroup::new(parent.path(), 64 * 1024 * 1024).is_err());
    assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 1);
    assert_eq!(
        std::fs::read(parent.path().join("keep")).unwrap(),
        b"existing"
    );
}

/// 不正な上限をファイルシステム操作より先に拒否する。
#[test]
fn invalid_capacity_is_rejected() {
    let parent = tempfile::tempdir().unwrap();
    for bytes in [0, u64::MAX] {
        assert!(MemoryGroup::new(parent.path(), bytes).is_err());
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
    }
}

/// 所属変更後にexecが失敗しても、空cgroupを残さない。
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a delegated cgroup v2 memory controller"]
fn real_failed_spawn_removes_group() {
    let group = MemoryGroup::new(&real_parent(), 64 * 1024 * 1024).unwrap();
    let path = group.path().to_owned();
    let mut command = std::process::Command::new("/no-such-jade-pdf2docx-memory-executable");
    group.configure(&mut command).unwrap();
    assert!(command.spawn().is_err());
    drop(group);
    assert!(!path.exists());
}

/// 外部で同名directoryが置換されても、自動回収が別の空群を削除しない。
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a delegated cgroup v2 memory controller"]
fn real_memory_drop_preserves_replaced_group() {
    let group = MemoryGroup::new(&real_parent(), 64 * 1024 * 1024).unwrap();
    let path = std::fs::canonicalize(group.path()).unwrap();
    std::fs::remove_dir(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    drop(group);
    let preserved = path.is_dir();
    let _ = std::fs::remove_dir(path);
    assert!(preserved, "自分が開いたinode以外を削除しない");
}

/// setsidでprocess groupを離れた子孫もcgroup停止で回収する。
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a delegated cgroup v2 memory controller"]
fn real_cleanup_kills_detached_descendant() {
    use std::{
        process::Command,
        time::{Duration, Instant},
    };
    let mut group = MemoryGroup::new(&real_parent(), 64 * 1024 * 1024).unwrap();
    let base = tempfile::tempdir().unwrap();
    let pid_file = base.path().join("pid");
    let mut command = Command::new("setsid");
    command
        .args([
            "/bin/sh",
            "-c",
            "echo $$ > \"$JADE_PDF2DOCX_MEMORY_PID_FILE\"; exec sleep 60",
        ])
        .env("JADE_PDF2DOCX_MEMORY_PID_FILE", &pid_file);
    group.configure(&mut command).unwrap();
    let mut child = command.spawn().unwrap();
    let started = Instant::now();
    while !pid_file.exists() {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    group.cleanup().unwrap();
    assert!(!child.wait().unwrap().success());
}

/// 明示した実cgroupの委譲先を必須とし、実カーネル検証を黙って省略しない。
#[cfg(target_os = "linux")]
fn real_parent() -> std::path::PathBuf {
    std::env::var_os("JADE_PDF2DOCX_TEST_CGROUP_PARENT")
        .map(std::path::PathBuf::from)
        .expect(
            "JADE_PDF2DOCX_TEST_CGROUP_PARENT must name a writable delegated cgroup v2 directory",
        )
}

/// 一つずつなら成功する割当でも、同時に二つ保持すると群全体がOOMで停止する。
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a delegated cgroup v2 memory controller"]
fn real_combined_memory_limit_kills_group_and_cleans_up() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let parent = real_parent();
    for count in [1, 2] {
        let mut group = MemoryGroup::new(&parent, 128 * 1024 * 1024).unwrap();
        let path = group.path().to_owned();
        assert_eq!(
            std::fs::read_to_string(path.join("memory.max"))
                .unwrap()
                .trim(),
            "134217728"
        );
        assert_eq!(
            std::fs::read_to_string(path.join("memory.swap.max"))
                .unwrap()
                .trim(),
            "0"
        );
        assert_eq!(
            std::fs::read_to_string(path.join("memory.oom.group"))
                .unwrap()
                .trim(),
            "1"
        );
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(
            "\"$JADE_PDF2DOCX_TEST_EXE\" --ignored --exact memory_group_tests::memory_holder --nocapture &\n\
             if [ \"$JADE_PDF2DOCX_TEST_COUNT\" = 2 ]; then \"$JADE_PDF2DOCX_TEST_EXE\" --ignored --exact memory_group_tests::memory_holder --nocapture & fi\nwait"
        ).env("JADE_PDF2DOCX_TEST_EXE", std::env::current_exe().unwrap())
            .env("JADE_PDF2DOCX_TEST_COUNT", count.to_string())
            .env("JADE_PDF2DOCX_TEST_HOLD", "1")
            .stdout(Stdio::null()).stderr(Stdio::null());
        group.configure(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > Duration::from_secs(15) {
                group.kill().unwrap();
                child.wait().unwrap();
                panic!("memory holder did not terminate");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(group.exceeded().unwrap(), count == 2);
        assert_eq!(status.success(), count == 1, "{status}");
        group.cleanup().unwrap();
        assert!(!path.exists());
    }
}

/// allocatorを最適化で消さず、80MiBの各ページを触って保持する実プロセスfixture。
#[cfg(target_os = "linux")]
#[test]
#[ignore = "internal subprocess fixture"]
fn memory_holder() {
    if std::env::var_os("JADE_PDF2DOCX_TEST_HOLD").is_none() {
        return;
    }
    let mut bytes = vec![0_u8; 80 * 1024 * 1024];
    for byte in bytes.iter_mut().step_by(4096) {
        *byte = 1;
    }
    std::hint::black_box(&bytes);
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert_eq!(bytes[0], 1);
}
