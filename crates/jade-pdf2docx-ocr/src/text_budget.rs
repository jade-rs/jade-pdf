//! 認識本文の累積容量を、切り捨てずに検査する。
use crate::OcrError;

/// UTF-8本文と各行の区切り1byteを含む容量、および行数の上限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcrTextLimits {
    /// 全行のUTF-8長と行数の合計byte上限。
    pub max_bytes: u64,
    /// 全体の行数上限。
    pub max_lines: u64,
}

impl Default for OcrTextLimits {
    /// 文書全体で8 MiB・32768行を既定値とする。
    fn default() -> Self {
        Self {
            max_bytes: 8 * 1024 * 1024,
            max_lines: 32_768,
        }
    }
}

impl OcrTextLimits {
    /// 上限を指定しない従来APIの互換用に最大値を返す。
    pub const fn unlimited() -> Self {
        Self {
            max_bytes: u64::MAX,
            max_lines: u64::MAX,
        }
    }
}

/// 本文を保持する前に予約し、失敗時は残量を変更しない。
#[derive(Debug, Clone)]
pub struct OcrTextBudget {
    /// 次の認識処理へ渡す未使用の容量。
    remaining: OcrTextLimits,
}

impl OcrTextBudget {
    /// 指定容量をすべて未使用として開始する。0は容量枯渇として扱う。
    pub fn new(limits: OcrTextLimits) -> Self {
        Self { remaining: limits }
    }

    /// 次のページや行の認識へ渡す残量を返す。
    pub fn remaining(&self) -> OcrTextLimits {
        self.remaining
    }

    /// 結合途中の行が区切り込みで収まるかを、残量を消費せず検査する。
    pub fn check_line_bytes(&self, bytes: u64) -> Result<(), OcrError> {
        if self.remaining.max_lines == 0
            || bytes
                .checked_add(1)
                .is_none_or(|needed| needed > self.remaining.max_bytes)
        {
            return Err(OcrError::ResourceLimit("OCR text budget exceeded".into()));
        }
        Ok(())
    }

    /// 完成行のUTF-8長と区切りを予約する。上限と同じ値は受理する。
    pub fn reserve_line(&mut self, text: &str) -> Result<(), OcrError> {
        let bytes = text.len() as u64;
        self.check_line_bytes(bytes)?;
        self.remaining.max_bytes -= bytes + 1;
        self.remaining.max_lines -= 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 日本語を文字数でなくUTF-8長で数え、最後の行にも区切り1byteを予約する。
    #[test]
    fn utf8_boundary_is_inclusive_and_failed_reservation_is_atomic() {
        let limits = OcrTextLimits {
            max_bytes: 4,
            max_lines: 1,
        };
        let mut budget = OcrTextBudget::new(limits);
        assert!(budget.reserve_line("日本").is_err());
        assert_eq!(budget.remaining(), limits);
        budget.reserve_line("日").unwrap();
        assert_eq!(
            budget.remaining(),
            OcrTextLimits {
                max_bytes: 0,
                max_lines: 0
            }
        );
        assert!(budget.reserve_line("a").is_err());
    }

    /// 行数枯渇と区切りの加算overflowを本文容量が余っていても拒否する。
    #[test]
    fn exhausted_lines_and_arithmetic_overflow_are_rejected() {
        let mut budget = OcrTextBudget::new(OcrTextLimits {
            max_bytes: 100,
            max_lines: 1,
        });
        budget.reserve_line("a").unwrap();
        assert!(budget.reserve_line("b").is_err());
        assert!(
            OcrTextBudget::new(OcrTextLimits::unlimited())
                .check_line_bytes(u64::MAX)
                .is_err()
        );
    }

    proptest! {
        /// 行ごとの予約はUTF-8長と行数の和に一致し、超過した予約では状態を変えない。
        #[test]
        fn reservations_match_exact_cumulative_sizes(
            lines in prop::collection::vec(".{0,24}", 0..32),
            max_bytes in 0u64..512,
            max_lines in 0u64..32,
        ) {
            let mut budget = OcrTextBudget::new(OcrTextLimits { max_bytes, max_lines });
            let mut bytes = 0;
            let mut count = 0;
            for line in lines {
                let needed = line.len() as u64 + 1;
                let before = budget.remaining();
                if bytes + needed <= max_bytes && count < max_lines {
                    prop_assert!(budget.reserve_line(&line).is_ok());
                    bytes += needed;
                    count += 1;
                } else {
                    prop_assert!(budget.reserve_line(&line).is_err());
                    prop_assert_eq!(budget.remaining(), before);
                }
                prop_assert_eq!(budget.remaining(), OcrTextLimits {
                    max_bytes: max_bytes - bytes, max_lines: max_lines - count,
                });
            }
        }
    }
}
