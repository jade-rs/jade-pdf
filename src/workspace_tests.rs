//! 親・監督で共有する一時領域の識別と二重回収を固定する。
use crate::workspace::{MARKER, Workspace};
use std::fs;

/// 自動回収でもworkerの共有生存leaseを尊重し、停止後の明示回収へ残す。
#[test]
fn recovery_preserves_workspace_while_worker_lease_is_alive() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("jade-pdf2docx-supervised-live-descendant");
    let workspace = Workspace::create_registered(&path, || Ok(())).unwrap();
    let worker = crate::workspace::worker_lease(Some(&path))
        .unwrap()
        .unwrap();
    fs::write(path.join("partial"), b"in use").unwrap();
    drop(workspace);
    assert_eq!(fs::read(path.join("partial")).unwrap(), b"in use");
    assert!(crate::workspace_cleanup::cleanup(&path).is_err());
    drop(worker);
    crate::workspace_cleanup::cleanup(&path).unwrap();
    assert!(!path.exists());
}

/// 同名markerが別inodeへ置換された場合、古いleaseで新workerの領域を削除しない。
#[cfg(unix)]
#[test]
fn recovery_preserves_workspace_after_marker_replacement() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("jade-pdf2docx-supervised-new-marker");
    let workspace = Workspace::create_registered(&path, || Ok(())).unwrap();
    let content = fs::read(path.join(MARKER)).unwrap();
    fs::rename(path.join(MARKER), path.join("old-marker")).unwrap();
    fs::write(path.join(MARKER), content).unwrap();
    let worker = crate::workspace::worker_lease(Some(&path))
        .unwrap()
        .unwrap();
    fs::write(path.join("partial"), b"in use").unwrap();
    drop(workspace);
    assert_eq!(fs::read(path.join("partial")).unwrap(), b"in use");
    drop(worker);
    crate::workspace_cleanup::cleanup(&path).unwrap();
    assert!(!path.exists());
}

/// 親が異常終了しても監督の回収所有権で部分ファイルを破棄できる。
#[test]
fn recovery_removes_owned_workspace_and_double_cleanup_is_safe() {
    let parent = Workspace::new().unwrap();
    let path = parent.path().to_owned();
    let recovery = Workspace::recover(&path).unwrap();
    fs::create_dir(path.join("nested")).unwrap();
    fs::write(path.join("nested/partial"), b"partial").unwrap();
    drop(recovery);
    assert!(!path.exists());
    drop(parent);
}

/// 普通のdirectory・不正marker・directoryリンクは回収対象として受け取らない。
#[test]
fn recovery_rejects_foreign_or_corrupt_workspace() {
    let foreign = tempfile::tempdir().unwrap();
    assert!(Workspace::recover(foreign.path()).is_err());
    assert!(foreign.path().exists());
    let owned = Workspace::new().unwrap();
    fs::write(owned.path().join(MARKER), b"bad").unwrap();
    assert!(Workspace::recover(owned.path()).is_err());
    assert!(owned.path().exists());
}

/// markerのリンク先や回収対象のリンク先を誤って削除しない。
#[cfg(unix)]
#[test]
fn recovery_rejects_workspace_and_marker_symlinks() {
    use std::os::unix::fs::symlink;
    let parent = Workspace::new().unwrap();
    let base = tempfile::tempdir().unwrap();
    let alias = base.path().join("jade-pdf2docx-supervised-alias");
    symlink(parent.path(), &alias).unwrap();
    assert!(Workspace::recover(&alias).is_err());
    let marker = parent.path().join(MARKER);
    fs::rename(&marker, parent.path().join("original")).unwrap();
    symlink(parent.path().join("original"), &marker).unwrap();
    assert!(Workspace::recover(parent.path()).is_err());
    assert!(parent.path().exists());
}

