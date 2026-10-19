//! 展開途中の停止をreaderで再現し、通常/Fontの部分結果を返さないことを固定する。
use super::*;
use crate::ParseCancellation;
use std::{cell::Cell, io};

/// 最初のchunk後に停止を要求する展開reader。
struct CancellingDecoder<'a> {
    /// 共有する停止要求。
    token: &'a ParseCancellation,
    /// 展開呼び出し数。
    calls: &'a Cell<usize>,
    /// 停止時の結果。0はEOF、1はchunk、2はエラー。
    outcome: u8,
}
impl Read for CancellingDecoder<'_> {
    /// 2回目の展開で停止要求を発行する。
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if call == 1 {
            buffer.fill(7);
            return Ok(buffer.len());
        }
        self.token.cancel();
        match self.outcome {
            0 => Ok(0),
            1 => {
                buffer[0] = 8;
                Ok(1)
            }
            _ => Err(io::Error::other("decoder stopped")),
        }
    }
}

/// 複数chunk展開で停止された場合、EOFやcodecエラーにも部分結果を返さない。
#[test]
fn cancellation_during_decode_discards_partial_output() {
    for font in [false, true] {
        for outcome in 0..3 {
            let token = ParseCancellation::new();
            let calls = Cell::new(0);
            let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
            let reader = CancellingDecoder {
                token: &token,
                calls: &calls,
                outcome,
            };
            assert!(decode_buffer(reader, &budget, font).is_none());
            assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
            assert_eq!(calls.get(), 2);
        }
    }
}
