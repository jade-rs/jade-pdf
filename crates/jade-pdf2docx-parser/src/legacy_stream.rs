//! 未検査の互換経路でも、直接Lengthが示すbinary本文の境界を優先する。

/// 本文と次の走査位置を返し、Lengthが使えない旧入力はmarker検索へ戻す。
pub(crate) fn payload<'a>(
    bytes: &'a [u8],
    dictionary: &[u8],
    position: usize,
) -> Option<(&'a [u8], usize)> {
    let mut start = position.checked_add(b"stream".len())?;
    if bytes.get(start) == Some(&b'\r') {
        start += 1;
    }
    if bytes.get(start) == Some(&b'\n') {
        start += 1;
    }
    if let Some(length) =
        crate::metadata::value(dictionary, b"Length").and_then(crate::syntax::integer)
        && let Some(end) = start.checked_add(length).filter(|end| *end <= bytes.len())
    {
        let mut marker = end;
        while bytes
            .get(marker)
            .is_some_and(|byte| crate::syntax::whitespace(*byte))
        {
            marker += 1;
        }
        if bytes[marker..].starts_with(b"endstream") {
            let next = marker + b"endstream".len();
            if bytes
                .get(next)
                .is_none_or(|byte| crate::syntax::whitespace(*byte) || b"()<>[]{}/%".contains(byte))
            {
                return Some((&bytes[start..end], next));
            }
        }
    }
    let relative = bytes
        .get(start..)?
        .windows(b"endstream".len())
        .position(|window| window == b"endstream")?;
    let marker = start + relative;
    let mut end = marker;
    while end > start && matches!(bytes.get(end - 1), Some(b'\n') | Some(b'\r')) {
        end -= 1;
    }
    Some((&bytes[start..end], marker + b"endstream".len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Lengthを持たない互換入力と不正Lengthは従来のmarker境界へ戻す。
    #[test]
    fn legacy_marker_fallback_is_preserved() {
        for dictionary in [
            b"<< >>".as_slice(),
            b"<< /Length 999 >>",
            b"<< /Length 7 0 R >>",
        ] {
            assert_eq!(
                payload(b"stream\na\r\nendstream", dictionary, 0),
                Some((b"a".as_slice(), 19))
            );
        }
        assert!(payload(b"stream", b"<< >>", 0).is_none());
        assert!(payload(b"", b"<< /Length 0 >>", usize::MAX).is_none());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 任意byteと本文内markerを保持し、空本文でも次の走査位置が一致する。
        #[test]
        fn direct_length_preserves_arbitrary_binary(data in prop::collection::vec(any::<u8>(), 0..128)) {
            for source in [data.as_slice(), b"", b"endstream\r\n"] {
                let dictionary = format!("<< /Length {} >>", source.len());
                let mut bytes = b"stream\r\n".to_vec();
                bytes.extend_from_slice(source);
                bytes.extend_from_slice(b"\r\nendstream endobj");
                prop_assert_eq!(payload(&bytes, dictionary.as_bytes(), 0), Some((source, 19 + source.len())));
            }
        }
    }
}
