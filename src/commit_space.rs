//! 親が保存領域と最終公開を所有し、監督が同期済み候補と補助回収を担う内部通信。
use crate::{
    worker::{self, WorkerError},
    workspace::{self, Workspace},
};
use jade_pdf2docx_docx::ParseCancellation;
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(test)]
#[path = "commit_space_protocol_tests.rs"]
mod tests;

/// privateな監督領域に作る、worker成功後の保存開始要求。
const REQUEST: &str = "commit-request";
/// 完全な通知だけを読むためにrenameで公開する、保存directoryのbasename。
const GRANT: &str = "commit-grant";
/// mkdir前に公開し、通知前の親死亡でも回収候補を特定する記録。
const INTENT: &str = "commit-intent";
/// コピーと同期完了後だけ監督が公開する通知。
const PREPARED: &str = "commit-prepared";
/// 同期済み候補の固定basename。外部パスはprotocolに含めない。
const CANDIDATE: &str = "document.docx";
/// 不完全な通知を受理しないための固定内容。
const PREPARED_CONTENT: &[u8] = b"JADE-PDF2DOCX-PREPARED-1\n";

/// 子の停止後まで保存用directoryを所有する親側の状態。
pub(crate) struct CommitOwner {
    /// 親と監督だけがアクセスする内部通信領域。
    workspace: PathBuf,
    /// 成果物を公開する最終パス。
    output: PathBuf,
    /// 親だけが指定する、DOCXの公開要求照合情報。
    pub(crate) publication_receipt: Option<String>,
    /// worker成功後にだけ作成する保存専用領域。
    directory: Option<Workspace>,
}

impl CommitOwner {
    /// 出力directoryをまだ作らず、要求の監視を準備する。
    pub(crate) fn new(workspace: &Path, output: &Path) -> Self {
        Self {
            workspace: workspace.to_owned(),
            output: output.to_owned(),
            directory: None,
            publication_receipt: None,
        }
    }

    /// worker成功の要求を観測したら、所有guardを保持してから通知する。
    pub(crate) fn poll(&mut self) -> io::Result<()> {
        if self.directory.is_some() || !self.workspace.join(REQUEST).try_exists()? {
            return Ok(());
        }
        let parent = output_parent(&self.output);
        fs::create_dir_all(parent)?;
        self.directory = Some(Workspace::new_registered_commit_in(
            &self.workspace,
            &self.workspace,
            |path| publish_name(&self.workspace, INTENT, path),
        )?);
        #[cfg(test)]
        allocation_checkpoint();
        publish_name(
            &self.workspace,
            GRANT,
            self.directory.as_ref().unwrap().path(),
        )?;
        Ok(())
    }

    /// 完全な準備通知を確認し、親自身が最終出力へatomic置換する。
    /// 成功後は期限・停止・監督終了状態で公開済みの成功を取り消さない。
    pub(crate) fn publish_prepared(&self, may_publish: impl FnOnce() -> bool) -> io::Result<bool> {
        let Some(directory) = &self.directory else {
            return Ok(false);
        };
        let notice = directory.path().join(PREPARED);
        let metadata = match fs::symlink_metadata(&notice) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            result => result?,
        };
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid prepared notice",
            ));
        }
        let mut content = Vec::new();
        fs::File::open(notice)?
            .take(PREPARED_CONTENT.len() as u64 + 1)
            .read_to_end(&mut content)?;
        if content != PREPARED_CONTENT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid prepared notice",
            ));
        }
        let candidate = directory.path().join(CANDIDATE);
        if !fs::symlink_metadata(&candidate)?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid prepared document",
            ));
        }
        if let Some(receipt) = &self.publication_receipt {
            crate::publication_receipt::stamp(&candidate, receipt)?;
        }
        if !may_publish() {
            return Ok(false);
        }
        fs::rename(candidate, &self.output)?;
        Ok(true)
    }
}

