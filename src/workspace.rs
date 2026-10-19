//! 監督の作成guardと、完成markerを引き受けた親のguardで専用一時領域を回収する。
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

/// 内部監督だけが読む、親の専用一時領域の指定。
pub(crate) const ENV: &str = "JADE_PDF2DOCX_SUPERVISOR_WORKSPACE";
/// 他のdirectoryを誤って回収しないための所有marker名。
pub(crate) const MARKER: &str = ".jade-pdf2docx-workspace-owner";
/// 完成内容を書き終えてから所有通知へrenameするまでのmarker名。
const PENDING_MARKER: &str = "workspace-marker-pending";
/// markerの形式を確認する固定byte列。
#[cfg(test)]
const CONTENT: &[u8] = b"JADE-PDF2DOCX-WORKSPACE-1\n";
/// 親が選んだ候補を監督が作成するdirectoryの識別prefix。
pub(crate) const PREFIX: &str = "jade-pdf2docx-supervised-";
/// 親が補助回収権を取得してから公開する起動許可。
pub(crate) const ACK: &str = "workspace-ack";
/// 起動許可のprotocol版。
const ACK_CONTENT: &[u8] = b"JADE-PDF2DOCX-WORKSPACE-ACK-1\n";
/// 同じfilesystemに置く保存専用directoryの識別prefix。
pub(crate) const COMMIT_PREFIX: &str = ".jade-pdf2docx-commit-";

/// 親の自動回収と監督の補助回収を表す所有権。
pub(crate) enum Workspace {
    /// 親が作成し、通常のTempDirとして回収する領域。
    #[cfg(test)]
    Parent(tempfile::TempDir),
    /// 自前で作成した領域、または内部通信後に受け取った補助回収の所有権。
    Recovery {
        /// このguardだけが回収する専用directory。
        path: PathBuf,
        /// Unixでは取得時のdirectoryを保持し、同名の第三者領域を回収しない。
        #[cfg(unix)]
        identity: crate::workspace_identity::DirectoryIdentity,
        /// 完成した外側markerに保持する共有生存lease。
        lease: Option<fs::File>,
    },
}

impl Workspace {
    /// 出力directoryを作らず、privateな一時directoryと識別markerを作る。
    #[cfg(test)]
    pub(crate) fn new() -> io::Result<Self> {
        let directory = tempfile::Builder::new().prefix(PREFIX).tempdir()?;
        fs::write(directory.path().join(MARKER), CONTENT)?;
        Ok(Self::Parent(directory))
    }

    /// 出力親の既存directory祖先へ配置し、保存と一時領域のfilesystemを揃える。
    /// 未作成の出力親や途中の通常fileは、worker成功後の出力準備まで変更しない。
    #[cfg(test)]
    pub(crate) fn new_for_output(output: &Path) -> io::Result<Self> {
        let ancestor = existing_output_ancestor(output)?;
        let mut builder = tempfile::Builder::new();
        builder.prefix(PREFIX);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let directory = builder.tempdir_in(ancestor)?;
        fs::write(directory.path().join(MARKER), CONTENT)?;
        Ok(Self::Parent(directory))
    }

