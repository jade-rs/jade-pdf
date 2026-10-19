//! PNG保存・符号化の予算付き配列と借用chunk処理。
use crate::decode::DecodeBudget;
use flate2::{Compression, write::ZlibEncoder};
use std::io::{self, Write};

/// PNGを予約容量内で複製する。
pub(super) fn copy(budget: &DecodeBudget, bytes: &[u8]) -> Option<Vec<u8>> {
    let mut copied = budget.image_vector(bytes.len())?;
    copied.extend_from_slice(bytes);
    Some(copied)
}

/// 再確保時の全容量を累積へ数え、停止後は既存容量にも追記しない。
pub(super) fn append(budget: &DecodeBudget, bytes: &mut Vec<u8>, input: &[u8]) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    let required = bytes.len().checked_add(input.len())?;
    if required > bytes.capacity() {
        let capacity = bytes.capacity().checked_mul(2)?.max(required).max(64);
        let mut fresh = budget.image_vector(capacity)?;
        fresh.append(bytes);
        *bytes = fresh;
    }
    bytes.extend_from_slice(input);
    Some(())
}

/// zlib出力を予算内に保存するwriter。
struct Buffer<'a> {
    /// 呼び出し全体で共有する累積予算。
    budget: &'a DecodeBudget,
    /// 予約済みの圧縮出力。
    bytes: &'a mut Vec<u8>,
}

impl Write for Buffer<'_> {
    /// 書き込み前に必要容量を予約する。
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        append(self.budget, self.bytes, input)
            .ok_or_else(|| io::Error::other("image storage unavailable"))?;
        Ok(input.len())
    }

    /// bufferには外部のflush対象がない。
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// 行を借用して圧縮し、scanlineやCRC入力の複製なしでPNGを生成する。
pub(super) fn encode(
    budget: &DecodeBudget,
    width: u32,
    height: u32,
    pixels: &[u8],
    channels: usize,
    color_type: u8,
) -> Option<Vec<u8>> {
    encode_with_buffer(
        budget,
        width,
        height,
        pixels,
        channels,
        color_type,
        &mut Vec::new(),
    )
}

/// JPEG専用の予約済み圧縮容量を再利用してRGB PNGを生成する。
pub(super) fn encode_jpeg(
    budget: &DecodeBudget,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Option<Vec<u8>> {
    budget.with_jpeg_compressed(|compressed| {
        encode_with_buffer(budget, width, height, pixels, 3, 2, compressed)
    })
}

/// 圧縮用配列の拡張と完成PNGの容量を、それぞれ予算へ予約する。
fn encode_with_buffer(
    budget: &DecodeBudget,
    width: u32,
    height: u32,
    pixels: &[u8],
    channels: usize,
    color_type: u8,
    compressed: &mut Vec<u8>,
) -> Option<Vec<u8>> {
    budget.check_image_pixels(width, height)?;
    let row_len = (width as usize).checked_mul(channels)?;
    if row_len == 0 || height == 0 || pixels.len() < row_len.checked_mul(height as usize)? {
        return None;
    }
    let mut encoder = ZlibEncoder::new(
        Buffer {
            budget,
            bytes: compressed,
        },
        Compression::default(),
    );
    for row in pixels[..row_len * height as usize].chunks_exact(row_len) {
        encoder.write_all(&[0]).ok()?;
        encoder.write_all(row).ok()?;
    }
    let compressed = encoder.finish().ok()?.bytes;
    u32::try_from(compressed.len()).ok()?;
    let mut png = budget.image_vector(57usize.checked_add(compressed.len())?)?;
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = color_type;
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", compressed);
    chunk(&mut png, b"IEND", &[]);
    Some(png)
}

/// 必要容量を予約済みのPNGへchunkを追記する。
fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let mut crc = u32::MAX;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    png.extend_from_slice(&(!crc).to_be_bytes());
}
