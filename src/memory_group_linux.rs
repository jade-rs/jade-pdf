//! cgroup v2の制限を設定してからexec前の子を所属させる。
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileExt, MetadataExt, OpenOptionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// 同一プロセス内で専用directory名が衝突しない連番。
static NEXT_GROUP: AtomicU64 = AtomicU64::new(0);

#[path = "memory_cleanup_linux.rs"]
mod stale;
pub(crate) use stale::cleanup;
#[path = "memory_parent_linux.rs"]
mod parent;
pub(crate) use parent::ParentDeath;

/// 明示回収protocolを実装した群だけに付ける名前の接頭辞。
const REGISTERED_PREFIX: &str = "jade-pdf2docx-memory-v1-";

/// 作成した空cgroupだけを、親directoryのFDを保持して回収する。
struct CreatedDirectory {
    /// パスの祖先が変更されても元の委譲先を保持する。
    _parent: File,
    /// 親FDに相対する、自分が排他的に作成したdirectory。
    path: PathBuf,
    /// 元inodeを生存させ、外部の同名置換を削除しない。
    identity: Option<File>,
}

impl CreatedDirectory {
    /// 元のdirectoryを参照している場合だけ空群を削除する。
    fn remove(&self) -> io::Result<()> {
        let Some(identity) = &self.identity else {
            return Err(io::Error::other("memory cgroup identity was not captured"));
        };
        let current = fs::symlink_metadata(&self.path)?;
        let owned = identity.metadata()?;
        if !current.is_dir() || current.dev() != owned.dev() || current.ino() != owned.ino() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "memory cgroup changed during cleanup",
            ));
        }
        fs::remove_dir(&self.path)
    }
}

impl Drop for CreatedDirectory {
    /// cgroupの仮想control fileを再帰削除せず、空directoryだけ削除する。
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

/// 変換監督とその全子孫が継承する、カーネルにaccountされるメモリの制限。
pub(crate) struct MemoryGroup {
    /// 専用directoryと委譲先の所有権。
    directory: CreatedDirectory,
    /// control fileのinodeと、空群も含む生存leaseを保持するdirectory FD。
    handle: File,
    /// exec前の所属変更先。CLOEXECで変換側へ残さない。
    procs: File,
    /// forkやsetsidした子孫もまとめて停止するcontrol file。
    kill: File,
    /// directoryを回収済みか。
    cleaned: bool,
}

impl MemoryGroup {
    /// 検証済みのcgroup filesystem内に、設定済みの空の群を作成する。
    pub(crate) fn new(parent: &Path, bytes: u64) -> io::Result<Self> {
        if bytes == 0 || bytes > i64::MAX as u64 || !parent.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid memory limit or cgroup parent",
            ));
        }
        let parent = open_directory(parent)?;
        require_cgroup(&parent)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let name = format!(
            "{REGISTERED_PREFIX}{}-{stamp}-{}",
            std::process::id(),
            NEXT_GROUP.fetch_add(1, Ordering::Relaxed)
        );
        let path = fd_path(&parent).join(name);
        fs::create_dir(&path)?;
        let mut directory = CreatedDirectory {
            _parent: parent,
            path,
            identity: None,
        };
        let handle = open_directory(&directory.path)?;
        require_cgroup(&handle)?;
        directory.identity = Some(handle.try_clone()?);
        // 空の起動待ち群も生存中と判定できるよう、登録より先にleaseを取る。
        handle.lock()?;
        let controls = fd_path(&handle);
        // createを使わず、カーネルが提供するcontrol fileだけを書き換える。
        write_control(&controls.join("memory.max"), bytes.to_string().as_bytes())?;
        write_control(&controls.join("memory.swap.max"), b"0")?;
        write_control(&controls.join("memory.oom.group"), b"1")?;
        let procs = OpenOptions::new()
            .write(true)
            .open(controls.join("cgroup.procs"))?;
        let kill = OpenOptions::new()
            .write(true)
            .open(controls.join("cgroup.kill"))?;
        // 設定済みの群を登録し、子cgroupの作成も禁止する。
        write_control(&controls.join("cgroup.max.depth"), b"0")?;
        Ok(Self {
            directory,
            handle,
            procs,
            kill,
            cleaned: false,
        })
    }

    /// 子が変換コードを実行するより前に所属させ、失敗時はspawnを失敗させる。
    pub(crate) fn configure(&self, command: &mut Command) -> io::Result<()> {
        command.env(
            crate::memory_group::PARENT_GROUP_ENV,
            fs::canonicalize(&self.directory.path)?,
        );
        let procs = self.procs.try_clone()?;
        // SAFETY: fork後はwriteとerrno取得のみ。メモリ確保やlockを使わない。
        unsafe {
            command.pre_exec(move || {
                loop {
                    let written = libc::write(procs.as_raw_fd(), b"0".as_ptr().cast(), 1);
                    if written == 1 {
                        return Ok(());
                    }
                    let error = io::Error::last_os_error();
                    if written < 0 && error.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(if written < 0 {
                        error
                    } else {
                        io::ErrorKind::WriteZero.into()
                    });
                }
            });
        }
        Ok(())
    }

    /// reclaimや他のOOM killerの停止と区別し、この群での上限超過を検出する。
    pub(crate) fn exceeded(&self) -> io::Result<bool> {
        let events = fs::read_to_string(fd_path(&self.handle).join("memory.events"))?;
        memory_exceeded(&events)
    }

    /// カーネルの群停止で、process groupを離れた子孫も停止する。
    pub(crate) fn kill(&self) -> io::Result<()> {
        write_one(&self.kill, b"1")
    }

    /// 群を停止し、終了を有限時間待って自分の空cgroupだけを削除する。
    pub(crate) fn cleanup(&mut self) -> io::Result<()> {
        if self.cleaned {
            return Ok(());
        }
        self.kill()?;
        let started = Instant::now();
        loop {
            let events = fs::read_to_string(fd_path(&self.handle).join("cgroup.events"))?;
            if event(&events, "populated")? == 0 {
                self.directory.remove()?;
                self.cleaned = true;
                return Ok(());
            }
            if started.elapsed() >= Duration::from_secs(2) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "memory cgroup remains populated after kill",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// 生存中の親FDに相対する検証用パスを返す。
    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.directory.path
    }
}