/// 親なしのlibrary呼出は従来経路を使い、内部監督では保存領域の所有通知を待つ。
pub(crate) fn request(cancellation: &ParseCancellation) -> Result<Option<Workspace>, WorkerError> {
    let Some(workspace) = std::env::var_os(workspace::ENV) else {
        return Ok(None);
    };
    worker::check_interruption(cancellation)?;
    let workspace = PathBuf::from(workspace);
    fs::write(workspace.join(REQUEST), b"")?;
    wait_for_grant(&workspace, cancellation).map(Some)
}

/// 同期済み候補を通知し、親が公開または停止するまで補助回収の所有権を保つ。
/// 親が死亡した場合はlease由来の停止で戻り、呼出元のguardが候補を削除する。
pub(crate) fn prepared(
    directory: &Path,
    cancellation: &ParseCancellation,
) -> Result<(), WorkerError> {
    worker::check_interruption(cancellation)?;
    let temporary = directory.join("commit-prepared.tmp");
    fs::write(&temporary, PREPARED_CONTENT)?;
    fs::rename(temporary, directory.join(PREPARED))?;
    #[cfg(test)]
    crate::worker::publication_checkpoint();
    loop {
        worker::check_interruption(cancellation)?;
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 親が通知した保存領域を取得するまで、lease由来の停止を確認する。
fn wait_for_grant(
    workspace: &Path,
    cancellation: &ParseCancellation,
) -> Result<Workspace, WorkerError> {
    let grant = workspace.join(GRANT);
    loop {
        // 親のDropが失われても、通知済みの領域は停止より先に回収所有権を取得する。
        match fs::File::open(&grant) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(129).read_to_end(&mut bytes)?;
                let basename = validate_basename(&bytes)?;
                return Ok(Workspace::recover_registered_commit(
                    &workspace.join(basename),
                    workspace,
                )?);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if cancellation.is_cancelled() {
                    recover_pending(workspace)?;
                }
                worker::check_interruption(cancellation)?;
            }
            Err(error) => return Err(error.into()),
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// basenameだけを完全な記録として公開し、不完全な書込を監督に読ませない。
fn publish_name(workspace: &Path, name: &str, directory: &Path) -> io::Result<()> {
    let basename = directory.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing commit directory name")
    })?;
    validate_basename(basename.as_encoded_bytes())?;
    let temporary = workspace.join(format!("{name}.tmp"));
    fs::write(&temporary, basename.as_encoded_bytes())?;
    fs::rename(temporary, workspace.join(name))
}

/// 親切断後、作成予定のうちjob固有markerが完成しているdirectoryだけを回収する。
fn recover_pending(workspace: &Path) -> io::Result<()> {
    let file = match fs::File::open(workspace.join(INTENT)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(129).read_to_end(&mut bytes)?;
    let basename = validate_basename(&bytes)?;
    let path = workspace.join(basename);
    // 未作成なら削除しない。既存directoryのmarker欠落は安全な回収を証明できない。
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    drop(Workspace::recover_registered_commit(&path, workspace)?);
    Ok(())
}

/// 通知は短いASCII basenameだけに限定し、所有済みworkspace以外の回収を防ぐ。
fn validate_basename(bytes: &[u8]) -> io::Result<&str> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid commit directory grant");
    let name = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    if bytes.len() > 128
        || !name.starts_with(workspace::COMMIT_PREFIX)
        || name.len() == workspace::COMMIT_PREFIX.len()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(invalid());
    }
    Ok(name)
}

/// bare filenameの場合も、親と監督で同じ保存基点を使用する。
fn output_parent(output: &Path) -> &Path {
    output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// 実プロセスfixtureだけが、所有領域作成後・通知前の死亡を再現する。
#[cfg(test)]
fn allocation_checkpoint() {
    let Some(path) = std::env::var_os("JADE_PDF2DOCX_TEST_COMMIT_ALLOCATION_CHECKPOINT") else {
        return;
    };
    let path = PathBuf::from(path);
    fs::write(&path, b"").unwrap();
    while !path.with_extension("continue").exists() {
        std::thread::sleep(Duration::from_millis(10));
    }
}