    /// 指定した基点に保存専用directoryを予約し、親側で回収を所有する。
    #[cfg(test)]
    pub(crate) fn new_commit_in(parent: &Path) -> io::Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix(COMMIT_PREFIX)
            .tempdir_in(parent)?;
        fs::write(directory.path().join(MARKER), CONTENT)?;
        Ok(Self::Parent(directory))
    }

    /// mkdir前に候補を記録し、このjobだけが回収できるmarkerを作る。
    pub(crate) fn new_registered_commit_in(
        parent: &Path,
        owner: &Path,
        mut register: impl FnMut(&Path) -> io::Result<()>,
    ) -> io::Result<Self> {
        let temporary = tempfile::Builder::new()
            .prefix(COMMIT_PREFIX)
            .rand_bytes(16)
            .make_in(parent, |path| {
                register(path)?;
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(path)?;
                let directory = Self::Recovery {
                    path: path.to_owned(),
                    #[cfg(unix)]
                    identity: crate::workspace_identity::DirectoryIdentity::open(path)?,
                    lease: None,
                };
                #[cfg(test)]
                creation_checkpoint();
                fs::write(path.join(MARKER), commit_content(owner))?;
                Ok(directory)
            })?;
        let (directory, mut path) = temporary.into_parts();
        // directoryのguardが削除を所有するため、file用のunlinkは使用しない。
        path.disable_cleanup(true);
        Ok(directory)
    }

    /// 作成予定または通知済みの保存領域を、job固有markerで照合する。
    pub(crate) fn recover_registered_commit(path: &Path, owner: &Path) -> io::Result<Self> {
        Self::recover_with_content(path, COMMIT_PREFIX, &commit_content(owner))
    }

    /// 内部親の指定を受け取り、形式と実directoryを確認して補助所有権を作る。
    /// 外部ユーザー指定の一般directoryに使用してはならない。
    #[cfg(test)]
    pub(crate) fn recover(path: &Path) -> io::Result<Self> {
        Self::recover_with_content(path, PREFIX, CONTENT)
    }

    /// 内部親から通知された保存専用領域の補助回収を引き受ける。
    #[cfg(test)]
    pub(crate) fn recover_commit(path: &Path) -> io::Result<Self> {
        Self::recover_with_content(path, COMMIT_PREFIX, CONTENT)
    }

    /// 用途別のprefixとmarkerを確認し、一般directoryの回収を拒否する。
    fn recover_with_content(path: &Path, prefix: &str, expected: &[u8]) -> io::Result<Self> {
        #[cfg(unix)]
        let identity = crate::workspace_identity::DirectoryIdentity::open(path)?;
        Self::validated_marker(path, prefix, expected)?;
        Ok(Self::Recovery {
            path: path.to_owned(),
            #[cfg(unix)]
            identity,
            lease: None,
        })
    }

    /// 型と内容を照合し、回収判定と生存leaseで共有するmarkerを開く。
    fn validated_marker(path: &Path, prefix: &str, expected: &[u8]) -> io::Result<fs::File> {
        Self::validated_marker_named(path, prefix, expected, MARKER)
    }

    /// 指定名の通常fileを同一inode・完全一致する上限付き内容で検査する。
    fn validated_marker_named(
        path: &Path,
        prefix: &str,
        expected: &[u8],
        name: &str,
    ) -> io::Result<fs::File> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid supervisor workspace");
        let directory_metadata = fs::symlink_metadata(path)?;
        let marker_metadata = fs::symlink_metadata(path.join(name))?;
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(prefix))
            || !directory_metadata.is_dir()
            || !marker_metadata.is_file()
        {
            return Err(invalid());
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let mut marker = options.open(path.join(name))?;
        if !marker.metadata()?.is_file() {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let opened = marker.metadata()?;
            if opened.dev() != marker_metadata.dev() || opened.ino() != marker_metadata.ino() {
                return Err(invalid());
            }
        }
        let mut content = Vec::new();
        Read::by_ref(&mut marker)
            .take(expected.len() as u64 + 1)
            .read_to_end(&mut content)?;
        if content != expected {
            return Err(invalid());
        }
        Ok(marker)
    }

    /// 新規worker領域を作る基点と、後片付け対象のパスを返す。
    pub(crate) fn path(&self) -> &Path {
        match self {
            #[cfg(test)]
            Self::Parent(directory) => directory.path(),
            Self::Recovery { path, .. } => path,
        }
    }

    /// 親の切断監視開始後、指定候補を排他的に作成して所有する。
    pub(crate) fn receive(mut check: impl FnMut() -> io::Result<()>) -> io::Result<Option<Self>> {
        std::env::var_os(ENV)
            .map(|path| Self::create_registered(Path::new(&path), &mut check))
            .transpose()
    }

    /// mkdir後は直ちにguardで所有し、停止やmarker書込失敗でも未完成領域を回収する。
    pub(crate) fn create_registered(
        path: &Path,
        mut check: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Self> {
        check()?;
        if !path.is_absolute()
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(PREFIX))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid workspace candidate",
            ));
        }
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
        let mut directory = Self::Recovery {
            path: path.to_owned(),
            #[cfg(unix)]
            identity: crate::workspace_identity::DirectoryIdentity::open(path)?,
            lease: None,
        };
        #[cfg(test)]
        startup_creation_checkpoint(
            path,
            "JADE_PDF2DOCX_TEST_WORKSPACE_CREATION_CHECKPOINT",
            &mut check,
        )?;
        check()?;
        let mut marker = fs::File::create_new(path.join(PENDING_MARKER))?;
        marker.try_lock_shared()?;
        marker.write_all(&workspace_content(path))?;
        match &mut directory {
            Self::Recovery { lease, .. } => *lease = Some(marker),
            #[cfg(test)]
            Self::Parent(_) => unreachable!("new workspace must use a recovery guard"),
        }
        #[cfg(test)]
        startup_creation_checkpoint(
            path,
            "JADE_PDF2DOCX_TEST_WORKSPACE_MARKER_CHECKPOINT",
            &mut check,
        )?;
        fs::rename(path.join(PENDING_MARKER), path.join(MARKER))?;
        Ok(directory)
    }

    /// 候補パスに結びついた完成markerだけで親の補助回収権を作る。
    pub(crate) fn recover_registered(path: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        let identity = crate::workspace_identity::DirectoryIdentity::open(path)?;
        let marker = Self::registered_marker(path)?;
        marker.try_lock_shared()?;
        Ok(Self::Recovery {
            path: path.to_owned(),
            #[cfg(unix)]
            identity,
            lease: Some(marker),
        })
    }

    /// 親/workerが所有通知として採用する、公開済みの現行版markerを開く。
    pub(crate) fn registered_marker(path: &Path) -> io::Result<fs::File> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workspace path must be absolute",
            ));
        }
        Self::validated_marker(path, PREFIX, &workspace_content(path))
    }

    /// 運用回収だけは、公開済みmarkerがない場合に完成したpendingも照合する。
    #[cfg(unix)]
    pub(crate) fn cleanup_marker(path: &Path) -> io::Result<(fs::File, &'static str)> {
        match Self::registered_marker(path) {
            Ok(marker) => Ok((marker, MARKER)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let marker = Self::validated_marker_named(
                    path,
                    PREFIX,
                    &workspace_content(path),
                    PENDING_MARKER,
                )?;
                Ok((marker, PENDING_MARKER))
            }
            Err(error) => Err(error),
        }
    }

    /// 親の補助回収権取得を、部分書込の通知を見せず監督へ伝える。
    pub(crate) fn acknowledge(path: &Path) -> io::Result<()> {
        fs::write(path.join("workspace-ack-pending"), ACK_CONTENT)?;
        fs::rename(path.join("workspace-ack-pending"), path.join(ACK))
    }

    /// 起動許可まで切断を確認して待ち、不正通知ではworkerを起動しない。
    pub(crate) fn wait_for_ack(
        path: &Path,
        mut check: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        loop {
            check()?;
            match fs::symlink_metadata(path.join(ACK)) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
                Ok(metadata) => {
                    if !metadata.is_file() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid workspace acknowledgement",
                        ));
                    }
                    let mut content = Vec::new();
                    fs::File::open(path.join(ACK))?
                        .take(ACK_CONTENT.len() as u64 + 1)
                        .read_to_end(&mut content)?;
                    if content != ACK_CONTENT {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid workspace acknowledgement",
                        ));
                    }
                    check()?;
                    return Ok(());
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// workerも共有leaseを保持するが、guardの終了時に作業directoryは削除しない。
pub(crate) fn worker_lease(path: Option<&Path>) -> io::Result<Option<fs::File>> {
    path.map(|path| {
        let marker = Workspace::registered_marker(path)?;
        marker.try_lock_shared()?;
        Ok(marker)
    })
    .transpose()
}