/// mkdir前の登録順序、既存名衝突の再試行、job専用の回収とprivate権限を固定する。
#[test]
fn registered_commit_retries_collision_without_deleting_foreign_directory() {
    let base = tempfile::tempdir().unwrap();
    let owner = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let mut candidates = Vec::new();
    let directory = Workspace::new_registered_commit_in(base.path(), owner.path(), |path| {
        assert!(!path.exists(), "candidate must be registered before mkdir");
        if candidates.is_empty() {
            fs::create_dir(path)?;
            fs::write(path.join("keep"), b"foreign")?;
        }
        candidates.push(path.to_owned());
        Ok(())
    })
    .unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(directory.path(), candidates[1]);
    assert!(Workspace::recover_registered_commit(directory.path(), other.path()).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let recovery = Workspace::recover_registered_commit(directory.path(), owner.path()).unwrap();
    drop(recovery);
    assert!(!directory.path().exists());
    assert_eq!(fs::read(candidates[0].join("keep")).unwrap(), b"foreign");
}

/// 保存先と同じfilesystemの既存祖先へ配置し、未作成の出力親は作らない。
#[test]
fn output_workspace_uses_existing_ancestor_and_preserves_foreign_entries() {
    for nested in [
        "output.docx",
        "missing/deeper/output.docx",
        "file/deeper/output.docx",
    ] {
        let base = tempfile::tempdir().unwrap();
        fs::write(base.path().join("file"), b"foreign").unwrap();
        let foreign = base.path().join(".jade-pdf2docx-commit-foreign");
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("keep"), b"foreign").unwrap();
        let output = base.path().join(nested);
        let workspace = Workspace::new_for_output(&output).unwrap();
        assert_eq!(
            workspace.path().parent().unwrap(),
            base.path().canonicalize().unwrap()
        );
        assert!(!base.path().join("missing").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let metadata = fs::metadata(workspace.path()).unwrap();
            assert_eq!(metadata.dev(), fs::metadata(base.path()).unwrap().dev());
            assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        }
        let path = workspace.path().to_owned();
        let recovery = Workspace::recover(&path).unwrap();
        fs::create_dir(path.join(".jade-pdf2docx-commit-unmarked")).unwrap();
        fs::write(
            path.join(".jade-pdf2docx-commit-unmarked/partial"),
            b"partial",
        )
        .unwrap();
        drop(recovery);
        assert!(!path.exists());
        drop(workspace);
        assert_eq!(fs::read(base.path().join("file")).unwrap(), b"foreign");
        assert_eq!(fs::read(foreign.join("keep")).unwrap(), b"foreign");
        assert_eq!(fs::read_dir(base.path()).unwrap().count(), 2);
    }
}

/// 保存先の祖先がdirectoryリンクでも、実体と同じfilesystemに一時領域を配置する。
#[cfg(unix)]
#[test]
fn output_workspace_follows_existing_directory_ancestor() {
    let base = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(destination.path(), base.path().join("alias")).unwrap();
    let workspace =
        Workspace::new_for_output(&base.path().join("alias/missing/output.docx")).unwrap();
    assert_eq!(
        workspace.path().parent().unwrap(),
        destination.path().canonicalize().unwrap()
    );
    assert!(!destination.path().join("missing").exists());
}

/// bare filenameと未作成の相対出力親は、現在directoryのfilesystemに一時領域を置く。
#[test]
fn relative_output_workspace_uses_current_directory_without_creating_destination() {
    let unique = tempfile::tempdir().unwrap();
    let missing =
        std::path::Path::new(unique.path().file_name().unwrap()).join("missing/output.docx");
    for output in [std::path::Path::new("output.docx"), missing.as_path()] {
        let workspace = Workspace::new_for_output(output).unwrap();
        assert_eq!(
            workspace.path().parent().unwrap(),
            std::env::current_dir().unwrap().canonicalize().unwrap()
        );
        assert!(!missing.parent().unwrap().exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                fs::metadata(workspace.path()).unwrap().dev(),
                fs::metadata(".").unwrap().dev()
            );
        }
    }
}

/// 未作成成分の後の親参照を解決し、将来の保存先と同じ既存祖先へ配置する。
#[test]
fn output_workspace_resolves_parent_references_after_missing_components() {
    let base = tempfile::tempdir().unwrap();
    let output = base.path().join("missing/../../output.docx");
    let workspace = Workspace::new_for_output(&output).unwrap();
    assert_eq!(
        workspace.path().parent().unwrap(),
        base.path().parent().unwrap().canonicalize().unwrap()
    );
    assert!(!base.path().join("missing").exists());
}

/// directoryリンクの後の親参照はリンク先を基準にし、字面だけで削らない。
#[cfg(unix)]
#[test]
fn output_workspace_resolves_symlink_before_missing_parent_references() {
    let base = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(destination.path(), base.path().join("alias")).unwrap();
    let output = base.path().join("alias/missing/../../output.docx");
    let workspace = Workspace::new_for_output(&output).unwrap();
    assert_eq!(
        workspace.path().parent().unwrap(),
        destination.path().parent().unwrap().canonicalize().unwrap()
    );
    assert!(!destination.path().join("missing").exists());
}
