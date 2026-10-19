//! 出力directoryを作成する前に、将来の配置と入力alias検査の基点を解決する。
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

/// 未作成の出力親を作らず、既存リンクと親参照を解決した絶対パスを返す。
/// 保存先と同じfilesystemへの配置や、directory作成後に成立する入力alias検査に使う。
/// 呼出後のfilesystem変更を固定するものではなく、返すdirectoryが未作成の場合もある。
pub fn resolve_output_parent(output: &Path) -> io::Result<PathBuf> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    resolve_future_directory(parent)
}

/// 既存リンクを実体へ解決し、未作成成分の後の親参照も将来の保存位置へ反映する。
/// 未作成directoryは作らず、存在する成分の権限エラー等は呼出元へ返す。
fn resolve_future_directory(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            Component::Normal(_) => {
                resolved.push(component.as_os_str());
                match fs::canonicalize(&resolved) {
                    Ok(path) => resolved = path,
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
            Component::Prefix(_) | Component::RootDir => resolved.push(component.as_os_str()),
        }
    }
    Ok(resolved)
}
