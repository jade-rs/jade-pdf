//! 文字描画operandの復号配列をmetadata予算内で保存する。
use super::ToUnicodeCMap;
use crate::{decode::DecodeBudget, lossy_chunks::LossyChunks};
use encoding_rs::{BIG5, CoderResult, EUC_KR, Encoding, GBK, SHIFT_JIS};

/// byte列と必要なhex/幅一覧・出力を、確保前の累積予約で復号する。
pub(super) fn decode(
    budget: &DecodeBudget,
    raw: &[u8],
    cmap: &ToUnicodeCMap,
    encoding: Option<&str>,
) -> Option<String> {
    let bytes = super::pdf_string_bytes(budget, raw)?;
    decode_bytes(budget, &bytes, raw.first() == Some(&b'('), cmap, encoding)
}

/// glyph境界を決めた呼び出し元からbyte列を受け取り、Unicode mappingを保持する。
pub(crate) fn decode_bytes(
    budget: &DecodeBudget,
    bytes: &[u8],
    literal: bool,
    cmap: &ToUnicodeCMap,
    encoding: Option<&str>,
) -> Option<String> {
    if !cmap.is_empty() {
        let mut hex = budget.metadata_vector(bytes.len().checked_mul(2)?)?;
        for &byte in bytes {
            hex.push(b"0123456789ABCDEF"[usize::from(byte >> 4)]);
            hex.push(b"0123456789ABCDEF"[usize::from(byte & 15)]);
        }
        let hex = std::str::from_utf8(&hex).ok()?;
        if cmap.key_is_allowed(hex)
            && let Some(value) = cmap.mappings.get(hex)
        {
            return copy(budget, value);
        }
        let count = cmap
            .mappings
            .keys()
            .count()
            .checked_add(cmap.code_space_ranges.len())?;
        let mut widths = budget.metadata_vector(count)?;
        widths.extend(cmap.code_space_ranges.iter().map(|range| range.width));
        widths.extend(
            cmap.mappings
                .keys()
                .filter(|key| cmap.key_is_allowed(key))
                .map(String::len),
        );
        widths.retain(|width| *width > 0);
        widths.sort_unstable_by(|a, b| b.cmp(a));
        widths.dedup();
        let mut output_length = 0usize;
        if visit_mappings(hex, cmap, &widths, |value| {
            output_length = output_length.checked_add(value.len())?;
            Some(())
        })
        .is_some()
        {
            let mut output = budget.metadata_vector(output_length)?;
            visit_mappings(hex, cmap, &widths, |value| {
                output.extend_from_slice(value.as_bytes());
                Some(())
            })?;
            return String::from_utf8(output).ok();
        }
    }
    if literal {
        return collect(budget, |emit| {
            for part in LossyChunks::new(bytes) {
                emit(part.as_bytes())?;
            }
            Some(())
        });
    }
    if matches!(
        encoding,
        Some(
            "UniGB-UTF16-H"
                | "UniGB-UCS2-H"
                | "UniCNS-UTF16-H"
                | "UniCNS-UCS2-H"
                | "UniKS-UTF16-H"
                | "UniKS-UCS2-H"
        )
    ) && char::decode_utf16(
        bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]])),
    )
    .all(|scalar| scalar.is_ok())
    {
        return utf16(budget, bytes, false);
    }
    let external = match encoding {
        Some("GBK-EUC-H" | "GBK2K-H") => Some(GBK),
        Some("ETen-B5-H") => Some(BIG5),
        Some("KSCms-UHC-H") => Some(EUC_KR),
        _ => None,
    };
    if let Some(encoding) = external {
        return collect(budget, |emit| visit_encoding(bytes, encoding, emit));
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        return utf16(budget, bytes, true);
    }
    collect(budget, |emit| visit_encoding(bytes, SHIFT_JIS, emit))
}

/// greedyな長いcode優先で、全codeが対応する場合だけUnicodeを通知する。
fn visit_mappings(
    hex: &str,
    cmap: &ToUnicodeCMap,
    widths: &[usize],
    mut emit: impl FnMut(&str) -> Option<()>,
) -> Option<()> {
    let mut cursor = 0usize;
    while cursor < hex.len() {
        let (width, value) = widths.iter().find_map(|&width| {
            let end = cursor.checked_add(width)?;
            let key = hex.get(cursor..end)?;
            cmap.key_is_allowed(key)
                .then(|| cmap.mappings.get(key))
                .flatten()
                .map(|value| (width, value))
        })?;
        emit(value)?;
        cursor += width;
    }
    Some(())
}

/// 結果の全UTF-8容量を1回目に数え、2回目に予約領域へ直接保存する。
fn collect(
    budget: &DecodeBudget,
    mut visit: impl FnMut(&mut dyn FnMut(&[u8]) -> Option<()>) -> Option<()>,
) -> Option<String> {
    let mut length = 0usize;
    visit(&mut |chunk| {
        length = length.checked_add(chunk.len())?;
        Some(())
    })?;
    let mut output = budget.metadata_vector(length)?;
    visit(&mut |chunk| {
        output.extend_from_slice(chunk);
        Some(())
    })?;
    String::from_utf8(output).ok()
}

/// 既存mappingの複製容量も全体を予約する。
fn copy(budget: &DecodeBudget, text: &str) -> Option<String> {
    let mut output = budget.metadata_vector(text.len())?;
    output.extend_from_slice(text.as_bytes());
    String::from_utf8(output).ok()
}

/// UTF-16を借用u16 iteratorで読み、不正surrogateは結果を保存しない。
fn utf16(budget: &DecodeBudget, bytes: &[u8], strip_bom: bool) -> Option<String> {
    let bytes = if strip_bom { &bytes[2..] } else { bytes };
    collect(budget, |emit| {
        for scalar in char::decode_utf16(
            bytes
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]])),
        ) {
            let mut utf8 = [0; 4];
            emit(scalar.ok()?.encode_utf8(&mut utf8).as_bytes())?;
        }
        Some(())
    })
}

/// 外部encodingを固定bufferで復号し、encoding_rsの可変長出力確保を避ける。
fn visit_encoding(
    bytes: &[u8],
    encoding: &'static Encoding,
    emit: &mut dyn FnMut(&[u8]) -> Option<()>,
) -> Option<()> {
    let mut decoder = encoding.new_decoder();
    let mut cursor = 0usize;
    let mut output = [0u8; 4096];
    loop {
        let (result, read, written, _) =
            decoder.decode_to_utf8(&bytes[cursor..], &mut output, true);
        emit(&output[..written])?;
        cursor += read;
        if result == CoderResult::InputEmpty {
            return Some(());
        }
        if read == 0 && written == 0 {
            return None;
        }
    }
}
