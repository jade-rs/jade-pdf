//! 自動回収guardが同名の第三者directoryを削除しないことを固定する。
use crate::workspace::{PREFIX, Workspace};
use std::fs;

/// 外側/保存用の作成者と補助回収guardは、取得後の同名置換を削除しない。
#[test]
fn automatic_cleanup_preserves_replaced_directory() {
    for commit in [false, true] {
        for recover in [false, true] {
            let base = tempfile::tempdir().unwrap();
            let owner = tempfile::tempdir().unwrap();
            let directory = if commit {
                Workspace::new_registered_commit_in(base.path(), owner.path(), |_| Ok(())).unwrap()
            } else {
                Workspace::create_registered(
                    &base.path().join(format!("{PREFIX}candidate")),
                    || Ok(()),
                )
                .unwrap()
            };
            let path = directory.path().to_owned();
            let recovery = recover.then(|| {
                if commit {
                    Workspace::recover_registered_commit(&path, owner.path()).unwrap()
                } else {
                    Workspace::recover_registered(&path).unwrap()
                }
            });
            fs::rename(&path, base.path().join("moved")).unwrap();
            fs::create_dir(&path).unwrap();
            fs::write(path.join("keep"), b"foreign").unwrap();
            drop(recovery);
            assert_eq!(fs::read(path.join("keep")).unwrap(), b"foreign");
            drop(directory);
            assert_eq!(fs::read(path.join("keep")).unwrap(), b"foreign");
        }
    }
}

/// 作成済み領域をリンクへ置き換えても、リンク自体とリンク先を保持する。
#[test]
fn automatic_cleanup_preserves_replacement_symlink() {
    use std::os::unix::fs::symlink;
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join(format!("{PREFIX}candidate"));
    let directory = Workspace::create_registered(&path, || Ok(())).unwrap();
    let foreign = base.path().join("foreign");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("keep"), b"foreign").unwrap();
    fs::rename(&path, base.path().join("moved")).unwrap();
    symlink(&foreign, &path).unwrap();
    drop(directory);
    assert!(fs::symlink_metadata(&path).unwrap().is_symlink());
    assert_eq!(fs::read(foreign.join("keep")).unwrap(), b"foreign");
}
