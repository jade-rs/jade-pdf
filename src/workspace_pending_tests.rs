//! rename前の完成markerだけを回収し、不完全な所有証拠を保持する回帰。
use super::{Candidate, cleanup, tests::holder};
use crate::workspace::{MARKER, Workspace};
use std::{fs, io, path::Path};

/// 作成者がrename前に共有leaseを保持するmarker名。
const PENDING: &str = "workspace-marker-pending";

/// 候補の絶対パスに一致する現行版の所有証拠を作る。
fn content(path: &Path) -> Vec<u8> {
    let mut bytes = b"JADE-PDF2DOCX-WORKSPACE-3\n".to_vec();
    bytes.extend_from_slice(path.as_os_str().as_encoded_bytes());
    bytes
}

/// rename前の生存監督は保持し、SIGKILLでleaseが解放された後だけ回収する。
#[test]
fn completed_pending_survives_creator_until_killed() {
    let base = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(base.path()).unwrap();
    let path = root.join("jade-pdf2docx-supervised-pending-creator");
    let mut creator = holder(&path, &root.join("ready"), "creator-pending");
    assert!(!path.join(MARKER).exists());
    assert_eq!(fs::read(path.join(PENDING)).unwrap(), content(&path));
    // 自動回収の親/workerは、未公開markerを所有通知として受け取らない。
    assert!(Workspace::recover_registered(&path).is_err());
    assert!(crate::workspace::worker_lease(Some(&path)).is_err());
    fs::write(path.join("partial"), b"partial").unwrap();
    fs::write(root.join("input.pdf"), b"keep").unwrap();
    let live_error = cleanup(&path).unwrap_err().kind();
    assert_eq!(fs::read(path.join("partial")).unwrap(), b"partial");
    creator.child.kill().unwrap();
    creator.child.wait().unwrap();
    creator.reaped = true;
    cleanup(&path).unwrap();
    assert_eq!(live_error, io::ErrorKind::WouldBlock);
    assert!(!path.exists());
    assert_eq!(fs::read(root.join("input.pdf")).unwrap(), b"keep");
}

/// 空・途中・旧版・別パス・余分なbyteと、不正な完成markerを削除しない。
#[test]
fn incomplete_or_foreign_pending_is_preserved() {
    let base = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(base.path()).unwrap();
    for kind in [
        "empty",
        "partial",
        "old",
        "foreign",
        "extra",
        "invalid-final",
    ] {
        let path = root.join(format!("jade-pdf2docx-supervised-pending-{kind}"));
        fs::create_dir(&path).unwrap();
        let expected = content(&path);
        let bytes = match kind {
            "empty" => Vec::new(),
            "partial" => expected[..expected.len() - 1].to_vec(),
            "old" => b"JADE-PDF2DOCX-WORKSPACE-2\n".to_vec(),
            "foreign" => content(&root.join("jade-pdf2docx-supervised-another")),
            "extra" => [expected.as_slice(), b"x"].concat(),
            _ => expected,
        };
        fs::write(path.join(PENDING), &bytes).unwrap();
        if kind == "invalid-final" {
            fs::write(path.join(MARKER), b"invalid").unwrap();
        }
        fs::write(path.join("keep"), b"foreign").unwrap();
        assert!(cleanup(&path).is_err(), "{kind}");
        assert_eq!(fs::read(path.join(PENDING)).unwrap(), bytes, "{kind}");
        assert_eq!(fs::read(path.join("keep")).unwrap(), b"foreign", "{kind}");
    }
}

/// pendingのsymlink/FIFO/directoryを所有証拠に使わず、外部内容を保持する。
#[test]
fn pending_links_and_non_files_are_preserved() {
    use std::os::unix::fs::symlink;
    let base = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(base.path()).unwrap();
    for kind in ["link", "fifo", "directory"] {
        let path = root.join(format!("jade-pdf2docx-supervised-pending-{kind}"));
        fs::create_dir(&path).unwrap();
        let external = root.join(format!("external-{kind}"));
        fs::write(&external, content(&path)).unwrap();
        let marker = path.join(PENDING);
        match kind {
            "link" => symlink(&external, &marker).unwrap(),
            "fifo" => assert!(
                std::process::Command::new("mkfifo")
                    .arg(&marker)
                    .status()
                    .unwrap()
                    .success()
            ),
            _ => fs::create_dir(&marker).unwrap(),
        }
        fs::write(path.join("keep"), b"foreign").unwrap();
        assert!(cleanup(&path).is_err(), "{kind}");
        assert_eq!(fs::read(&external).unwrap(), content(&path));
        assert_eq!(fs::read(path.join("keep")).unwrap(), b"foreign");
    }
}

/// 候補取得後のdirectory/marker交換と公開renameで、確認前の削除を拒否する。
#[test]
fn pending_candidate_rejects_replacement_and_publication() {
    let base = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(base.path()).unwrap();
    for kind in ["directory", "marker", "publication"] {
        let path = root.join(format!("jade-pdf2docx-supervised-pending-{kind}"));
        fs::create_dir(&path).unwrap();
        fs::write(path.join(PENDING), content(&path)).unwrap();
        let candidate = Candidate::open(&path).unwrap();
        match kind {
            "directory" => {
                fs::rename(&path, root.join("kept-directory")).unwrap();
                fs::create_dir(&path).unwrap();
                fs::write(path.join(PENDING), content(&path)).unwrap();
            }
            "marker" => {
                fs::rename(path.join(PENDING), root.join("kept-marker")).unwrap();
                fs::write(path.join(PENDING), content(&path)).unwrap();
            }
            _ => fs::rename(path.join(PENDING), path.join(MARKER)).unwrap(),
        }
        fs::write(path.join("keep"), b"foreign").unwrap();
        assert!(candidate.remove().is_err(), "{kind}");
        assert_eq!(fs::read(path.join("keep")).unwrap(), b"foreign");
    }
}