impl Drop for MemoryGroup {
    /// 早期returnでも子孫を停止し、回収できなかった群は明示する。
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!("memory cgroup cleanup failed: {error}");
        }
    }
}

/// symlinkの末尾を追わず、既存directoryだけ開く。
fn open_directory(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
}

/// 通常のdirectoryにcontrol fileを作って制限成功と誤認しない。
fn require_cgroup(directory: &File) -> io::Result<()> {
    // SAFETY: kernelが初期化するstatfsの有効な領域と、生存中のFDを渡す。
    let mut info = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(directory.as_raw_fd(), info.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstatfsが成功し、構造体を初期化済み。
    if unsafe { info.assume_init() }.f_type != libc::CGROUP2_SUPER_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "memory limit parent is not a cgroup v2 directory",
        ));
    }
    Ok(())
}

/// 所有するdirectory FDを基点にcontrol fileを開く。
fn fd_path(file: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

/// 既存のcontrol fileへ、先頭から設定値を一度だけ書く。
fn write_control(path: &Path, value: &[u8]) -> io::Result<()> {
    write_one(&OpenOptions::new().write(true).open(path)?, value)
}

/// 短いwriteを設定成功と扱わない。
fn write_one(file: &File, value: &[u8]) -> io::Result<()> {
    if file.write_at(value, 0)? == value.len() {
        Ok(())
    } else {
        Err(io::ErrorKind::WriteZero.into())
    }
}

/// 必須カーネルcounterの欠落や破損を制限成功と扱わない。
fn event(events: &str, key: &str) -> io::Result<u64> {
    events
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(' ')?;
            (name == key).then(|| value.parse().ok())
        })
        .flatten()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid cgroup event counter"))
}

/// oom_killはglobal/祖先のOOMでも増えるため、群自身のoomを参照する。
fn memory_exceeded(events: &str) -> io::Result<bool> {
    Ok(event(events, "oom")? > 0)
}

#[cfg(test)]
mod tests {
    /// 他のOOM killerによる停止を、この群の上限超過と誤報しない。
    #[test]
    fn only_local_limit_oom_is_reported() {
        assert!(!super::memory_exceeded("max 2\noom 0\noom_kill 1\n").unwrap());
        assert!(super::memory_exceeded("oom_kill 0\noom 1\n").unwrap());
        for events in ["", "oom_kill 1\n", "oom -1\n", "oom 18446744073709551616\n"] {
            assert!(super::memory_exceeded(events).is_err());
        }
    }
}
