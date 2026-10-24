//! Shared, bounded SDK provisioning for build-time embedding and native loading.
use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct Asset {
    pub os: &'static str,
    pub arch: &'static str,
    pub archive: &'static str,
    pub archive_bytes: u64,
    pub archive_sha256: &'static str,
    pub member: &'static str,
    pub library_bytes: u64,
    pub library_sha256: &'static str,
}

include!("pdfium_asset_pins.rs");
pub const MAX_LIBRARY_BYTES: u64 = 128 * 1024 * 1024;

pub fn platform(os: &str, arch: &str, target_env: &str) -> Result<&'static Asset> {
    ensure!(
        os != "linux" || target_env != "musl",
        "Default Pdfium requires glibc on Linux; provide PDFIUM_LIB_PATH or PDFIUM_BUNDLE_LIB for musl"
    );
    ASSETS
        .iter()
        .find(|a| a.os == os && a.arch == arch)
        .context("Unsupported default Pdfium platform; provide a custom library")
}

pub fn cache_root() -> Result<PathBuf> {
    for name in ["JADE_PDFIUM_CACHE_DIR", "PDFIUM_AUTO_CACHE_DIR"] {
        if let Some(value) = std::env::var_os(name) {
            ensure!(!value.is_empty(), "{name} must not be empty");
            return Ok(value.into());
        }
    }
    Ok(dirs::cache_dir()
        .context("No user cache directory available")?
        .join("jade-pdf"))
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn custom_library(path: &Path) -> Result<PathBuf> {
    let metadata = path
        .metadata()
        .with_context(|| format!("Cannot open Pdfium library {}", path.display()))?;
    ensure!(
        metadata.is_file() && (100_000..=MAX_LIBRARY_BYTES).contains(&metadata.len()),
        "Invalid custom Pdfium library size or file type"
    );
    Ok(path.to_owned())
}

pub fn file_matches(path: &Path, bytes: u64, expected: &str) -> Result<bool> {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    // Cache entries must be regular files owned by this cache, not link targets.
    if !metadata.is_file() || metadata.len() != bytes || bytes > MAX_LIBRARY_BYTES {
        return Ok(false);
    }
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let read = std::io::copy(&mut Read::by_ref(&mut file).take(bytes + 1), &mut hash)?;
    Ok(read == bytes && format!("{:x}", hash.finalize()) == expected)
}

#[derive(Debug)]
pub struct CacheLock {
    file: File,
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        // Closing one descriptor does not release a flock while a duplicate
        // survives, including the fork-to-exec window of an unrelated child.
        // End this owner's critical section explicitly before closing it.
        let _ = self.file.unlock();
    }
}

fn cache_directory(root: &Path, key: &str) -> Result<PathBuf> {
    ensure!(
        key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid Pdfium cache key"
    );
    Ok(root.join(format!("pdfium-{PDFIUM_VERSION}")).join(key))
}

/// A valid installed payload is read-only and can be checksum-verified without
/// joining the installer queue. Every lookup still hashes the complete bytes.
/// Atomic publishers only replace it with the same pinned payload. The locked
/// path rechecks after waiting whenever the first verification does not match.
pub fn verified_cached(root: &Path, key: &str, name: &str, bytes: u64) -> Result<Option<PathBuf>> {
    ensure!(
        Path::new(name).file_name() == Some(std::ffi::OsStr::new(name)),
        "Invalid SDK cache filename"
    );
    let target = cache_directory(root, key)?.join(name);
    Ok(file_matches(&target, bytes, key)?.then_some(target))
}

pub fn locked_directory(root: &Path, key: &str, timeout: Duration) -> Result<(PathBuf, CacheLock)> {
    let directory = cache_directory(root, key)?;
    std::fs::create_dir_all(&directory)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("install.lock"))?;
    let start = Instant::now();
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok((directory, CacheLock { file: lock })),
            Err(TryLockError::WouldBlock) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(TryLockError::WouldBlock) => {
                bail!("Timed out waiting for Pdfium cache installation")
            }
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
    }
}

