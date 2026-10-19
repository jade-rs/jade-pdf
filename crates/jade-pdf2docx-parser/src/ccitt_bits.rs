//! CCITT符号列を借用し、実データ境界とゼロpaddingを検査する。
use fax::BitReader;

/// 借用した符号列の位置を保持し、EOF後の符号消費を許可しない。
#[derive(Clone)]
pub(super) struct InputBits<'a> {
    /// 元の符号化データ。
    data: &'a [u8],
    /// 次に読むbitの位置。
    position: usize,
    /// 実際に利用できるbit数。
    length: usize,
}

impl<'a> InputBits<'a> {
    /// 借用データの実際のbit数を検査してreaderを作る。
    pub(super) fn new(data: &'a [u8]) -> Option<Self> {
        Some(Self {
            data,
            position: 0,
            length: data.len().checked_mul(8)?,
        })
    }

    /// 実データ内の指定bit数だけを先読みする。
    pub(super) fn exact(&self, bits: u8) -> Option<u16> {
        if self.position.checked_add(usize::from(bits))? > self.length {
            return None;
        }
        self.peek(bits)
    }

    /// 完了行の後に、最終byteのゼロpadding以外の入力が残っていないか調べる。
    pub(super) fn padded_end(&self) -> bool {
        let remaining = self.length - self.position;
        remaining == 0
            || (remaining <= usize::from(self.bits_to_byte_boundary())
                && self.exact(remaining as u8) == Some(0))
    }

    /// 行の直前のゼロpaddingを検査してbyte境界へ移動する。
    pub(super) fn align(&mut self) -> Option<()> {
        let count = self.bits_to_byte_boundary();
        if count == 0 {
            return Some(());
        }
        if self.exact(count)? != 0 {
            return None;
        }
        self.consume(count).ok()
    }

    /// 少なくとも11個のゼロと1個の1からなるEOLを読む。
    pub(super) fn eol(&mut self) -> Option<()> {
        let mut zeros = 0usize;
        loop {
            let bit = self.exact(1)?;
            self.consume(1).ok()?;
            if bit == 1 {
                return (zeros >= 11).then_some(());
            }
            zeros += 1;
        }
    }

    /// 破損行の末尾EOLを探し、次の行境界処理が読める位置に戻す。
    /// 長い破損データでも追加確保せず、4096 bitごとに協調停止を確認する。
    pub(super) fn find_eol(
        &mut self,
        byte_aligned: bool,
        budget: Option<&super::DecodeBudget>,
    ) -> Option<()> {
        let mut zeros = 0usize;
        while let Some(bit) = self.exact(1) {
            if self.position.is_multiple_of(4096)
                && budget.is_some_and(|budget| budget.is_stopped())
            {
                return None;
            }
            self.consume(1).ok()?;
            if bit == 0 {
                zeros += 1;
            } else if zeros >= 11 {
                // 次の境界処理には最大19 bitだけ残し、長いfillの再走査を避ける。
                self.position -= 12;
                if byte_aligned {
                    self.position -= self.position % 8;
                }
                return Some(());
            } else {
                zeros = 0;
            }
        }
        None
    }
}

impl BitReader for InputBits<'_> {
    type Error = ();

    /// 符号表の先読みに限り末尾をゼロ補完し、実際の消費は別途検査する。
    fn peek(&self, bits: u8) -> Option<u16> {
        if bits > 16 || self.position >= self.length {
            return None;
        }
        let mut value = 0u16;
        for offset in 0..usize::from(bits) {
            value <<= 1;
            if let Some(index) = self
                .position
                .checked_add(offset)
                .filter(|index| *index < self.length)
            {
                value |= u16::from((self.data[index / 8] >> (7 - index % 8)) & 1);
            }
        }
        Some(value)
    }

    /// 実データの範囲を越える符号消費を拒否する。
    fn consume(&mut self, bits: u8) -> Result<(), Self::Error> {
        let position = self.position.checked_add(usize::from(bits)).ok_or(())?;
        if position > self.length {
            return Err(());
        }
        self.position = position;
        Ok(())
    }

    /// 次のbyte境界までに必要なpadding bit数を返す。
    fn bits_to_byte_boundary(&self) -> u8 {
        ((8 - self.position % 8) % 8) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 長いfillを再走査せず、検出済みEOLの直前までreaderを進める。
    #[test]
    fn recovery_leaves_only_bounded_eol_fill() {
        for padding in 0..8 {
            let mut data = vec![0; 8192];
            data[0] = 255;
            *data.last_mut().unwrap() = 1 << padding;
            for aligned in [false, true] {
                let mut bits = InputBits::new(&data).unwrap();
                assert_eq!(bits.find_eol(aligned, None), Some(()));
                assert!(bits.length - bits.position - padding <= 19);
                if aligned {
                    assert_eq!(bits.align(), Some(()));
                }
                assert_eq!(bits.eol(), Some(()));
                assert!(bits.padded_end());
            }
        }
    }

    /// EOLのない長い破損入力でも、途中位置から開始した停止要求を観測する。
    #[test]
    fn recovery_observes_cancellation_while_searching() {
        let data = vec![255; 8192];
        let token = crate::ParseCancellation::default();
        token.cancel();
        let budget = super::super::DecodeBudget::with_cancellation(
            &crate::ParseLimits::default(),
            Some(&token),
        );
        let mut bits = InputBits::new(&data).unwrap();
        bits.consume(1).unwrap();
        assert_eq!(bits.find_eol(false, Some(&budget)), None);
        assert!(bits.position < bits.length);
        assert!(budget.is_stopped());
    }
}
