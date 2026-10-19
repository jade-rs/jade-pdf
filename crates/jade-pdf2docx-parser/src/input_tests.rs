//! 偽readerを使い、実時間に依存せず読込中の停止・容量境界を固定する。
use super::*;
use crate::ParseCancellation;
use std::{
    cell::Cell,
    io::{self, Cursor},
};

/// 指定readで停止要求を出し、正常chunk・EOF・I/O失敗を返すreader。
struct CancellingReader<'a> {
    /// 呼び出し側が共有する停止要求。
    token: &'a ParseCancellation,
    /// 呼び出し数。停止検出後の余分なreadを検証する。
    calls: &'a Cell<usize>,
    /// 最初のchunkを正常に返すか。
    first_chunk: bool,
    /// 停止要求と同時に返す結果。0はEOF、1はchunk、2はI/Oエラー。
    outcome: u8,
}
impl Read for CancellingReader<'_> {
    /// 2回目までのread中に停止要求を出す。
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if self.first_chunk && call == 1 {
            buffer[0] = 1;
            return Ok(1);
        }
        self.token.cancel();
        match self.outcome {
            0 => Ok(0),
            1 => {
                buffer[0] = 2;
                Ok(1)
            }
            _ => Err(io::Error::other("reader stopped")),
        }
    }
}

/// 停止要求後に返ったchunk・EOF・I/O失敗のどれでも部分入力を公開しない。
#[test]
fn cancellation_during_read_discards_partial_buffer() {
    for first_chunk in [false, true] {
        for outcome in 0..3 {
            let token = ParseCancellation::new();
            let calls = Cell::new(0);
            let limits = ParseLimits::default();
            let budget = DecodeBudget::with_cancellation(&limits, Some(&token));
            let reader = CancellingReader {
                token: &token,
                calls: &calls,
                first_chunk,
                outcome,
            };
            assert!(
                matches!(read(reader, &limits, &budget), Err(ParseError::Cancelled)),
                "first={first_chunk} outcome={outcome}"
            );
            assert_eq!(calls.get(), if first_chunk { 2 } else { 1 });
        }
    }
}

/// 入力上限のinclusive境界と0 byte上限を、複数chunkでも保持する。
#[test]
fn input_limit_is_inclusive_and_reads_only_one_extra_byte() {
    for limit in [0, 1, 8191, 8192, 8193, 16384] {
        let limits = ParseLimits {
            max_input_bytes: limit,
            ..Default::default()
        };
        let exact = vec![7; limit as usize];
        assert_eq!(
            read(Cursor::new(&exact), &limits, &DecodeBudget::new(&limits)).unwrap(),
            exact
        );
        let large = vec![7; limit as usize + 10];
        let mut cursor = Cursor::new(large);
        assert!(
            matches!(read(&mut cursor, &limits, &DecodeBudget::new(&limits)), Err(ParseError::ResourceLimit { resource: "input bytes", actual, .. }) if actual == limit + 1)
        );
        assert_eq!(cursor.position(), limit + 1);
    }
}
