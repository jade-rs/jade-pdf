//! 対応済み二段filterの外側を有界に復元し、既存画像経路へ渡す。
use super::{
    DecodeBudget, ObjectBytes, decode_zlib, image_attribute, image_predictor,
    object_body_for_number,
};
use crate::{image_filter::ImageFilter, metadata, syntax::name_is};

/// 既存JPEG経路が適用できない明示的な色変換は黙って無視しない。
fn supported_color_transform(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
    jpeg: &[u8],
) -> Option<()> {
    if metadata::value(jpeg, b"ColorTransform").is_none() {
        return Some(());
    }
    let value = image_attribute(budget, pdf, jpeg, b"ColorTransform")?;
    if super::image_icc::is_null(&value) {
        return Some(());
    }
    let mut lexer = crate::syntax::Lexer {
        bytes: &value,
        cursor: 0,
    };
    let crate::syntax::Token::Word(word) = lexer.next()? else {
        return None;
    };
    let transform = std::str::from_utf8(word).ok()?.parse::<u8>().ok()?;
    if lexer.next().is_some() || transform > 1 {
        return None;
    }
    if transform == 1 {
        return Some(());
    }
    let color = image_attribute(budget, pdf, dictionary, b"ColorSpace")?;
    (metadata::is_name(&color, b"DeviceGray")
        || matches!(
            super::image_icc::components(budget, pdf, dictionary),
            Some(1 | 4)
        ))
    .then_some(())
}

/// 対応済みの二段filterだけを内側filterとして分類する。
pub(super) fn terminal_filter(dictionary: &[u8]) -> Option<ImageFilter> {
    let value = metadata::value(dictionary, b"Filter")?;
    let mut filters = metadata::array_entries(value, 2)?;
    let outer = filters.next()??;
    let inner = filters.next()??;
    if filters.next().is_some() {
        return None;
    }
    if metadata::is_name(outer, b"FlateDecode") && metadata::is_name(inner, b"DCTDecode") {
        Some(ImageFilter::Dct)
    } else if metadata::is_name(outer, b"ASCII85Decode") && metadata::is_name(inner, b"FlateDecode")
    {
        Some(ImageFilter::Flate)
    } else {
        None
    }
}

/// DecodeParmsの二要素を参照解決し、欠落/nullだけは空辞書にする。
fn parameters<'a>(
    budget: &DecodeBudget,
    pdf: &'a [u8],
    value: Option<&'a [u8]>,
) -> Option<[ObjectBytes<'a>; 2]> {
    let empty = || ObjectBytes::Borrowed(b"<< >>".as_slice());
    let Some(value) = value else {
        return Some([empty(), empty()]);
    };
    if super::image_icc::is_null(value) {
        return Some([empty(), empty()]);
    }
    let mut entries = metadata::array_entries(value, 2)?;
    let mut result = [empty(), empty()];
    for output in &mut result {
        let entry = entries.next()??;
        let resolved = if let Some(number) = budget.reference(pdf, entry) {
            object_body_for_number(budget, pdf, number)?
        } else {
            ObjectBytes::Borrowed(entry)
        };
        *output = if super::image_icc::is_null(&resolved) {
            empty()
        } else {
            if !metadata::dictionary_entries_complete(&resolved) {
                return None;
            }
            resolved
        };
    }
    entries.next().is_none().then_some(result)
}

/// 外側だけを展開し、内側パラメータを保持した単一filter辞書を作る。
pub(super) fn unwrap(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
    data: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let terminal = terminal_filter(dictionary)?;
    if !metadata::dictionary_entries_complete(dictionary) {
        return None;
    }
    let mut filter_count = 0;
    let mut parms_count = 0;
    for (key, _) in metadata::entries(dictionary)? {
        if name_is(key, b"Filter") {
            filter_count += 1;
        }
        if name_is(key, b"DecodeParms") {
            parms_count += 1;
        }
    }
    if filter_count != 1 || parms_count > 1 {
        return None;
    }
    let value = if metadata::value(dictionary, b"DecodeParms").is_some() {
        Some(image_attribute(budget, pdf, dictionary, b"DecodeParms")?)
    } else {
        None
    };
    let [outer, inner] = parameters(budget, pdf, value.as_deref())?;
    let (prefix, decoded): (&[u8], Vec<u8>) = match terminal {
        ImageFilter::Dct => {
            supported_color_transform(budget, pdf, dictionary, &inner)?;
            let mut decoded = decode_zlib(data, budget)?;
            image_predictor::restore(budget, pdf, &outer, &mut decoded)?;
            (b"<< /Filter /DCTDecode /DecodeParms ", decoded)
        }
        ImageFilter::Flate => {
            if metadata::entries(&outer)?.next().is_some() {
                return None;
            }
            (
                b"<< /Filter /FlateDecode /DecodeParms ",
                super::image_ascii85::decode(data, budget)?,
            )
        }
        _ => return None,
    };
    let retained = || {
        metadata::entries(dictionary)
            .into_iter()
            .flatten()
            .filter(|(key, _)| !name_is(key, b"Filter") && !name_is(key, b"DecodeParms"))
    };
    let mut size = prefix.len().checked_add(inner.len())?.checked_add(4)?;
    for (key, value) in retained() {
        size = size
            .checked_add(key.len())?
            .checked_add(value.len())?
            .checked_add(3)?;
    }
    let mut normalized = budget.metadata_vector(size)?;
    normalized.extend_from_slice(prefix);
    normalized.extend_from_slice(&inner);
    normalized.push(b'\n');
    for (key, value) in retained() {
        normalized.push(b'/');
        normalized.extend_from_slice(key);
        normalized.push(b' ');
        normalized.extend_from_slice(value);
        normalized.push(b'\n');
    }
    normalized.extend_from_slice(b">>");
    Some((normalized, decoded))
}
