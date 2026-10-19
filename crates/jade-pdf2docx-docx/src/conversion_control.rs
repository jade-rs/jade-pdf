//! PDF解析からDOCX公開直前までの協調キャンセルを共有する。
use crate::{ConvertError, ParseOptions};

/// 部分成果物を公開せずに停止できる変換段階。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// 入出力の検証前。
    Start,
    /// PDF解析の完了後。
    Parsed,
    /// 表とレイアウトの推定後。
    Layout,
    /// DOCX本文の構築後。
    Built,
    /// ZIPと埋め込みfontの梱包後。
    Packed,
    /// 出力directoryと一時ファイルの作成前。
    #[cfg(not(target_arch = "wasm32"))]
    BeforeWrite,
    /// 一時ファイルへの書込chunkの完了後。
    #[cfg(not(target_arch = "wasm32"))]
    Writing,
    /// ファイル同期後、atomicな置換の直前。
    #[cfg(not(target_arch = "wasm32"))]
    BeforeCommit,
}

/// 解析と共通の停止要求をDOCXの構築・保存にも適用する。
pub(crate) fn check(options: &ParseOptions) -> Result<(), ConvertError> {
    if options
        .cancellation
        .as_ref()
        .is_some_and(|token| token.is_cancelled())
    {
        Err(ConvertError::Parse(
            jade_pdf2docx_parser::ParseError::Cancelled,
        ))
    } else {
        Ok(())
    }
}
