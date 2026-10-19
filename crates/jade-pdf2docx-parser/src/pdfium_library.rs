//! 明示されたnative libraryを自動downloadへ切り替えずに読み込む。
use std::path::Path;

use pdfium_render::prelude::Pdfium;

use crate::ParseError;

/// 明示pathの不備はそのまま返し、未指定の場合だけ従来の自動取得を利用する。
pub(super) fn bind(input: &Path) -> Result<Pdfium, ParseError> {
    match std::env::var_os("PDFIUM_LIB_PATH") {
        Some(value) => {
            let library = Path::new(&value);
            if value.is_empty() || !library.is_file() {
                return Err(super::pdfium_error(
                    input,
                    "PDFIUM_LIB_PATH must point to an existing regular library file",
                ));
            }
            pdfium_auto::bind_pdfium_from_path(library)
                .map_err(|error| super::pdfium_error(input, format!("PDFIUM_LIB_PATH: {error}")))
        }
        None => {
            pdfium_auto::bind_pdfium_silent().map_err(|error| super::pdfium_error(input, error))
        }
    }
}
