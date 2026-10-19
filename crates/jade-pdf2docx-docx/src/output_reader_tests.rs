//! worker成果物のコピー途中のI/O失敗・割込と出力保護を固定する。
use super::*;
use std::io::{self, Read};

/// 保存先とは別の所有領域内でコピーし、途中失敗では両方の既存ファイルを保持する。
#[test]
fn supplied_directory_contains_atomic_copy_and_preserves_output_on_error() {
    let base = tempfile::tempdir().unwrap();
    let temporary = tempfile::tempdir_in(base.path()).unwrap();
    let output = base.path().join("output.docx");
    std::fs::write(&output, b"existing").unwrap();
    std::fs::write(temporary.path().join("owner"), b"owner").unwrap();
    let mut observed = false;
    let result = write_atomic_from_reader_in(
        &output,
        Some(temporary.path()),
        ScriptedReader {
            step: 0,
            interrupt: false,
            fail: true,
        },
        |stage| {
            if stage == Stage::Writing {
                observed = true;
                assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 2);
                assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 2);
            }
            Ok(())
        },
    );
    assert!(observed);
    assert!(matches!(result, Err(ConvertError::Io(_))));
    assert_eq!(std::fs::read(output).unwrap(), b"existing");
    assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 1);
    assert_eq!(
        std::fs::read(temporary.path().join("owner")).unwrap(),
        b"owner"
    );
}

/// 割込・部分読込・失敗を順番に返すreader。
struct ScriptedReader {
    /// 現在の読込段階。
    step: usize,
    /// 最初に割込を返すか。
    interrupt: bool,
    /// 最後にエラーを返すか。
    fail: bool,
}

impl Read for ScriptedReader {
    /// 実時間に依存せずI/O結果を返す。
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let step = self.step;
        self.step += 1;
        if self.interrupt && step == 0 {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if self.interrupt && self.fail {
            return Err(io::Error::other("unexpected retry after interruption"));
        }
        if step == usize::from(self.interrupt) {
            buffer[..3].copy_from_slice(b"doc");
            return Ok(3);
        }
        if self.fail {
            Err(io::Error::other("read failed"))
        } else {
            Ok(0)
        }
    }
}

/// コピー途中の読込失敗でも既存出力とtemporary fileの回収を保つ。
#[test]
fn partial_read_error_preserves_output() {
    let base = tempfile::tempdir().unwrap();
    let output = base.path().join("output.docx");
    std::fs::write(&output, b"existing").unwrap();
    let result = write_atomic_from_reader(
        &output,
        ScriptedReader {
            step: 0,
            interrupt: false,
            fail: true,
        },
        |_| Ok(()),
    );
    assert!(matches!(result, Err(ConvertError::Io(_))));
    assert_eq!(std::fs::read(output).unwrap(), b"existing");
    assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 1);
}

/// 単発割込を再試行し、最後まで読んだ結果だけを公開する。
#[test]
fn interrupted_read_is_retried() {
    let base = tempfile::tempdir().unwrap();
    let output = base.path().join("output.docx");
    write_atomic_from_reader(
        &output,
        ScriptedReader {
            step: 0,
            interrupt: true,
            fail: false,
        },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(std::fs::read(output).unwrap(), b"doc");
}

/// 割込の再試行前も停止を確認し、部分出力を公開しない。
#[test]
fn interrupted_read_observes_cancellation() {
    let base = tempfile::tempdir().unwrap();
    let output = base.path().join("output.docx");
    std::fs::write(&output, b"existing").unwrap();
    let result = write_atomic_from_reader(
        &output,
        ScriptedReader {
            step: 0,
            interrupt: true,
            fail: true,
        },
        |stage| {
            if stage == Stage::Writing {
                Err(ConvertError::Parse(
                    jade_pdf2docx_parser::ParseError::Cancelled,
                ))
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(
        result,
        Err(ConvertError::Parse(
            jade_pdf2docx_parser::ParseError::Cancelled
        ))
    ));
    assert_eq!(std::fs::read(output).unwrap(), b"existing");
    assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 1);
}
