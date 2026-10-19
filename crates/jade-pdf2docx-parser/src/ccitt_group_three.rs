//! CCITT Group 3の1D行を、実データの境界とDecodeParmsを確認して復号する。
use fax::maps::{black, white};

use super::{Parameters, bits::InputBits};

/// 正常行を追加し、終端または宣言行数まで復号する。
pub(super) fn decode(
    rgb: &mut Vec<u8>,
    data: &[u8],
    parameters: Parameters,
    budget: Option<&super::DecodeBudget>,
) -> Option<()> {
    let repaired = super::row_decoder::decode(rgb, data, parameters, budget, decode_row, terminal)?;
    if let Some(budget) = budget {
        budget.record_ccitt_repairs(repaired)?;
    }
    Some(())
}

/// 6個のEOLからなるRTCを検査する。
fn terminal(bits: &mut InputBits<'_>, parameters: &Parameters) -> Option<()> {
    for _ in 0..6 {
        if parameters.byte_aligned {
            bits.align()?;
        }
        bits.eol()?;
    }
    Some(())
}

/// 1D符号のrunを検査し、予約済み配列へ1行を追加する。
fn decode_row(rgb: &mut Vec<u8>, bits: &mut InputBits<'_>, parameters: &Parameters) -> Option<()> {
    let mut remaining = parameters.columns;
    let mut black_run = false;
    while remaining > 0 {
        let mut run = 0u16;
        loop {
            let next = if black_run {
                black::decode(bits)
            } else {
                white::decode(bits)
            }?;
            run = run.checked_add(next)?;
            if run > remaining {
                return None;
            }
            if next < 64 {
                break;
            }
        }
        remaining -= run;
        let value = if black_run != parameters.black_is_one {
            0
        } else {
            255
        };
        let end = rgb.len().checked_add(usize::from(run).checked_mul(3)?)?;
        if end > rgb.capacity() {
            return None;
        }
        rgb.resize(end, value);
        black_run = !black_run;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fax::{BitWriter, VecWriter};
    use proptest::prelude::*;

    /// 白黒交互のrun長モデルを符号化し、makeup符号と終端符号を連結する。
    fn encoded_runs(runs: &[u16]) -> Vec<u8> {
        let mut writer = VecWriter::new();
        for (index, &length) in runs.iter().enumerate() {
            let mut remaining = length;
            while remaining >= 64 {
                let makeup = (remaining / 64 * 64).min(2560);
                let code = if index % 2 == 0 {
                    white::encode(makeup)
                } else {
                    black::encode(makeup)
                }
                .unwrap();
                writer.write(code).unwrap();
                remaining -= makeup;
            }
            let code = if index % 2 == 0 {
                white::encode(remaining)
            } else {
                black::encode(remaining)
            }
            .unwrap();
            writer.write(code).unwrap();
        }
        writer.finish()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// makeup・色境界・極性を含むrunモデルと復号した全画素を比較する。
        #[test]
        fn runs_preserve_pixels_and_polarity(runs in prop::collection::vec(1u16..=4096, 1..=8), inverted in any::<bool>()) {
            let columns: u16 = runs.iter().sum();
            let data = encoded_runs(&runs);
            let mut rgb = Vec::with_capacity(usize::from(columns) * 3);
            prop_assert_eq!(decode(&mut rgb, &data, Parameters {
                columns, rows: 1, black_is_one: inverted, end_of_line: false, byte_aligned: false, end_of_block: false, damaged_rows: 0,
            }, None), Some(()));
            let expected: Vec<u8> = runs.iter().enumerate().flat_map(|(index, &run)| {
                let gray = if (index % 2 == 1) != inverted { 0 } else { 255 };
                std::iter::repeat_n(gray, usize::from(run) * 3)
            }).collect();
            prop_assert_eq!(rgb, expected);
        }
    }
}
