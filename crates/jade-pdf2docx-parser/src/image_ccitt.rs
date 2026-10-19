//! CCITTの符号化行を検査し、画像辞書のsample配置で画素またはPNGへ変換する。
use super::{DecodeBudget, encode_rgb8_png};

#[path = "image_ccitt_parameters.rs"]
mod parameter_reader;

#[path = "image_ccitt_samples.rs"]
mod samples;

#[path = "ccitt_bits.rs"]
mod bits;
#[path = "ccitt_group_four.rs"]
mod group_four;
#[path = "ccitt_group_three.rs"]
mod group_three;
#[path = "ccitt_rows.rs"]
mod row_decoder;

/// CCITTの行境界・終端と出力寸法を指定する。
pub(super) struct Parameters {
    /// 1行の画素数。
    pub columns: u16,
    /// 終端不要時の最大行数。零では入力末尾まで、終端必須時は終端まで復号する。
    pub rows: u32,
    /// 黒の符号を白画素へ反転するか。
    pub black_is_one: bool,
    /// 各行の先頭にEOLを必須とするか。
    pub end_of_line: bool,
    /// 各符号化行の前にbyte境界までゼロを埋めるか。
    pub byte_aligned: bool,
    /// 最終行の後にRTCまたはEOFBを必須とするか。
    pub end_of_block: bool,
    /// EOL付きGroup 3で許容する破損行の総数。他の方式では零。
    pub damaged_rows: u32,
}

/// CCITTFaxDecode の 1bpp 画像 stream を RGB PNG に変換する。
pub(super) fn decode(
    decode_budget: &DecodeBudget,
    pdf_bytes: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
    dictionary: &[u8],
) -> Option<Vec<u8>> {
    let rgb = decode_rgb(decode_budget, pdf_bytes, data, width, height, dictionary)?;
    encode_rgb8_png(decode_budget, width, height, &rgb)
}

/// 画像辞書のGray1 Decodeをfilter極性の後、soft mask合成の前に適用する。
pub(super) fn decode_source(
    budget: &DecodeBudget,
    pdf: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
    dictionary: &[u8],
) -> Option<Vec<u8>> {
    let parameters = super::image_decode_parameters(budget, pdf, dictionary)?;
    if super::image_sample_decode::require_codec_identity(budget, pdf, dictionary).is_some() {
        return decode(budget, pdf, data, width, height, &parameters);
    }
    let mapping = super::image_sample_decode::SampleDecode::read_ccitt(budget, pdf, dictionary)?;
    let mut rgb = decode_rgb(budget, pdf, data, width, height, &parameters)?;
    mapping.apply(budget, &mut rgb)?;
    encode_rgb8_png(budget, width, height, &rgb)
}

/// 外部maskの二値sampleを、RGB配列の容量を保持したままGrayへ詰める。
pub(super) fn decode_gray(
    budget: &DecodeBudget,
    pdf: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
    parameters: &[u8],
) -> Option<Vec<u8>> {
    let mut samples = decode_rgb(budget, pdf, data, width, height, parameters)?;
    let count = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    if samples.len() != count.checked_mul(3)? {
        return None;
    }
    for index in 0..count {
        if index % 4096 == 0 && budget.is_stopped() {
            return None;
        }
        samples[index] = samples[index * 3];
    }
    samples.truncate(count);
    (!budget.is_stopped()).then_some(samples)
}

/// 行終端とsample配置を検査し、共通の予算付きRGB画素を返す。
fn decode_rgb(
    decode_budget: &DecodeBudget,
    pdf_bytes: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
    dictionary: &[u8],
) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }
    decode_budget.check_image_pixels(width, height)?;
    if !crate::metadata::dictionary_entries_complete(dictionary) {
        return None;
    }
    let reader = parameter_reader::Reader {
        budget: decode_budget,
        pdf: pdf_bytes,
        dictionary,
    };
    let columns = u32::try_from(reader.integer(b"Columns", b"1728")?).ok()?;
    if columns == 0 {
        return None;
    }
    let declared_rows = u32::try_from(reader.integer(b"Rows", b"0")?).ok()?;
    let k = reader.integer(b"K", b"0")?;
    let end_of_block = reader.boolean(b"EndOfBlock", b"true")?;
    let black_is_one = reader.boolean(b"BlackIs1", b"false")?;
    let end_of_line = reader.boolean(b"EndOfLine", b"false")?;
    let byte_aligned = reader.boolean(b"EncodedByteAlign", b"false")?;
    let damaged_rows = u32::try_from(reader.integer(b"DamagedRowsBeforeError", b"0")?).ok()?;
    // filterと画像はそれぞれの幅でbyte境界へ揃える。
    // 既知行数または必要sample分の容量を先に予約し、未知行数の追加行も数える。
    let layout = samples::Layout::new(width, height, columns)?;
    let reserved_rows = if end_of_block || declared_rows == 0 {
        layout.required_rows()?
    } else {
        declared_rows
    };
    decode_budget.check_image_pixels(columns, reserved_rows)?;
    if columns > u16::MAX as u32 {
        return None;
    }
    if k > 0 {
        return None;
    }
    if reserved_rows < layout.required_rows()? {
        return None;
    }
    let capacity = usize::try_from(u64::from(columns) * u64::from(reserved_rows) * 3).ok()?;
    let mut rgb = decode_budget.image_vector(capacity)?;
    let parameters = Parameters {
        columns: columns as u16,
        rows: declared_rows,
        black_is_one,
        end_of_line,
        byte_aligned,
        end_of_block,
        damaged_rows: if k == 0 && end_of_line {
            damaged_rows
        } else {
            0
        },
    };
    if k < 0 {
        group_four::decode(&mut rgb, data, parameters, Some(decode_budget))?;
    } else {
        group_three::decode(&mut rgb, data, parameters, Some(decode_budget))?;
    }
    let actual_rows = u32::try_from(rgb.len() / (usize::from(columns as u16) * 3)).ok()?;
    if actual_rows < layout.required_rows()? {
        return None;
    }
    if width == columns && height == actual_rows {
        return Some(rgb);
    }
    layout.image_rgb(decode_budget, &rgb, black_is_one)
}
