//! PP-Structure / TableSystem 相当の外部プロセスが stdout に返す JSON を解釈する。
//!
//! 契約: プロセスの標準出力が UTF-8 JSON で、少なくとも `{ "html": "<table>...</table>" }` を含む。
//! 任意で `cell_bboxes` を付与できる（将来の幾何マッチ用）。

use std::{
    ffi::OsStr,
    fmt::{Display, Formatter},
    path::Path,
    process::Command,
};

/// 外部表パイプラインの標準出力 JSON。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct TablePipelineOutput {
    /// 表を含む HTML 断片（通常 `<table>...</table>`）。
    pub html: String,
    /// セル bbox 等の任意メタデータ（形式はパイプライン依存）。
    #[serde(default)]
    pub cell_bboxes: Option<Vec<serde_json::Value>>,
}

/// 表パイプライン実行・JSON 解釈のエラー。
#[derive(Debug)]
pub enum TablePipelineError {
    /// JSON として解釈できない。
    Json(serde_json::Error),
    /// プロセス起動・読み取り失敗。
    Io(std::io::Error),
    /// プロセスが非ゼロ終了。
    ExecutableFailed {
        /// 終了コード。
        code: Option<i32>,
        /// 標準エラー相当。
        stderr: String,
    },
}

impl Display for TablePipelineError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TablePipelineError::Json(err) => write!(f, "table pipeline json: {err}"),
            TablePipelineError::Io(err) => write!(f, "table pipeline io: {err}"),
            TablePipelineError::ExecutableFailed { code, stderr } => {
                write!(f, "table pipeline failed (code={code:?}): {stderr}")
            }
        }
    }
}

impl std::error::Error for TablePipelineError {}

impl From<serde_json::Error> for TablePipelineError {
    fn from(value: serde_json::Error) -> Self {
        TablePipelineError::Json(value)
    }
}

impl From<std::io::Error> for TablePipelineError {
    fn from(value: std::io::Error) -> Self {
        TablePipelineError::Io(value)
    }
}

/// JSON 文字列を `TablePipelineOutput` にデコードする。
pub fn parse_table_pipeline_json(text: &str) -> Result<TablePipelineOutput, TablePipelineError> {
    serde_json::from_str(text.trim()).map_err(TablePipelineError::from)
}

/// 外部コマンドを `program + extra_args + image_path` で実行し、stdout を JSON として解釈する。
///
/// `image_path` は **最後の引数**として付与する（多くの CLI が入力画像を末尾に取る想定）。
pub fn run_table_pipeline_command(
    program: &Path,
    extra_args: &[&OsStr],
    image_path: &Path,
) -> Result<TablePipelineOutput, TablePipelineError> {
    let output = Command::new(program)
        .args(extra_args.iter().copied())
        .arg(image_path)
        .output()?;
    if !output.status.success() {
        return Err(TablePipelineError::ExecutableFailed {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_table_pipeline_json(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_minimal_json() {
        let j = r#"{"html":"<table><tr><td>a</td></tr></table>"}"#;
        let out = parse_table_pipeline_json(j).expect("parse");
        assert!(out.html.contains("<table"));
        assert!(out.cell_bboxes.is_none());
    }

    #[test]
    fn parse_accepts_cell_bboxes_array() {
        let j = r#"{"html":"<table></table>","cell_bboxes":[[0,0,1,1]]}"#;
        let out = parse_table_pipeline_json(j).expect("parse");
        assert!(out.cell_bboxes.is_some());
    }
}
