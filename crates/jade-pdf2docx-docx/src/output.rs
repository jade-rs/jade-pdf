//! 入力ファイル保護と、完成した DOCX の atomic な保存を担当する。
use crate::{ConvertError, conversion_control::Stage};
use std::path::Path;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "output_reader_tests.rs"]
mod reader_tests;

/// 別名・symlink・hardlink を含め、入力と出力が同じファイルなら拒否する。
#[cfg(not(target_arch = "wasm32"))]
pub fn validate_distinct_paths(input: &Path, output: &Path) -> Result<(), ConvertError> {
    if output.try_exists()? {
        if paths_share_file(input, output)? {
            return Err(ConvertError::OutputAliasesInput);
        }
        return Ok(());
    }
    // mkdirで未作成成分が現れた後に成立するaliasも、directory作成前に拒否する。
    if let Some(name) = output.file_name() {
        let future = crate::resolve_output_parent(output)?.join(name);
        if future.try_exists()? && paths_share_file(input, &future)? {
            return Err(ConvertError::OutputAliasesInput);
        }
    }
    Ok(())
}

/// UnixではopenによるFIFO停止を避け、symlinkを解決したmetadataの識別子を比較する。
#[cfg(unix)]
fn paths_share_file(input: &Path, output: &Path) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let input = std::fs::metadata(input)?;
    let output = std::fs::metadata(output)?;
    Ok(input.dev() == output.dev() && input.ino() == output.ino())
}

/// Unix以外では従来のOS別ファイル識別を維持する。
#[cfg(all(not(unix), not(target_arch = "wasm32")))]
fn paths_share_file(input: &Path, output: &Path) -> std::io::Result<bool> {
    same_file::is_same_file(input, output)
}

/// 同じディレクトリの一時ファイルを同期し、完成後に出力のディレクトリエントリを置換する。
///
/// 途中で失敗した一時ファイルは自動削除され、既存出力は保持される。
/// symlink/hardlink のリンク先は上書きしない。ディレクトリの永続化までは保証しない。
/// 書込開始前・chunkごと・置換直前に停止を確認する。作成済みの親directoryは残る。
/// 最後の確認後は置換を成功として扱い、公開後にキャンセルエラーを返さない。
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_atomic(
    output: &Path,
    bytes: &[u8],
    check: impl FnMut(Stage) -> Result<(), ConvertError>,
) -> Result<(), ConvertError> {
    write_atomic_from_reader(output, std::io::Cursor::new(bytes), check)
}

/// worker成果物を固定bufferでコピーし、既存出力を保持したままatomicに公開する。
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_atomic_from_reader(
    output: &Path,
    source: impl std::io::Read,
    check: impl FnMut(Stage) -> Result<(), ConvertError>,
) -> Result<(), ConvertError> {
    write_atomic_from_reader_in(output, None, source, check)
}

/// 呼出元が回収を所有する同一filesystemの領域があれば、保存用一時ファイルを置く。
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_atomic_from_reader_in(
    output: &Path,
    temporary_directory: Option<&Path>,
    mut source: impl std::io::Read,
    mut check: impl FnMut(Stage) -> Result<(), ConvertError>,
) -> Result<(), ConvertError> {
    use std::io::Write;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    check(Stage::BeforeWrite)?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(temporary_directory.unwrap_or(parent))?;
    let mut buffer = [0; 8192];
    loop {
        let length = match source.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                check(Stage::Writing)?;
                continue;
            }
            result => result?,
        };
        if length == 0 {
            break;
        }
        file.write_all(&buffer[..length])?;
        check(Stage::Writing)?;
    }
    file.as_file().sync_all()?;
    check(Stage::BeforeCommit)?;
    file.persist(output)
        .map_err(|error| ConvertError::Io(error.error))?;
    Ok(())
}

/// ブラウザではファイルパス API の代わりに bytes API を使うよう明示する。
#[cfg(target_arch = "wasm32")]
pub(crate) fn validate_distinct_paths(_: &Path, _: &Path) -> Result<(), ConvertError> {
    Err(ConvertError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "use the bytes conversion API on wasm32",
    )))
}

/// ブラウザでのファイル保存は呼び出し元に委ねる。
#[cfg(target_arch = "wasm32")]
pub(crate) fn write_atomic(
    _: &Path,
    _: &[u8],
    _: impl FnMut(Stage) -> Result<(), ConvertError>,
) -> Result<(), ConvertError> {
    Err(ConvertError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "use the bytes conversion API on wasm32",
    )))
}
