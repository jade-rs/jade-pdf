//! 上限付きsnapshotと外部SHA-256 vectorの回帰テスト。
use crate::input_snapshot::{Snapshot, copy_verified};
use std::io::{self, Read};

/// 公開SHA-256 test vectorのabc入力。
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// 固定長bufferをまたぐ既知vectorを照合する。
#[test]
fn known_hash_vectors_and_exact_limits() {
    for (bytes, digest) in [
        (
            Vec::new(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (b"abc".to_vec(), ABC),
        (
            vec![b'a'; 1_000_000],
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        ),
    ] {
        let mut destination = Vec::new();
        copy_verified(
            bytes.as_slice(),
            &mut destination,
            bytes.len() as u64,
            digest,
        )
        .unwrap();
        assert_eq!(destination, bytes);
    }
}

/// 超過検出はlimit+1 byteまでで止まり、巨大streamを最後まで読み込まない。
#[test]
fn oversize_reads_only_one_byte_beyond_limit() {
    let mut source = io::Cursor::new(vec![b'a'; 100_000]);
    let mut destination = Vec::new();
    let error = copy_verified(&mut source, &mut destination, 3, ABC).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(source.position(), 4);
    assert_eq!(destination.len(), 4);
}

/// mismatchとI/O障害を成功したsnapshotへ読み替えない。
#[test]
fn mismatch_and_read_failure_are_errors() {
    assert!(copy_verified(b"abd".as_slice(), &mut Vec::new(), 3, ABC).is_err());
    /// 読込障害を返す入力。
    struct Broken;
    impl Read for Broken {
        /// file故障を模擬する。
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("broken source"))
        }
    }
    assert!(copy_verified(Broken, &mut Vec::new(), 3, ABC).is_err());
}

/// 原本の置換・書換え後も変換へ渡すsnapshotは照合したbyteを保持する。
#[test]
fn verified_snapshot_is_independent_of_original_and_removed_on_drop() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input.pdf");
    std::fs::write(&source, b"abc").unwrap();
    let snapshot = Snapshot::capture(&source, 3, ABC).unwrap();
    let path = snapshot.path().to_path_buf();
    std::fs::write(&source, b"abd").unwrap();
    std::fs::remove_file(&source).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"abc");
    drop(snapshot);
    assert!(!path.exists());
}

/// FIFOはwriter待ちへ入らず、regular file以外として拒否する。
#[cfg(unix)]
#[test]
fn fifo_is_rejected_without_waiting_for_writer() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input.pdf");
    let name = CString::new(source.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = Snapshot::capture(&source, 3, ABC)
            .map(|_| ())
            .map_err(|error| error.kind());
        let _ = send.send(result);
    });
    assert_eq!(
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("FIFO open must not block"),
        Err(io::ErrorKind::InvalidInput)
    );
}
