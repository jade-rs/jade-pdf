use std::path::{Path, PathBuf};

/// `ndlocr-lite-rs` の公開 GitHub リポジトリ URL。
pub const NDL_OCR_LITE_RS_GITHUB_URL: &str = "https://github.com/bokuweb/ndlocr-lite-rs";

/// リポジトリルートからデバッグ用 `ndlocr-lite-rs` checkout への相対パス。
pub const NDL_OCR_LITE_RS_RELATIVE_PATH_FROM_REPO_ROOT: &str = "../../bokuweb/ndlocr-lite-rs";

/// デバッグ用 `ndlocr-lite-rs` checkout の想定ワークスペースパスを解決する。
pub fn resolve_ndlocr_workspace_path(repo_root: &Path) -> PathBuf {
    repo_root.join(NDL_OCR_LITE_RS_RELATIVE_PATH_FROM_REPO_ROOT)
}