impl Workspace {
    /// 停止送信後のlease解放を指定時間だけ待ち、元directory/markerだけを回収する。
    /// 期限後も生存する所有者・同名置換・検証不能は保持する。
    pub(crate) fn cleanup_with_grace(&mut self, grace: std::time::Duration) {
        #[allow(irrefutable_let_patterns)]
        if let Self::Recovery {
            path,
            #[cfg(unix)]
            identity,
            lease,
        } = self
        {
            #[cfg(unix)]
            let mut marker_name = MARKER;
            #[cfg(not(unix))]
            let marker_name = MARKER;
            #[cfg(unix)]
            if !identity.matches(path) {
                return;
            }
            if let Some(marker) = lease {
                // 同じFDのlock upgradeはstdで動作未規定なので、一度解放してから取得する。
                // 競合した所有者がいれば保持し、最後のguardまたは明示回収へ任せる。
                if marker.unlock().is_err() {
                    return;
                }
                let started = std::time::Instant::now();
                loop {
                    match marker.try_lock() {
                        Ok(()) => break,
                        Err(fs::TryLockError::WouldBlock) if started.elapsed() < grace => {
                            std::thread::sleep(
                                grace
                                    .saturating_sub(started.elapsed())
                                    .min(std::time::Duration::from_millis(10)),
                            );
                        }
                        Err(_) => return,
                    }
                }
                #[cfg(unix)]
                if let Some(name) = [MARKER, PENDING_MARKER]
                    .iter()
                    .find(|name| crate::workspace_identity::matches_file(&path.join(name), marker))
                {
                    marker_name = name;
                } else {
                    return;
                }
            }
            #[cfg(unix)]
            if !identity.matches(path) {
                return;
            }
            let _ = crate::workspace_removal::remove(path, marker_name);
        }
    }
}

