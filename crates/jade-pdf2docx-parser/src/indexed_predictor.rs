use std::path::Path;

use crate::{
    ParseError, ParseLimits,
    limits::check_limit,
    syntax::{Lexer, Token, integer, name_is},
};

/// FlateDecode の行差分復元パラメータ。
pub(crate) struct Predictor {
    /// 1 は無変換、2 は TIFF、10..15 は PNG。
    kind: usize,
    /// 1 pixel の色成分数。
    colors: usize,
    /// 1 成分の bit 数。
    bits: usize,
    /// 1 行の pixel 数。
    columns: usize,
}

impl Default for Predictor {
    /// PDF 仕様の既定値を使用する。
    fn default() -> Self {
        Self {
            kind: 1,
            colors: 1,
            bits: 8,
            columns: 1,
        }
    }
}

impl Predictor {
    /// 検証済み属性を読む呼び出し元から差分復元の設定を受け取る。
    pub(crate) fn new(kind: usize, colors: usize, bits: usize, columns: usize) -> Self {
        Self {
            kind,
            colors,
            bits,
            columns,
        }
    }
    /// DecodeParms dictionary（または単一 filter の array）を読む。
    pub(crate) fn read(
        path: &Path,
        lexer: &mut Lexer<'_>,
        token: Token<'_>,
        limits: &ParseLimits,
    ) -> Result<Self, ParseError> {
        let array = matches!(token, Token::Open(false));
        if array {
            check_limit("container nesting", 2, limits.max_container_nesting as u64)?;
        }
        let token = if array {
            lexer
                .next()
                .ok_or_else(|| invalid(path, "missing DecodeParms array value"))?
        } else {
            token
        };
        let mut result = Self::default();
        if matches!(token, Token::Open(true)) {
            check_limit(
                "container nesting",
                if array { 3 } else { 2 },
                limits.max_container_nesting as u64,
            )?;
            loop {
                let key = match lexer.next() {
                    Some(Token::Close(true)) => break,
                    Some(Token::Name(key)) => key,
                    _ => return Err(invalid(path, "invalid DecodeParms key")),
                };
                let Some(Token::Word(word)) = lexer.next() else {
                    return Err(invalid(path, "invalid DecodeParms value"));
                };
                let value =
                    integer(word).ok_or_else(|| invalid(path, "invalid DecodeParms integer"))?;
                if name_is(key, b"Predictor") {
                    result.kind = value;
                } else if name_is(key, b"Colors") {
                    result.colors = value;
                } else if name_is(key, b"BitsPerComponent") {
                    result.bits = value;
                } else if name_is(key, b"Columns") {
                    result.columns = value;
                }
            }
        } else if !matches!(token, Token::Word(b"null")) {
            return Err(invalid(path, "DecodeParms must be direct dictionary"));
        }
        if array && !matches!(lexer.next(), Some(Token::Close(false))) {
            return Err(invalid(path, "unsupported DecodeParms filter chain"));
        }
        Ok(result)
    }

    /// 展開済み buffer 内で行を復元し、追加の大きな allocation を行わない。
    pub(crate) fn apply(&self, path: &Path, data: &mut Vec<u8>) -> Result<(), ParseError> {
        self.apply_controlled(path, data, || false)
    }

    /// 共通buffer内で差分を復元し、行境界と4096 byteごとに停止を検査する。
    pub(crate) fn apply_controlled(
        &self,
        path: &Path,
        data: &mut Vec<u8>,
        mut stopped: impl FnMut() -> bool,
    ) -> Result<(), ParseError> {
        if stopped() {
            return Err(invalid(path, "predictor decoding stopped"));
        }
        if self.kind == 1 {
            return Ok(());
        }
        if self.colors == 0 || self.columns == 0 || !matches!(self.bits, 1 | 2 | 4 | 8 | 16) {
            return Err(invalid(path, "invalid predictor dimensions"));
        }
        let pixel_bits = self
            .colors
            .checked_mul(self.bits)
            .ok_or_else(|| invalid(path, "predictor pixel size overflow"))?;
        let row = pixel_bits
            .checked_mul(self.columns)
            .and_then(|bits| bits.checked_add(7))
            .map(|bits| bits / 8)
            .ok_or_else(|| invalid(path, "predictor row size overflow"))?;
        let pixel = pixel_bits
            .checked_add(7)
            .map(|bits| bits / 8)
            .ok_or_else(|| invalid(path, "predictor pixel size overflow"))?;
        if self.kind == 2 {
            if self.bits != 8 {
                return Err(invalid(path, "unsupported TIFF predictor bit depth"));
            }
            if !data.len().is_multiple_of(row) {
                return Err(invalid(path, "incomplete TIFF predictor row"));
            }
            for start in (0..data.len()).step_by(row) {
                if stopped() {
                    return Err(invalid(path, "predictor decoding stopped"));
                }
                for column in pixel..row {
                    if column % 4096 == 0 && stopped() {
                        return Err(invalid(path, "predictor decoding stopped"));
                    }
                    data[start + column] =
                        data[start + column].wrapping_add(data[start + column - pixel]);
                }
            }
            return if stopped() {
                Err(invalid(path, "predictor decoding stopped"))
            } else {
                Ok(())
            };
        }
        if !(10..=15).contains(&self.kind) {
            return Err(invalid(path, "unsupported indexed stream predictor"));
        }
        let stride = row
            .checked_add(1)
            .ok_or_else(|| invalid(path, "predictor row size overflow"))?;
        if !data.len().is_multiple_of(stride) {
            return Err(invalid(path, "incomplete PNG predictor row"));
        }
        let rows = data.len() / stride;
        for line in 0..rows {
            if stopped() {
                return Err(invalid(path, "predictor decoding stopped"));
            }
            let filter = data[line * stride];
            if filter > 4 {
                return Err(invalid(path, "invalid PNG predictor filter"));
            }
            let source = line * stride + 1;
            let target = line * row;
            for column in 0..row {
                if column % 4096 == 0 && stopped() {
                    return Err(invalid(path, "predictor decoding stopped"));
                }
                let left = if column >= pixel {
                    data[target + column - pixel]
                } else {
                    0
                };
                let above = if line > 0 {
                    data[target + column - row]
                } else {
                    0
                };
                let upper_left = if line > 0 && column >= pixel {
                    data[target + column - row - pixel]
                } else {
                    0
                };
                let prediction = match filter {
                    0 => 0,
                    1 => left,
                    2 => above,
                    3 => ((u16::from(left) + u16::from(above)) / 2) as u8,
                    _ => paeth(left, above, upper_left),
                };
                data[target + column] = data[source + column].wrapping_add(prediction);
            }
        }
        data.truncate(rows * row);
        if stopped() {
            Err(invalid(path, "predictor decoding stopped"))
        } else {
            Ok(())
        }
    }
}

