//! Unixのdirectory descriptorを保持し、自動回収先の同名置換を検出する。
use std::{
    fs, io,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

/// 開いたinodeを生存させ、削除後のinode再利用も同一領域と誤認しない所有証拠。
pub(crate) struct DirectoryIdentity {
    /// リンクを辿らずに開いたdirectoryのdescriptor。
    directory: fs::File,
}

impl DirectoryIdentity {
    /// directoryだけをリンク追跡なしで開き、guardの生存期間中保持する。
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(path)?;
        Ok(Self { directory })
    }

    /// パスが同じdevice/inodeのdirectoryを指す場合だけ回収を許可する。
    /// 検査後の外部renameとの競合を排他的に防ぐものではない。
    pub(crate) fn matches(&self, path: &Path) -> bool {
        let Ok(current) = fs::symlink_metadata(path) else {
            return false;
        };
        let Ok(owned) = self.directory.metadata() else {
            return false;
        };
        current.is_dir() && current.dev() == owned.dev() && current.ino() == owned.ino()
    }
}

/// 名前付きの通常fileが、保持したdescriptorと同じdevice/inodeか照合する。
pub(crate) fn matches_file(path: &Path, file: &fs::File) -> bool {
    let Ok(current) = fs::symlink_metadata(path) else {
        return false;
    };
    let Ok(owned) = file.metadata() else {
        return false;
    };
    current.is_file() && current.dev() == owned.dev() && current.ino() == owned.ino()
}