pub fn publish(directory: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf> {
    ensure!(
        Path::new(name).file_name() == Some(std::ffi::OsStr::new(name)),
        "Invalid SDK cache filename"
    );
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    let target = directory.join(name);
    temporary.persist(&target).map_err(|e| e.error)?;
    Ok(target)
}

fn download(asset: &Asset) -> Result<Vec<u8>> {
    ensure!(
        asset.archive_bytes <= 64 * 1024 * 1024,
        "SDK archive exceeds download limit"
    );
    let url = format!("https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F{PDFIUM_VERSION}/{}", asset.archive);
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(concat!("jade-pdf-diff/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client.get(url).send()?.error_for_status()?;
    if let Some(bytes) = response.content_length() {
        ensure!(bytes == asset.archive_bytes, "Unexpected SDK download size");
    }
    let mut bytes = Vec::new();
    response
        .take(asset.archive_bytes + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == asset.archive_bytes && digest(&bytes) == asset.archive_sha256,
        "SDK archive checksum or size mismatch"
    );
    Ok(bytes)
}

fn extract(
    archive: &[u8],
    member: &str,
    expected_size: u64,
    expected_hash: &str,
) -> Result<Vec<u8>> {
    ensure!(
        expected_size <= MAX_LIBRARY_BYTES,
        "SDK library exceeds extraction limit"
    );
    let decoder = flate2::read::GzDecoder::new(archive);
    let mut archive = tar::Archive::new(decoder.take(MAX_LIBRARY_BYTES));
    let mut library = None;
    for (index, entry) in archive.entries()?.enumerate() {
        ensure!(index < 10_000, "SDK archive has too many entries");
        let mut entry = entry?;
        if entry.path()?.as_ref() == Path::new(member) {
            ensure!(
                library.is_none()
                    && entry.header().entry_type().is_file()
                    && entry.size() == expected_size,
                "Invalid SDK library entry"
            );
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(expected_size + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 == expected_size && digest(&bytes) == expected_hash,
                "SDK library checksum or size mismatch"
            );
            library = Some(bytes);
        }
    }
    library.context("SDK library missing from archive")
}

pub fn ensure_asset(asset: &Asset, root: &Path, allow_download: bool) -> Result<PathBuf> {
    let name = Path::new(asset.member)
        .file_name()
        .context("Missing SDK library name")?
        .to_str()
        .context("Invalid SDK library name")?;
    if let Some(target) = verified_cached(root, asset.library_sha256, name, asset.library_bytes)? {
        return Ok(target);
    }
    let (directory, _lock) = locked_directory(root, asset.library_sha256, Duration::from_secs(75))?;
    let target = directory.join(name);
    if file_matches(&target, asset.library_bytes, asset.library_sha256)? {
        return Ok(target);
    }
    ensure!(
        allow_download,
        "No verified cached Pdfium {PDFIUM_VERSION}; automatic download is disabled"
    );
    let bytes = download(asset)?;
    let library = extract(
        &bytes,
        asset.member,
        asset.library_bytes,
        asset.library_sha256,
    )?;
    publish(&directory, name, &library)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (name, kind, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_entry_type(*kind);
            header.set_size(bytes.len() as u64);
            if kind.is_symlink() {
                header.set_link_name("elsewhere").unwrap();
            }
            header.set_cksum();
            archive.append_data(&mut header, name, *bytes).unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn sdk_extraction_rejects_links_duplicates_corruption_and_wrong_sizes() {
        let data = b"verified library";
        let hash = digest(data);
        let good = ("lib/pdfium", tar::EntryType::Regular, data.as_slice());
        assert_eq!(
            extract(&archive(&[good]), "lib/pdfium", data.len() as u64, &hash).unwrap(),
            data
        );
        for entries in [
            vec![("lib/pdfium", tar::EntryType::Symlink, b"".as_slice())],
            vec![good, good],
            vec![("other", tar::EntryType::Regular, data.as_slice())],
        ] {
            assert!(extract(&archive(&entries), "lib/pdfium", data.len() as u64, &hash).is_err());
        }
        let bytes = archive(&[good]);
        assert!(extract(&bytes, "lib/pdfium", data.len() as u64 + 1, &hash).is_err());
        assert!(extract(&bytes, "lib/pdfium", data.len() as u64, &digest(b"wrong")).is_err());
        assert!(extract(&bytes, "lib/pdfium", MAX_LIBRARY_BYTES + 1, &hash).is_err());
    }

    #[test]
    fn verified_warm_reads_do_not_wait_for_the_installer_and_still_reject_corruption() {
        const BYTES: &[u8] = b"verified warm cache";
        const HASH: &str = "1591c3bdd2b1eb013fc78b033bc007945086e328f10f085e151b26c86c6cd26b";
        assert_eq!(digest(BYTES), HASH);
        let asset = Asset {
            os: "test",
            arch: "test",
            archive: "",
            archive_bytes: 0,
            archive_sha256: HASH,
            member: "lib/library",
            library_bytes: BYTES.len() as u64,
            library_sha256: HASH,
        };
        let root = tempfile::tempdir().unwrap();
        let (directory, lock) = locked_directory(root.path(), HASH, Duration::ZERO).unwrap();
        let target = publish(&directory, "library", BYTES).unwrap();
        let path = root.path().to_owned();
        let (sender, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            sender.send(ensure_asset(&asset, &path, false)).unwrap();
        });
        let observed = receiver.recv_timeout(Duration::from_secs(2));
        // Drop before joining even on failure, so an old implementation is a
        // bounded two-second regression failure rather than a 75-second hang.
        drop(lock);
        thread.join().unwrap();
        assert_eq!(
            observed
                .expect("Verified warm read waited for installer")
                .unwrap(),
            target
        );

        let (_, lock) = locked_directory(root.path(), HASH, Duration::ZERO).unwrap();
        std::fs::write(&target, vec![0u8; BYTES.len()]).unwrap();
        assert!(
            verified_cached(root.path(), HASH, "library", BYTES.len() as u64)
                .unwrap()
                .is_none()
        );
        drop(lock);
        let invalid = Asset {
            os: "test",
            arch: "test",
            archive: "",
            archive_bytes: 0,
            archive_sha256: HASH,
            member: "lib/library",
            library_bytes: BYTES.len() as u64,
            library_sha256: HASH,
        };
        assert!(ensure_asset(&invalid, root.path(), false)
            .unwrap_err()
            .to_string()
            .contains("disabled"));
        assert_eq!(std::fs::read(&target).unwrap(), vec![0u8; BYTES.len()]);
    }

    #[test]
    fn cache_rejects_same_size_corruption_and_preserves_it_when_offline() {
        let root = tempfile::tempdir().unwrap();
        let asset = &ASSETS[0];
        let (directory, lock) =
            locked_directory(root.path(), asset.library_sha256, Duration::ZERO).unwrap();
        let target = directory.join(Path::new(asset.member).file_name().unwrap());
        File::create(&target)
            .unwrap()
            .set_len(asset.library_bytes)
            .unwrap();
        drop(lock);
        assert!(!file_matches(&target, asset.library_bytes, asset.library_sha256).unwrap());
        assert!(ensure_asset(asset, root.path(), false)
            .unwrap_err()
            .to_string()
            .contains("disabled"));
        assert_eq!(target.metadata().unwrap().len(), asset.library_bytes);
    }

    #[test]
    fn cache_owner_releases_its_lock_while_a_duplicate_is_still_open() {
        let root = tempfile::tempdir().unwrap();
        let hash = digest(b"cache owner");
        let (_, lock) = locked_directory(root.path(), &hash, Duration::ZERO).unwrap();
        let duplicate = lock.file.try_clone().unwrap();
        drop(lock);
        let (_, next) = locked_directory(root.path(), &hash, Duration::ZERO)
            .expect("A duplicate must not extend the cache owner's critical section");
        drop(duplicate);
        assert!(locked_directory(root.path(), &hash, Duration::ZERO).is_err());
        drop(next);
        locked_directory(root.path(), &hash, Duration::ZERO).unwrap();
    }

    #[test]
    fn cache_lock_timeout_and_atomic_repair_do_not_accept_partial_libraries() {
        let root = tempfile::tempdir().unwrap();
        let payload = b"complete payload";
        let hash = digest(payload);
        let (directory, lock) = locked_directory(root.path(), &hash, Duration::ZERO).unwrap();
        assert!(locked_directory(root.path(), &hash, Duration::ZERO)
            .unwrap_err()
            .to_string()
            .contains("Timed out"));
        let target = directory.join("pdfium");
        std::fs::write(&target, b"partial").unwrap();
        publish(&directory, "pdfium", payload).unwrap();
        assert!(file_matches(&target, payload.len() as u64, &hash).unwrap());
        assert!(publish(&directory, "../escape", payload).is_err());
        drop(lock);
        locked_directory(root.path(), &hash, Duration::ZERO)
            .expect("Cache lock must be available immediately after its owner is dropped");
    }
}
