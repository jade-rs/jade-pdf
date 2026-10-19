//! 公開前のDOCXへ要求照合情報を含め、公開後の停止でも同じ成果物を識別する。
use std::{
    fs::OpenOptions,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

/// ZIPの最大コメントと固定EOCDを含む、末尾読取の上限。
const TAIL_LIMIT: u64 = 65_535 + 22;

/// 要求キーとfingerprintを固定長の小文字hexだけに限定する。
pub(crate) fn valid(value: &str) -> bool {
    value.len() == 97
        && value.as_bytes()[32] == b':'
        && value
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 32 || b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 固定上限のZIP末尾だけを読み、私有候補のコメントを置換して同期する。
/// 本文entryは変更せず、失敗した候補は最終pathへ公開しない。
pub(crate) fn stamp(path: &Path, receipt: &str) -> io::Result<()> {
    if !valid(receipt) {
        return Err(invalid());
    }
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(TAIL_LIMIT);
    file.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::new();
    Read::by_ref(&mut file)
        .take(TAIL_LIMIT)
        .read_to_end(&mut tail)?;
    let offset = tail
        .windows(22)
        .enumerate()
        .rev()
        .find_map(|(offset, footer)| {
            let comment_size = u16::from_le_bytes([footer[20], footer[21]]) as usize;
            let central_size = u32::from_le_bytes(footer[12..16].try_into().unwrap()) as u64;
            let central_start = u32::from_le_bytes(footer[16..20].try_into().unwrap()) as u64;
            (footer[..4] == *b"PK\x05\x06"
                && offset + 22 + comment_size == tail.len()
                && footer[4..8] == [0; 4]
                && footer[8..10] == footer[10..12]
                && footer[8..10] != [0; 2]
                && central_start + central_size == start + offset as u64)
                .then_some(offset)
        })
        .ok_or_else(invalid)?;
    let comment = format!("JADE-PDF2DOCX-PUBLICATION-1\n{receipt}");
    file.seek(SeekFrom::Start(start + offset as u64 + 20))?;
    file.write_all(&(comment.len() as u16).to_le_bytes())?;
    file.write_all(comment.as_bytes())?;
    file.set_len(start + offset as u64 + 22 + comment.len() as u64)?;
    file.sync_all()
}

/// 不完全なZIPを通常の保存失敗として報告する。
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid publication receipt or DOCX footer",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 小さな有効ZIPを作り、コメントの置換が本文byteへ影響しないことを検証する。
    fn document(path: &Path, comment: &str) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        zip.start_file("word/document.xml", zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(b"<document>unchanged</document>").unwrap();
        zip.set_comment(comment);
        zip.finish().unwrap();
    }

    /// 最大長の既存コメントも固定上限で置換でき、再実行でも同じ本文を維持する。
    #[test]
    fn replaces_bounded_comment_and_rejects_invalid_document() {
        let base = tempfile::tempdir().unwrap();
        let output = base.path().join("document.docx");
        let receipt = format!("{}:{}", "a".repeat(32), "b".repeat(64));
        for comment in ["x".repeat(65535), "embedded PK\x05\x06".to_owned()] {
            document(&output, &comment);
            stamp(&output, &receipt).unwrap();
            stamp(&output, &receipt).unwrap();
            let mut zip = zip::ZipArchive::new(std::fs::File::open(&output).unwrap()).unwrap();
            assert_eq!(
                zip.comment(),
                format!("JADE-PDF2DOCX-PUBLICATION-1\n{receipt}").as_bytes()
            );
            let mut content = String::new();
            zip.by_name("word/document.xml")
                .unwrap()
                .read_to_string(&mut content)
                .unwrap();
            assert_eq!(content, "<document>unchanged</document>");
        }
        std::fs::write(&output, b"not a ZIP").unwrap();
        assert!(stamp(&output, &receipt).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"not a ZIP");
    }

    /// 本番と同じ公開関数を呼び、rename直後に親プロセスのSIGKILLを待つ。
    #[cfg(unix)]
    #[test]
    fn published_fixture() {
        let Some(base) = std::env::var_os("JADE_PDF2DOCX_TEST_RECEIPT_BASE") else {
            return;
        };
        let base = Path::new(&base);
        let mut owner = crate::commit_space::CommitOwner::new(base, &base.join("output.docx"));
        owner.publication_receipt = Some(format!("{}:{}", "a".repeat(32), "b".repeat(64)));
        std::fs::write(base.join("commit-request"), b"").unwrap();
        owner.poll().unwrap();
        let directory = base.join(std::fs::read_to_string(base.join("commit-grant")).unwrap());
        document(&directory.join("document.docx"), "");
        std::fs::write(
            directory.join("commit-prepared"),
            b"JADE-PDF2DOCX-PREPARED-1\n",
        )
        .unwrap();
        assert!(owner.publish_prepared(|| true).unwrap());
        std::fs::write(base.join("ready"), b"").unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// 照合情報の保存失敗や公開期限切れでは、既存の最終出力を保持する。
    #[test]
    fn invalid_candidate_and_cancellation_preserve_previous_output() {
        let base = tempfile::tempdir().unwrap();
        let output = base.path().join("output.docx");
        std::fs::write(&output, b"previous output").unwrap();
        let mut owner = crate::commit_space::CommitOwner::new(base.path(), &output);
        owner.publication_receipt = Some(format!("{}:{}", "a".repeat(32), "b".repeat(64)));
        std::fs::write(base.path().join("commit-request"), b"").unwrap();
        owner.poll().unwrap();
        let directory = base
            .path()
            .join(std::fs::read_to_string(base.path().join("commit-grant")).unwrap());
        let candidate = directory.join("document.docx");
        std::fs::write(&candidate, b"invalid DOCX").unwrap();
        std::fs::write(
            directory.join("commit-prepared"),
            b"JADE-PDF2DOCX-PREPARED-1\n",
        )
        .unwrap();
        assert!(
            owner
                .publish_prepared(|| panic!("must fail before publication"))
                .is_err()
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"previous output");
        document(&candidate, "");
        assert!(!owner.publish_prepared(|| false).unwrap());
        assert_eq!(std::fs::read(&output).unwrap(), b"previous output");
        assert!(owner.publish_prepared(|| true).unwrap());
        assert_ne!(std::fs::read(&output).unwrap(), b"previous output");
    }

    /// 親のDropも終了記録もない実SIGKILL後に、元要求と公開済み成果物を照合できる。
    #[cfg(unix)]
    #[test]
    fn published_receipt_survives_parent_sigkill() {
        let base = tempfile::tempdir().unwrap();
        std::fs::write(base.path().join("output.docx"), b"previous output").unwrap();
        let request = base.path().join("request-identity");
        let identity = format!("{}:{}", "a".repeat(32), "b".repeat(64));
        std::fs::write(&request, &identity).unwrap();
        let mut child = crate::worker::ChildGuard::spawn(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "publication_receipt::tests::published_fixture",
                    "--nocapture",
                ])
                .env("JADE_PDF2DOCX_TEST_RECEIPT_BASE", base.path())
                .stdout(std::process::Stdio::null()),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !base.path().join("ready").exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            base.path().join("ready").exists(),
            "publication was not observed"
        );
        child.child.kill().unwrap();
        assert!(!child.child.wait().unwrap().success());
        let zip =
            zip::ZipArchive::new(std::fs::File::open(base.path().join("output.docx")).unwrap())
                .unwrap();
        assert_eq!(
            zip.comment(),
            format!("JADE-PDF2DOCX-PUBLICATION-1\n{identity}").as_bytes()
        );
        assert_eq!(std::fs::read_to_string(request).unwrap(), identity);
    }
}
