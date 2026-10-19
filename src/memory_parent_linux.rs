//! 親を失った監督が、自分の所属する登録済みcgroupだけを停止する。
use super::{fd_path, open_directory, stale::validate_registration, write_one};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader},
    path::Path,
};

/// 元のcgroup.kill inodeを保持する、非所有の親死亡時停止権。
pub(crate) struct ParentDeath {
    /// 名前の置換後も、検証済みの元の群だけを停止する。
    kill: File,
}

impl ParentDeath {
    /// 親が渡したパスも信用せず、現在の自分の所属を検証する。
    pub(crate) fn from_env() -> io::Result<Option<Self>> {
        std::env::var_os(crate::memory_group::PARENT_GROUP_ENV)
            .map(|path| Self::open(Path::new(&path)))
            .transpose()
    }

    /// 同じuidの別群や通常fileを停止対象にできないよう、kernelの所属を照合する。
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "memory group path must be absolute",
            ));
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid memory group name")
            })?;
        let directory = open_directory(path)?;
        validate_registration(&directory, name)?;
        let controls = fd_path(&directory);
        let member = std::process::id().to_string();
        let mut found = false;
        // kernelが生成するPID行だけを走査し、所属全体をメモリへ読み込まない。
        for line in BufReader::new(File::open(controls.join("cgroup.procs"))?).lines() {
            if line? == member {
                found = true;
                break;
            }
        }
        if !found {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "supervisor is not in the specified memory group",
            ));
        }
        let kill = OpenOptions::new()
            .write(true)
            .open(controls.join("cgroup.kill"))?;
        Ok(Self { kill })
    }

    /// workspace guardの終了後に呼び出し、自分と離脱した子孫を一括停止する。
    pub(crate) fn terminate(&self) -> io::Result<()> {
        write_one(&self.kill, b"1")
    }
}
