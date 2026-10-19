//! 強制終了後の空cgroupを、登録protocolと生存leaseに従って明示回収する。
use super::{REGISTERED_PREFIX, event, fd_path, open_directory, require_cgroup};
use std::{
    fs::{self, File, TryLockError},
    io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

/// 指定された一つだけを検証し、停止操作や再帰削除を行わず回収する。
pub(crate) fn cleanup(path: &Path) -> io::Result<()> {
    Candidate::open(path)?.remove()
}

/// 元の親・対象のFDと排他leaseを、最後の削除まで保持する候補。
struct Candidate {
    /// 親パスが変更されても元のdirectoryを参照する。
    _parent: File,
    /// 親FDに相対する対象の名前。
    path: PathBuf,
    /// 元のinodeと、その生存leaseを保持する。
    handle: File,
}

impl Candidate {
    /// 登録済みの同一ユーザーの群だけを開き、生存所有者と排他する。
    fn open(path: &Path) -> io::Result<Self> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(invalid)?;
        if !path.is_absolute() || !registered_name(name) {
            return Err(invalid());
        }
        let parent = open_directory(path.parent().ok_or_else(invalid)?)?;
        require_cgroup(&parent)?;
        let path = fd_path(&parent).join(name);
        let handle = open_directory(&path)?;
        require_cgroup(&handle)?;
        validate_registration(&handle, name)?;
        match handle.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "memory cgroup owner is alive",
                ));
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
        Ok(Self {
            _parent: parent,
            path,
            handle,
        })
    }

    /// kernelの所属状態と元inodeを再検証し、空directoryだけを削除する。
    fn remove(self) -> io::Result<()> {
        let events = fs::read_to_string(fd_path(&self.handle).join("cgroup.events"))?;
        if event(&events, "populated")? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "memory cgroup remains populated",
            ));
        }
        let current = fs::symlink_metadata(&self.path)?;
        let owned = self.handle.metadata()?;
        if !current.is_dir() || current.dev() != owned.dev() || current.ino() != owned.ino() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "memory cgroup changed during cleanup",
            ));
        }
        fs::remove_dir(&self.path)
    }
}

/// 開いた元directoryの種類・所有者・名前・登録済み設定を検証する。
pub(super) fn validate_registration(handle: &File, name: &str) -> io::Result<()> {
    require_cgroup(handle)?;
    if !registered_name(name) {
        return Err(invalid());
    }
    // SAFETY: geteuidには引数や有効期間の前提条件がない。
    if handle.metadata()?.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "memory cgroup belongs to another user",
        ));
    }
    let controls = fd_path(&handle);
    let maximum = fs::read_to_string(controls.join("memory.max"))?
        .trim()
        .parse::<u64>()
        .map_err(|_| invalid())?;
    if maximum == 0 || maximum > i64::MAX as u64 {
        return Err(invalid());
    }
    for (name, expected) in [
        ("memory.swap.max", "0"),
        ("memory.oom.group", "1"),
        ("cgroup.max.depth", "0"),
    ] {
        if fs::read_to_string(controls.join(name))?.trim() != expected {
            return Err(invalid());
        }
    }
    Ok(())
}

/// バージョン付き接頭辞と、作成時の三つの十進数だけを認める。
fn registered_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix(REGISTERED_PREFIX) else {
        return false;
    };
    let mut fields = suffix.split('-');
    (0..3).all(|_| {
        fields.next().is_some_and(|field| {
            !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit())
        })
    }) && fields.next().is_none()
}

/// 未登録・旧版・不完全設定の群を回収しない。
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "not a registered jade-pdf2docx memory cgroup",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最終削除前に名前が同じ別inodeへ置換された場合は、その群を保持する。
    #[test]
    #[ignore = "requires a delegated cgroup v2 memory controller"]
    fn memory_cleanup_replaced_candidate_is_preserved() {
        let parent = std::env::var_os("JADE_PDF2DOCX_TEST_CGROUP_PARENT").unwrap();
        let group = super::super::MemoryGroup::new(Path::new(&parent), 64 * 1024 * 1024).unwrap();
        let path = fs::canonicalize(group.path()).unwrap();
        // 所有者死亡後の状態を再現し、候補を開いてから同名群へ置換する。
        group.handle.unlock().unwrap();
        let candidate = Candidate::open(&path).unwrap();
        fs::remove_dir(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let result = candidate.remove();
        assert!(result.is_err());
        drop(group);
        let preserved = path.is_dir();
        let _ = fs::remove_dir(path);
        assert!(preserved);
    }
}