/// PNG Paeth の最も近い隣接値を返す。
fn paeth(left: u8, above: u8, upper_left: u8) -> u8 {
    let predicted = i32::from(left) + i32::from(above) - i32::from(upper_left);
    let distances = [
        (predicted - i32::from(left)).abs(),
        (predicted - i32::from(above)).abs(),
        (predicted - i32::from(upper_left)).abs(),
    ];
    if distances[0] <= distances[1] && distances[0] <= distances[2] {
        left
    } else if distances[1] <= distances[2] {
        above
    } else {
        upper_left
    }
}

/// 構造不正を入力 path とともに返す。
fn invalid(path: &Path, reason: &'static str) -> ParseError {
    ParseError::InvalidPdfStructure {
        path: path.into(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 停止要求は長いPNG/TIFF行の途中にも反映し、容量を増やさない。
    #[test]
    fn controlled_predictor_stops_inside_long_row() {
        for kind in [2, 15] {
            let predictor = Predictor::new(kind, 1, 8, 12288);
            let mut data = vec![1; 12288 + usize::from(kind == 15)];
            let capacity = data.capacity();
            let mut calls = 0;
            assert!(
                predictor
                    .apply_controlled(Path::new("cancel.pdf"), &mut data, || {
                        calls += 1;
                        calls == 4
                    })
                    .is_err()
            );
            assert_eq!(data.capacity(), capacity);
            assert_eq!(*data.last().unwrap(), 1);
        }
    }

    /// 各 PNG filter の固定例で、隣接値参照と in-place 書き換えを検証する。
    #[test]
    fn png_filters_restore_known_rows() {
        for (filter, rows) in [
            (0, [10, 20, 30, 15, 25, 35]),
            (1, [10, 10, 10, 15, 10, 10]),
            (2, [10, 20, 30, 5, 5, 5]),
            (3, [10, 15, 20, 10, 8, 8]),
            (4, [10, 10, 10, 5, 5, 5]),
        ] {
            let predictor = Predictor {
                kind: 15,
                columns: 3,
                ..Predictor::default()
            };
            let mut data = vec![filter];
            data.extend_from_slice(&rows[..3]);
            data.push(filter);
            data.extend_from_slice(&rows[3..]);
            predictor.apply(Path::new("filter.pdf"), &mut data).unwrap();
            assert_eq!(data, [10, 20, 30, 15, 25, 35]);
        }
    }

    /// PNG の packed sample の行幅は bit 数から切り上げる。
    #[test]
    fn png_packed_samples_keep_row_boundaries() {
        let predictor = Predictor {
            kind: 12,
            columns: 3,
            bits: 4,
            ..Predictor::default()
        };
        let mut data = vec![2, 0x12, 0x30, 2, 0x33, 0x30];
        predictor.apply(Path::new("packed.pdf"), &mut data).unwrap();
        assert_eq!(data, [0x12, 0x30, 0x45, 0x60]);
    }

    /// TIFF 8 bit の horizontal difference を行ごとに復元する。
    #[test]
    fn tiff_eight_bit_rows_are_independent() {
        let predictor = Predictor {
            kind: 2,
            columns: 3,
            ..Predictor::default()
        };
        let mut data = vec![10, 10, 10, 15, 10, 10];
        predictor.apply(Path::new("tiff.pdf"), &mut data).unwrap();
        assert_eq!(data, [10, 20, 30, 15, 25, 35]);
    }

    /// 不完全な行・不正 filter・寸法 overflow は panic せず型付きで拒否する。
    #[test]
    fn malformed_predictors_are_structure_errors() {
        for (columns, colors, data) in [
            (3, 1, vec![2, 1]),
            (1, 1, vec![5, 1]),
            (usize::MAX, 2, vec![]),
        ] {
            let predictor = Predictor {
                kind: 12,
                columns,
                colors,
                ..Predictor::default()
            };
            assert!(matches!(
                predictor.apply(Path::new("bad.pdf"), &mut data.clone()),
                Err(ParseError::InvalidPdfStructure { .. })
            ));
        }
    }
}
