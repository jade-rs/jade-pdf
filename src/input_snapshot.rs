//! 要求された入力digestをworkerの専用コピーへ照合する。
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

/// 照合済みPDFと寿命を共有し、終了時にコピーを回収する。
#[derive(Debug)]
pub(crate) struct Snapshot {
    /// 監督が回収するTMPDIR配下の専用領域。
    _directory: tempfile::TempDir,
    /// 解析・描画だけへ渡す固定したPDF。
    path: PathBuf,
}

impl Snapshot {
    /// regular fileを一度だけ開き、上限付きコピーに期待digestを照合する。
    pub(crate) fn capture(source: &Path, limit: u64, expected: &str) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // FIFOをmetadata検証前に開いてもwriter待ちで止まらない。
            options.custom_flags(libc::O_NONBLOCK);
        }
        let source = options.open(source)?;
        if !source.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "verified input must be a regular file",
            ));
        }
        let directory = tempfile::Builder::new()
            .prefix("jade-pdf2docx-input-")
            .tempdir()?;
        let path = directory.path().join("input.pdf");
        let mut destination = File::create_new(&path)?;
        copy_verified(source, &mut destination, limit, expected)?;
        drop(destination);
        Ok(Self {
            _directory: directory,
            path,
        })
    }

    /// 原本のpathnameを再び開かず、このコピーを変換へ渡す。
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

/// 最大limit+1 byteだけを固定bufferでコピーし、コピーしたbyteのhashを比較する。
pub(crate) fn copy_verified(
    source: impl Read,
    destination: &mut impl Write,
    limit: u64,
    expected: &str,
) -> io::Result<()> {
    let mut source = source.take(limit.saturating_add(1));
    let mut hash = hmac_sha256::Hash::new();
    let mut count = 0_u64;
    let mut buffer = [0; 8192];
    loop {
        let length = match source.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if length == 0 {
            break;
        }
        destination.write_all(&buffer[..length])?;
        hash.update(&buffer[..length]);
        count += length as u64;
        if count > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "verified input bytes exceed parser limit",
            ));
        }
    }
    let digest: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input SHA-256 mismatch",
        ));
    }
    Ok(())
}
