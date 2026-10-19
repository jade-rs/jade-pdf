//! 隔離workerの一時領域を親で所有し、正常終了後だけ完成DOCXを公開する。
use crate::{
    ConvertError, ParseCancellation, ParseOptions,
    conversion_control::{self, Stage},
    output,
};

#[cfg(test)]
#[path = "staged_cancellation_tests.rs"]
mod tests;
use std::path::{Path, PathBuf};

/// workerが生成するDOCXの一時領域と、親が公開する最終出力先。
///
/// workerの終了・回収を済ませてからcommitまたはdropすること。
/// dropではworkerの部分ファイルも削除し、commit前は最終出力を変更しない。
/// プロセスの強制終了・総メモリ制限・ZIP内容検証は呼び出し元の責務とする。
pub struct StagedDocxOutput {
    /// workerの部分ファイルを含め、drop時に回収する一時directory。
    workspace: tempfile::TempDir,
    /// 成功時にatomic置換する出力先。
    output: PathBuf,
}

impl StagedDocxOutput {
    /// 入力aliasを拒否し、出力directoryを作らずにworker領域を予約する。
    pub fn new(input: &Path, output: &Path) -> Result<Self, ConvertError> {
        output::validate_distinct_paths(input, output)?;
        let workspace = tempfile::Builder::new()
            .prefix("jade-pdf2docx-worker-")
            .tempdir()?;
        Ok(Self {
            workspace,
            output: output.to_path_buf(),
        })
    }

    /// workerが完成DOCXを保存する専用パスを返す。
    pub fn path(&self) -> PathBuf {
        self.workspace.path().join("document.docx")
    }

    /// 正常終了済みworkerの成果物を固定8 KiB bufferでatomicに公開する。
    /// 失敗時は既存出力を保持し、worker領域とcommit用一時ファイルを回収する。
    pub fn commit(self) -> Result<(), ConvertError> {
        self.commit_controlled(|_| Ok(()))
    }

    /// 保存前・8 KiB chunkごと・置換直前に停止を確認して成果物を公開する。
    /// 最終確認より後の停止要求は公開を取り消さない。実行中のOS I/Oは強制停止しない。
    pub fn commit_with_cancellation(
        self,
        cancellation: &ParseCancellation,
    ) -> Result<(), ConvertError> {
        let options = ParseOptions {
            cancellation: Some(cancellation.clone()),
            ..Default::default()
        };
        self.commit_controlled(|_| conversion_control::check(&options))
    }

    /// 別の生存プロセスが回収できる専用領域内でコピーしてatomicに公開する。
    /// 領域は出力と同一filesystem上の既存directoryとし、その所有・回収は呼出元が担う。
    /// 停止・I/O失敗時は既存出力を保持し、成功後の停止では公開を取り消さない。
    pub fn commit_in_directory_with_cancellation(
        self,
        directory: &Path,
        cancellation: &ParseCancellation,
    ) -> Result<(), ConvertError> {
        let options = ParseOptions {
            cancellation: Some(cancellation.clone()),
            ..Default::default()
        };
        self.commit_controlled_in(Some(directory), |_| conversion_control::check(&options))
    }

    /// 同期済み候補を専用領域に保存し、最終出力への置換を呼出元へ委ねる。
    /// 成功時に返す候補パスと領域の回収は呼出元が所有する。
    /// 最終出力は変更せず、停止確認・8 KiBコピー・同期は通常保存と共有する。
    pub fn prepare_in_directory_with_cancellation(
        self,
        directory: &Path,
        cancellation: &ParseCancellation,
    ) -> Result<PathBuf, ConvertError> {
        let options = ParseOptions {
            cancellation: Some(cancellation.clone()),
            ..Default::default()
        };
        conversion_control::check(&options)?;
        if !std::fs::symlink_metadata(directory)?.is_dir() {
            return Err(ConvertError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "preparation requires an existing directory",
            )));
        }
        let document = std::fs::File::open(self.path())?;
        let candidate = directory.join("document.docx");
        output::write_atomic_from_reader_in(&candidate, Some(directory), document, |_| {
            conversion_control::check(&options)
        })?;
        Ok(candidate)
    }

    /// 各保存段階の停止確認を共有し、成果物openより前にも確認する。
    fn commit_controlled(
        self,
        check: impl FnMut(Stage) -> Result<(), ConvertError>,
    ) -> Result<(), ConvertError> {
        self.commit_controlled_in(None, check)
    }

    /// 保存用領域だけを切り替え、全ての停止段階と公開契約を共有する。
    fn commit_controlled_in(
        self,
        directory: Option<&Path>,
        mut check: impl FnMut(Stage) -> Result<(), ConvertError>,
    ) -> Result<(), ConvertError> {
        check(Stage::Start)?;
        let document = std::fs::File::open(self.path())?;
        output::write_atomic_from_reader_in(&self.output, directory, document, check)
    }
}
