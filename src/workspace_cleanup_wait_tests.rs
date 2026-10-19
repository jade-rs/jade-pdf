//! forkからexecまで残る生存leaseと、明示回収の有限待機を検証する。
use crate::workspace::Workspace;
use std::{
    fs,
    io::{Read, Write},
    os::unix::{net::UnixStream, process::CommandExt},
    process::Command,
    time::{Duration, Instant},
};

/// 無関係な子がexec前に継承したleaseも尊重し、exec後だけ回収する。
#[test]
fn cleanup_waits_for_inherited_lease_until_child_exec() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join("jade-pdf2docx-supervised-inherited-lease");
    let workspace = Workspace::create_registered(&path, || Ok(())).unwrap();
    let worker = crate::workspace::worker_lease(Some(&path))
        .unwrap()
        .unwrap();
    fs::write(path.join("partial"), b"protected").unwrap();
    drop(workspace);
    let (mut parent, mut child) = UnixStream::pair().unwrap();
    parent
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    child
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let spawning = std::thread::spawn(move || {
        let mut command = Command::new("/usr/bin/true");
        // fork後にはsocketのread/writeだけを使い、allocや他threadのlockを触らない。
        unsafe {
            command.pre_exec(move || {
                child.write_all(&[1])?;
                child.read_exact(&mut [0])?;
                Ok(())
            });
        }
        command.spawn().unwrap().wait().unwrap()
    });
    parent.read_exact(&mut [0]).unwrap();
    drop(worker);
    let probe = Workspace::registered_marker(&path).unwrap();
    assert!(matches!(
        probe.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));
    assert_eq!(fs::read(path.join("partial")).unwrap(), b"protected");
    let releasing = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        parent.write_all(&[1]).unwrap();
    });
    let result = super::cleanup(&path);
    releasing.join().unwrap();
    assert!(spawning.join().unwrap().success());
    assert!(result.is_ok(), "exec後の回収が失敗した: {result:?}");
    assert!(!path.exists());
}

/// 生存leaseが解放されなければ有限時間で拒否し、部分成果物を保持する。
#[test]
fn cleanup_keeps_live_workspace_after_wait_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("jade-pdf2docx-supervised-live-wait");
    let workspace = Workspace::create_registered(&path, || Ok(())).unwrap();
    fs::write(path.join("partial"), b"protected").unwrap();
    let started = Instant::now();
    let error = super::cleanup(&path).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(fs::read(path.join("partial")).unwrap(), b"protected");
    drop(workspace);
    assert!(!path.exists());
}

/// 待機中にdirectoryまたはmarkerが交換されても、新しい領域を削除しない。
#[test]
fn cleanup_revalidates_identity_after_waiting_for_lease() {
    for kind in ["directory", "marker"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .join("jade-pdf2docx-supervised-replaced-wait");
        let workspace = Workspace::create_registered(&path, || Ok(())).unwrap();
        let worker = crate::workspace::worker_lease(Some(&path))
            .unwrap()
            .unwrap();
        let candidate = super::Candidate::open(&path).unwrap();
        drop(workspace);
        let replacing_path = path.clone();
        let replacing = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let marker = replacing_path.join(crate::workspace::MARKER);
            let content = fs::read(&marker).unwrap();
            if kind == "directory" {
                fs::rename(&replacing_path, replacing_path.with_extension("kept")).unwrap();
                fs::create_dir(&replacing_path).unwrap();
            } else {
                fs::rename(&marker, replacing_path.join("kept-marker")).unwrap();
            }
            fs::write(&marker, content).unwrap();
            fs::write(replacing_path.join("partial"), b"foreign").unwrap();
            drop(worker);
        });
        let result = candidate.remove();
        replacing.join().unwrap();
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::InvalidData,
            "{kind}"
        );
        assert_eq!(
            fs::read(path.join("partial")).unwrap(),
            b"foreign",
            "{kind}"
        );
    }
}
