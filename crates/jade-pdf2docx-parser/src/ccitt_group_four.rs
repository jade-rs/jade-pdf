//! Group 4の2D行を予約済みRGBへ復号し、行幅とEOFBを検査する。
use super::{Parameters, bits::InputBits};
use fax::{
    BitReader,
    maps::{self, Mode},
};

/// 参照行を出力RGBから借用し、終端または宣言行数まで復号する。
pub(super) fn decode(
    rgb: &mut Vec<u8>,
    data: &[u8],
    parameters: Parameters,
    budget: Option<&super::DecodeBudget>,
) -> Option<()> {
    super::row_decoder::decode(rgb, data, parameters, budget, append_row, terminal).map(|_| ())
}

/// byte padding任意の24 bit EOFBを検査する。
fn terminal(bits: &mut InputBits<'_>, parameters: &Parameters) -> Option<()> {
    if parameters.byte_aligned && bits.exact(12) != Some(1) {
        bits.align()?;
    }
    for _ in 0..2 {
        if bits.exact(12)? != 1 {
            return None;
        }
        bits.consume(12).ok()?;
    }
    Some(())
}

/// 直前の参照行を借用し、追加のtransition配列なしで1行を追加する。
fn append_row(rgb: &mut Vec<u8>, bits: &mut InputBits<'_>, parameters: &Parameters) -> Option<()> {
    let width = usize::from(parameters.columns);
    let row_bytes = width.checked_mul(3)?;
    let start = rgb.len();
    let end = start.checked_add(row_bytes)?;
    if end > rgb.capacity() {
        return None;
    }
    rgb.resize(end, 0);
    let (previous, current) = rgb.split_at_mut(start);
    let reference = start
        .checked_sub(row_bytes)
        .map(|offset| &previous[offset..]);
    decode_row(bits, reference, current, width, parameters.black_is_one)
}

/// Pass/Vertical/Horizontalの各modeを検査し、全幅を埋めた行だけを受け入れる。
fn decode_row(
    bits: &mut InputBits<'_>,
    reference: Option<&[u8]>,
    current: &mut [u8],
    width: usize,
    inverted: bool,
) -> Option<()> {
    let mut position = 0usize;
    let mut black = false;
    let mut first = true;
    while position < width {
        match maps::mode::decode(bits)? {
            Mode::Pass => {
                let b1 = reference_boundary(reference, position, first, black, width, inverted);
                let b2 = reference_boundary(reference, b1, false, !black, width, inverted);
                if b2 <= position {
                    return None;
                }
                fill(current, position, b2, black, inverted);
                position = b2;
            }
            Mode::Vertical(delta) => {
                let b1 = reference_boundary(reference, position, first, black, width, inverted);
                let a1 =
                    usize::try_from(i32::try_from(b1).ok()?.checked_add(i32::from(delta))?).ok()?;
                if a1 < position || a1 > width {
                    return None;
                }
                fill(current, position, a1, black, inverted);
                position = a1;
                black = !black;
            }
            Mode::Horizontal => {
                let first_run = run(bits, black, width - position)?;
                let a1 = position.checked_add(first_run)?;
                let second_run = run(bits, !black, width - a1)?;
                let a2 = a1.checked_add(second_run)?;
                if a2 <= position {
                    return None;
                }
                fill(current, position, a1, black, inverted);
                fill(current, a1, a2, !black, inverted);
                position = a2;
            }
            Mode::EOF | Mode::Extension => return None,
        }
        first = false;
    }
    Some(())
}

/// 参照行でa0の右にあり、現在色と反対の色へ変わる最初の境界を返す。
fn reference_boundary(
    reference: Option<&[u8]>,
    a0: usize,
    first: bool,
    black: bool,
    width: usize,
    inverted: bool,
) -> usize {
    let Some(reference) = reference else {
        return width;
    };
    let start = if first { a0 } else { a0.saturating_add(1) };
    for position in start..width {
        let after = (reference[position * 3] == 0) != inverted;
        if after == black {
            continue;
        }
        let before = position > 0 && (reference[(position - 1) * 3] == 0) != inverted;
        if before != after {
            return position;
        }
    }
    width
}

