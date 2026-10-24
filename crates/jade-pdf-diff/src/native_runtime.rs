//! Pin native loading and embedding to the same verified SDK payload.
use anyhow::Result;
use std::path::PathBuf;

use crate::pdfium_assets as assets;

#[cfg(feature = "bundled-pdfium")]
include!(concat!(env!("OUT_DIR"), "/pdfium_bundle.rs"));

/// Resolve the library used by native PDF APIs. The default SDK is checksum-pinned.
/// `PDFIUM_LIB_PATH` explicitly selects a caller-provided library instead.
pub fn pdfium_library_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("PDFIUM_LIB_PATH") {
        return assets::custom_library(std::path::Path::new(&path));
    }
    #[cfg(feature = "bundled-pdfium")]
    {
        let root = assets::cache_root()?;
        if let Some(target) = assets::verified_cached(
            &root,
            BUNDLED_SHA256,
            BUNDLED_NAME,
            BUNDLED_BYTES.len() as u64,
        )? {
            return Ok(target);
        }
        let (directory, _lock) =
            assets::locked_directory(&root, BUNDLED_SHA256, std::time::Duration::from_secs(75))?;
        let target = directory.join(BUNDLED_NAME);
        if assets::file_matches(&target, BUNDLED_BYTES.len() as u64, BUNDLED_SHA256)? {
            return Ok(target);
        }
        assets::publish(&directory, BUNDLED_NAME, BUNDLED_BYTES)
    }
    #[cfg(not(feature = "bundled-pdfium"))]
    {
        let asset = assets::platform(
            std::env::consts::OS,
            std::env::consts::ARCH,
            if cfg!(target_env = "musl") {
                "musl"
            } else {
                ""
            },
        )?;
        assets::ensure_asset(
            asset,
            &assets::cache_root()?,
            std::env::var_os("PDFIUM_NO_AUTO_DOWNLOAD").is_none(),
        )
    }
}
