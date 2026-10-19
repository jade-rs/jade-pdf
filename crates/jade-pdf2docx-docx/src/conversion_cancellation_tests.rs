//! 実際の変換段階で停止を発行し、保存・bytes公開前に部分成果物を破棄する。
use super::*;
use crate::conversion_control::{Stage, check};
use jade_pdf2docx_parser::{ParseCancellation, ParseError};

/// 書込境界と空bytesのcommit前停止は既存出力を保護し、一時ファイルを削除する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn atomic_output_cancellation_covers_chunk_boundaries() {
    for length in [0usize, 1, 8191, 8192, 8193, 16384, 16385] {
        for target_chunk in 0..=length.div_ceil(8192) {
            let directory = tempfile::tempdir().unwrap();
            let output = directory.path().join("output.docx");
            fs::write(&output, b"existing document").unwrap();
            let bytes = vec![42; length];
            let token = ParseCancellation::new();
            let options = ParseOptions {
                cancellation: Some(token.clone()),
                ..Default::default()
            };
            let mut chunks = 0;
            let result = output::write_atomic(&output, &bytes, |stage| {
                if stage == Stage::Writing {
                    chunks += 1;
                }
                if (target_chunk == 0 && stage == Stage::BeforeCommit)
                    || (target_chunk > 0 && chunks == target_chunk)
                {
                    token.cancel();
                }
                check(&options)
            });
            assert!(
                matches!(result, Err(ConvertError::Parse(ParseError::Cancelled))),
                "length={length}, chunk={target_chunk}"
            );
            assert_eq!(fs::read(&output).unwrap(), b"existing document");
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
            if target_chunk > 0 {
                assert_eq!(chunks, target_chunk);
            }
        }
    }
}

/// 最後の停止確認を通過したcommitは成功とし、公開後に停止エラーを返さない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancellation_after_final_check_allows_commit_to_win() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("nested/output.docx");
    let token = ParseCancellation::new();
    let options = ParseOptions {
        cancellation: Some(token.clone()),
        ..Default::default()
    };
    let bytes = vec![42; 16385];
    let mut chunks = 0;
    let mut final_check = false;
    output::write_atomic(&output, &bytes, |stage| {
        assert!(!final_check);
        if stage == Stage::Writing {
            chunks += 1;
        }
        let result = check(&options);
        if stage == Stage::BeforeCommit {
            final_check = true;
            token.cancel();
        }
        result
    })
    .unwrap();
    assert_eq!(chunks, 3);
    assert!(final_check);
    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert_eq!(fs::read_dir(output.parent().unwrap()).unwrap().count(), 1);
}