/// makeupと終端符号を合算し、残り幅を越えるrunを拒否する。
fn run(bits: &mut InputBits<'_>, black: bool, remaining: usize) -> Option<usize> {
    let mut length = 0usize;
    loop {
        let next = if black {
            maps::black::decode(bits)
        } else {
            maps::white::decode(bits)
        }?;
        length = length.checked_add(usize::from(next))?;
        if length > remaining {
            return None;
        }
        if next < 64 {
            return Some(length);
        }
    }
}

/// 検査済みの行区間へ指定色を保存する。
fn fill(row: &mut [u8], start: usize, end: usize, black: bool, inverted: bool) {
    row[start * 3..end * 3].fill(if black != inverted { 0 } else { 255 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use fax::{BitWriter, Color, VecWriter};
    use proptest::prelude::*;

    /// 独立した符号列と画素モデルを全行・両極性で比較する。
    fn assert_model(width: u16, rows: &[Vec<bool>], inverted: bool) {
        let data = if width == u16::MAX {
            horizontal_rows(rows)
        } else {
            let mut encoder = fax::encoder::Encoder::new(VecWriter::new());
            for row in rows {
                encoder
                    .encode_line(
                        row.iter()
                            .map(|&black| if black { Color::Black } else { Color::White }),
                        width,
                    )
                    .unwrap();
            }
            encoder.finish().unwrap().finish()
        };
        let expected: Vec<u8> = rows
            .iter()
            .flatten()
            .flat_map(|&black| [if black != inverted { 0 } else { 255 }; 3])
            .collect();
        let mut rgb = Vec::with_capacity(expected.len());
        assert_eq!(
            decode(
                &mut rgb,
                &data,
                Parameters {
                    columns: width,
                    rows: rows.len() as u32,
                    black_is_one: inverted,
                    end_of_line: false,
                    byte_aligned: false,
                    end_of_block: true,
                    damaged_rows: 0,
                },
                None,
            ),
            Some(())
        );
        assert_eq!(rgb, expected);
    }

    /// i16境界でoverflowする依存encoderを避け、最大幅fixtureをHorizontalだけで作る。
    fn horizontal_rows(rows: &[Vec<bool>]) -> Vec<u8> {
        let mut writer = VecWriter::new();
        for row in rows {
            let mut position = 0;
            while position < row.len() {
                writer
                    .write(maps::mode::encode(Mode::Horizontal).unwrap())
                    .unwrap();
                for black in [false, true] {
                    let length = row[position..]
                        .iter()
                        .take_while(|&&value| value == black)
                        .count();
                    position += length;
                    write_run(&mut writer, black, length);
                }
            }
        }
        writer.write(maps::EOL).unwrap();
        writer.write(maps::EOL).unwrap();
        writer.finish()
    }

    /// 2560画素以下のmakeupと終端符号を連結して長いrunを作る。
    fn write_run(writer: &mut VecWriter, black: bool, mut length: usize) {
        let encode = if black {
            maps::black::encode
        } else {
            maps::white::encode
        };
        while length >= 64 {
            let makeup = (length / 64 * 64).min(2560);
            writer.write(encode(makeup as u16).unwrap()).unwrap();
            length -= makeup;
        }
        writer.write(encode(length as u16).unwrap()).unwrap();
    }

    /// 最小幅、makeup境界、標準fax幅、u16最大幅を同じ画素モデルで固定する。
    #[test]
    fn reference_rows_cover_width_and_makeup_boundaries() {
        for width in [1u16, 3, 63, 64, 1728, u16::MAX] {
            let rows: Vec<Vec<bool>> = (0..7)
                .map(|kind| {
                    (0..width)
                        .map(|pixel| match kind {
                            0 => false,
                            1 => true,
                            2 => pixel >= width / 2,
                            3 => pixel > width / 2,
                            4 => pixel % 2 == 0,
                            5 => pixel % 4 == 1,
                            _ => false,
                        })
                        .collect()
                })
                .collect();
            for inverted in [false, true] {
                assert_model(width, &rows, inverted);
            }
        }
    }

    /// i16境界をまたぐ参照境界とVerticalの全deltaを独立した符号列で固定する。
    #[test]
    fn vertical_modes_cross_signed_boundary_without_overflow() {
        let width = u16::MAX;
        let mut writer = VecWriter::new();
        writer
            .write(maps::mode::encode(Mode::Horizontal).unwrap())
            .unwrap();
        write_run(&mut writer, false, 32768);
        write_run(&mut writer, true, usize::from(width) - 32768);
        let mut boundary = 32768usize;
        let mut boundaries = vec![boundary];
        for delta in -3i8..=3 {
            writer
                .write(maps::mode::encode(Mode::Vertical(delta)).unwrap())
                .unwrap();
            writer
                .write(maps::mode::encode(Mode::Vertical(0)).unwrap())
                .unwrap();
            boundary = (boundary as i32 + i32::from(delta)) as usize;
            boundaries.push(boundary);
        }
        writer.write(maps::EOL).unwrap();
        writer.write(maps::EOL).unwrap();
        let data = writer.finish();
        for inverted in [false, true] {
            let expected: Vec<u8> = boundaries
                .iter()
                .flat_map(|&edge| {
                    (0..usize::from(width)).flat_map(move |position| {
                        [if (position >= edge) != inverted {
                            0
                        } else {
                            255
                        }; 3]
                    })
                })
                .collect();
            let mut rgb = Vec::with_capacity(expected.len());
            assert_eq!(
                decode(
                    &mut rgb,
                    &data,
                    Parameters {
                        columns: width,
                        rows: boundaries.len() as u32,
                        black_is_one: inverted,
                        end_of_line: false,
                        byte_aligned: false,
                        end_of_block: true,
                        damaged_rows: 0,
                    },
                    None,
                ),
                Some(())
            );
            assert_eq!(rgb, expected);
        }
    }

    /// 負の画素位置と既に復号した位置への巻戻りを拒否する。
    #[test]
    fn vertical_modes_reject_negative_or_backward_positions() {
        for (width, edge, prefix) in [(3, 0, false), (16, 8, true)] {
            let reference: Vec<u8> = (0..width)
                .flat_map(|position| [if position >= edge { 0 } else { 255 }; 3])
                .collect();
            let mut writer = VecWriter::new();
            if prefix {
                writer
                    .write(maps::mode::encode(Mode::Horizontal).unwrap())
                    .unwrap();
                write_run(&mut writer, false, 5);
                write_run(&mut writer, true, 2);
            }
            writer
                .write(maps::mode::encode(Mode::Vertical(-3)).unwrap())
                .unwrap();
            let data = writer.finish();
            let mut bits = InputBits::new(&data).unwrap();
            let mut current = vec![0; width * 3];
            assert_eq!(
                decode_row(&mut bits, Some(&reference), &mut current, width, false),
                None
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// 広い行と長いrunを含む参照行列から、Pass/Vertical/Horizontalを照合する。
        #[test]
        fn varied_reference_rows_match_pixel_model(width in 1u16..=4096, models in prop::collection::vec(prop::collection::vec((any::<bool>(), 1usize..=512), 1..=16), 1..=8), inverted in any::<bool>()) {
            let rows: Vec<Vec<bool>> = models.iter().map(|runs| {
                let pattern: Vec<bool> = runs.iter().flat_map(|&(black, length)| std::iter::repeat_n(black, length)).collect();
                pattern.into_iter().cycle().take(usize::from(width)).collect()
            }).collect();
            assert_model(width, &rows, inverted);
        }
    }
}
