#[cfg(feature = "bundled-pdfium")]
#[path = "src/pdfium_assets.rs"]
mod pdfium_assets;

fn main() -> anyhow::Result<()> {
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        return Ok(());
    }
    for variable in [
        "PDFIUM_BUNDLE_LIB",
        "PDFIUM_BUILD_CACHE_DIR",
        "JADE_PDFIUM_CACHE_DIR",
        "PDFIUM_AUTO_CACHE_DIR",
        "PDFIUM_NO_AUTO_DOWNLOAD",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    println!("cargo:rerun-if-changed=src/pdfium_asset_pins.rs");
    println!("cargo:rerun-if-changed=src/pdfium_assets.rs");
    #[cfg(feature = "bundled-pdfium")]
    {
        use anyhow::Context;
        use std::path::PathBuf;
        let (source, expected) = if let Some(path) = std::env::var_os("PDFIUM_BUNDLE_LIB") {
            (
                pdfium_assets::custom_library(std::path::Path::new(&path))?,
                None,
            )
        } else {
            let asset = pdfium_assets::platform(
                &std::env::var("CARGO_CFG_TARGET_OS")?,
                &std::env::var("CARGO_CFG_TARGET_ARCH")?,
                &std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default(),
            )?;
            let cache = std::env::var_os("PDFIUM_BUILD_CACHE_DIR")
                .map(PathBuf::from)
                .map(Ok)
                .unwrap_or_else(pdfium_assets::cache_root)?;
            (
                pdfium_assets::ensure_asset(
                    asset,
                    &cache,
                    std::env::var_os("PDFIUM_NO_AUTO_DOWNLOAD").is_none(),
                )?,
                Some((asset.library_bytes, asset.library_sha256)),
            )
        };
        println!("cargo:rerun-if-changed={}", source.display());
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(&source)?
            .take(pdfium_assets::MAX_LIBRARY_BYTES + 1)
            .read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes.len() as u64 <= pdfium_assets::MAX_LIBRARY_BYTES,
            "Bundled SDK exceeds the library size limit"
        );
        if let Some((size, hash)) = expected {
            anyhow::ensure!(
                bytes.len() as u64 == size && pdfium_assets::digest(&bytes) == hash,
                "Bundled SDK changed after cache verification"
            );
        }
        let out = PathBuf::from(std::env::var_os("OUT_DIR").context("OUT_DIR is missing")?);
        std::fs::write(out.join("pdfium-library"), &bytes)?;
        let name = match std::env::var("CARGO_CFG_TARGET_OS")?.as_str() {
            "windows" => "pdfium.dll",
            "macos" => "libpdfium.dylib",
            _ => "libpdfium.so",
        };
        std::fs::write(out.join("pdfium_bundle.rs"), format!("const BUNDLED_BYTES: &[u8] = include_bytes!(\"pdfium-library\");\nconst BUNDLED_SHA256: &str = {:?};\nconst BUNDLED_NAME: &str = {name:?};\n", pdfium_assets::digest(&bytes)))?;
    }
    Ok(())
}
