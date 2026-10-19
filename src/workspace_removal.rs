//! 検証済みの専用領域を、所有markerを最後まで残して削除する。
use std::{fs, io, path::Path};

/// 呼出元が同一性と排他leaseを確認した領域を削除する。
pub(crate) fn remove(path: &Path, marker: &str) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_name() == marker {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
        #[cfg(test)]
        removal_checkpoint();
    }
    match fs::remove_file(path.join(marker)) {
        Ok(()) => {}
        // 登録前の作成guardはmarkerをまだ持たない。
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::remove_dir(path)
}

/// 実プロセス回帰用に、作業fileの削除直後で回収処理を停止する。
#[cfg(test)]
fn removal_checkpoint() {
    let Some(ready) = std::env::var_os("JADE_PDF2DOCX_TEST_REMOVAL_CHECKPOINT") else {
        return;
    };
    let ready = Path::new(&ready);
    fs::write(ready, b"ready").unwrap();
    while !ready.with_extension("continue").exists() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// 削除不能な作業fileがあるとき、走査順に依存せず所有証拠を保持する。
    #[test]
    fn failed_payload_removal_keeps_marker() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("workspace");
        fs::create_dir(&path).unwrap();
        let marker = path.join("owner");
        fs::write(&marker, b"owned").unwrap();
        let payload = path.join("payload");
        fs::create_dir(&payload).unwrap();
        fs::write(payload.join("partial.png"), b"partial").unwrap();
        // tmpfsの逆順走査でもmarkerを先に読む配置を含める。
        fs::rename(&marker, base.path().join("staged-marker")).unwrap();
        fs::rename(base.path().join("staged-marker"), &marker).unwrap();
        fs::set_permissions(&payload, fs::Permissions::from_mode(0o500)).unwrap();
        // rootなどpermissionを迂回する環境ではこの回帰を適用しない。
        if fs::write(payload.join("permission-probe"), b"").is_ok() {
            fs::set_permissions(&payload, fs::Permissions::from_mode(0o700)).unwrap();
            return;
        }
        let error = remove(&path, "owner").unwrap_err();
        let kept = fs::read(&marker);
        let partial = fs::read(payload.join("partial.png"));
        fs::set_permissions(&payload, fs::Permissions::from_mode(0o700)).unwrap();
        remove(&path, "owner").unwrap();
        assert!(!path.exists());
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(kept.unwrap(), b"owned");
        assert_eq!(partial.unwrap(), b"partial");
    }

    /// 作業領域内のlinkはunlinkだけを行い、外部のdirectory/fileを保持する。
    #[test]
    fn cleanup_does_not_follow_payload_symlinks() {
        use std::os::unix::fs::symlink;
        let base = tempfile::tempdir().unwrap();
        let outside = base.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"foreign").unwrap();
        let path = base.path().join("workspace");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("owner"), b"owned").unwrap();
        symlink(&outside, path.join("linked-directory")).unwrap();
        symlink(outside.join("keep"), path.join("linked-file")).unwrap();
        fs::create_dir(path.join("nested")).unwrap();
        fs::write(path.join("nested/partial"), b"partial").unwrap();
        remove(&path, "owner").unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"foreign");
    }

    /// 登録前の作成guardは、marker作成前でも所有するdirectoryを回収できる。
    #[test]
    fn creation_guard_can_remove_directory_before_marker_exists() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("workspace");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("partial-marker"), b"partial").unwrap();
        remove(&path, "owner").unwrap();
        assert!(!path.exists());
    }
}