impl Drop for Workspace {
    /// 通常guardは待機せず、生存leaseがあれば最後のguardか明示回収へ任せる。
    fn drop(&mut self) {
        self.cleanup_with_grace(std::time::Duration::ZERO);
    }
}

/// 出力親を変更せず、将来の出力と同じfilesystemの既存祖先を返す。
pub(crate) fn existing_output_ancestor(output: &Path) -> io::Result<PathBuf> {
    let resolved = jade_pdf2docx_docx::resolve_output_parent(output)?;
    let mut ancestor = resolved.as_path();
    loop {
        match fs::metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() => return fs::canonicalize(ancestor),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) => {}
            Err(error) => return Err(error),
        }
        let next = ancestor
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if next == ancestor {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no existing output directory ancestor",
            ));
        }
        ancestor = next;
    }
}

/// 別の予約候補に移動されたmarkerを回収しないよう、絶対パスでjobを識別する。
fn workspace_content(path: &Path) -> Vec<u8> {
    let mut content = b"JADE-PDF2DOCX-WORKSPACE-3\n".to_vec();
    content.extend_from_slice(path.as_os_str().as_encoded_bytes());
    content
}

/// 他jobの保存領域を回収しないよう、private通信領域の絶対パスをmarkerへ含める。
fn commit_content(owner: &Path) -> Vec<u8> {
    let mut content = b"JADE-PDF2DOCX-COMMIT-1\n".to_vec();
    content.extend_from_slice(owner.as_os_str().as_encoded_bytes());
    content
}

/// 実プロセス回帰用に、mkdir直後・所有marker書込前の親を停止させる。
#[cfg(test)]
fn creation_checkpoint() {
    let Some(path) = std::env::var_os("JADE_PDF2DOCX_TEST_COMMIT_CREATION_CHECKPOINT") else {
        return;
    };
    let path = PathBuf::from(path);
    fs::write(&path, b"").unwrap();
    while !path.with_extension("continue").exists() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 指定した作成段階の実プロセス停止を再現し、生存leaseの停止確認を継続する。
#[cfg(test)]
fn startup_creation_checkpoint(
    directory: &Path,
    environment: &str,
    mut check: impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let Some(path) = std::env::var_os(environment) else {
        return Ok(());
    };
    let path = PathBuf::from(path);
    fs::write(&path, directory.as_os_str().as_encoded_bytes())?;
    while !path.with_extension("continue").exists() {
        check()?;
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(())
}
