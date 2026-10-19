//! 水平の埋め込み Encoding CMap を借用走査し、1〜4 byte code を CID に変換する。
use crate::{
    decode::{DecodeBudget, decode_zlib},
    metadata,
    syntax::{Lexer, Token},
};
use std::{borrow::Cow, sync::Arc};

/// 一つの CMap で展開する code 数の上限。
const MAX_CODES: usize = 65_536;
/// glyph ごとの codespace 比較数を制限する。
const MAX_SPACES: usize = 256;

/// 同じ byte 数の合法 character code の範囲。
#[derive(Debug, Clone, Copy)]
struct Space {
    /// 先頭 code の big endian 値。
    first: u32,
    /// 末尾 code の big endian 値。
    last: u32,
    /// code の byte 数。
    width: u8,
}

impl Space {
    /// 同じ長さの prefix に重なりがあるかを byte ごとの区間で検査する。
    fn overlaps_prefix(self, other: Self) -> bool {
        let common = self.width.min(other.width);
        (0..common).all(|i| {
            let a = u32::from(self.width - 1 - i) * 8;
            let b = u32::from(other.width - 1 - i) * 8;
            ((self.first >> a) & 255) <= ((other.last >> b) & 255)
                && ((other.first >> b) & 255) <= ((self.last >> a) & 255)
        })
    }

    /// codespace の各 byte の境界内に入るかを調べる。
    fn contains(self, code: u32, width: u8) -> bool {
        width == self.width
            && (0..width).all(|i| {
                let shift = u32::from(i) * 8;
                let byte = (code >> shift) & 255;
                ((self.first >> shift) & 255) <= byte && byte <= ((self.last >> shift) & 255)
            })
    }
}

/// 上書き順を保持する展開済み character code と CID。
#[derive(Debug)]
struct Mapping {
    /// big endian の character code。
    code: u32,
    /// descendant の glyph 選択に使う CID。
    cid: u16,
    /// code の byte 数。
    width: u8,
    /// 後の mapping を優先するための宣言順。
    order: usize,
}

/// 同じ Font metrics の clone 間で共有する Encoding 対応表。
#[derive(Debug, Clone)]
pub(crate) struct Encoding {
    /// 合法 code の byte 幅と範囲。
    spaces: Arc<Vec<Space>>,
    /// byte 幅/code で整列し、重複を除いた CID 対応表。
    mappings: Arc<Vec<Mapping>>,
}

impl Encoding {
    /// 検査済みの元 PDF の stream のみから独立した Encoding を読む。
    pub(crate) fn read(budget: &DecodeBudget, bytes: &[u8], number: u32) -> Option<Self> {
        if budget.is_stopped() {
            return None;
        }
        let index = budget.stream_index(bytes)?;
        let (dictionary, body) = index.for_object(bytes, number)?;
        if metadata::value(dictionary, b"UseCMap").is_some()
            || metadata::value(dictionary, b"WMode").is_some_and(|v| v != b"0")
            || metadata::value(dictionary, b"DecodeParms").is_some_and(|v| v != b"null")
        {
            return None;
        }
        let body = match metadata::value(dictionary, b"Filter") {
            None => Cow::Borrowed(body),
            Some(value) if metadata::is_name(value, b"FlateDecode") => {
                Cow::Owned(decode_zlib(body, budget)?)
            }
            _ => return None,
        };
        Self::parse(budget, &body)
    }

    /// 二回の借用走査で必要容量を先に数え、予算内の配列だけを作る。
    fn parse(budget: &DecodeBudget, body: &[u8]) -> Option<Self> {
        let mut spaces_count = 0usize;
        let mut mappings_count = 0usize;
        scan(
            body,
            |_| {
                spaces_count += 1;
                (spaces_count <= MAX_SPACES).then_some(())
            },
            |range, _| {
                mappings_count =
                    mappings_count.checked_add((range.last - range.first) as usize + 1)?;
                (mappings_count <= MAX_CODES).then_some(())
            },
        )?;
        if spaces_count == 0 || spaces_count > MAX_SPACES {
            return None;
        }
        budget.charge_font_metrics_bytes(
            spaces_count * size_of::<Space>() + mappings_count * size_of::<Mapping>(),
        )?;
        let mut spaces = budget.try_vector_capacity(spaces_count)?;
        let mut mappings = budget.try_vector_capacity(mappings_count)?;
        scan(
            body,
            |space| {
                if spaces
                    .iter()
                    .any(|previous: &Space| previous.overlaps_prefix(space))
                {
                    return None;
                }
                spaces.push(space);
                Some(())
            },
            |range, first_cid| {
                for code in range.first..=range.last {
                    mappings.push(Mapping {
                        code,
                        cid: (first_cid + code - range.first) as u16,
                        width: range.width,
                        order: mappings.len(),
                    });
                }
                Some(())
            },
        )?;
        if mappings
            .iter()
            .any(|m| !spaces.iter().any(|s| s.contains(m.code, m.width)))
        {
            return None;
        }
        mappings.sort_unstable_by(|a, b| {
            (a.width, a.code)
                .cmp(&(b.width, b.code))
                .then(b.order.cmp(&a.order))
        });
        mappings.dedup_by(|a, b| a.width == b.width && a.code == b.code);
        Some(Self {
            spaces: budget.budgeted_arc(spaces)?,
            mappings: budget.budgeted_arc(mappings)?,
        })
    }

    /// codespace の短い幅から照合し、未割当の合法 code は CID 0 に対応付ける。
    pub(crate) fn next(&self, bytes: &mut &[u8]) -> Option<(usize, bool)> {
        let mut code = 0u32;
        for width in 1..=4u8 {
            code = (code << 8) | u32::from(*bytes.get(usize::from(width - 1))?);
            if self.spaces.iter().any(|space| space.contains(code, width)) {
                let cid = self
                    .mappings
                    .binary_search_by_key(&(width, code), |m| (m.width, m.code))
                    .map_or(0, |index| self.mappings[index].cid);
                *bytes = &bytes[usize::from(width)..];
                return Some((usize::from(cid), width == 1 && code == 32));
            }
        }
        None
    }
}

/// CMap の section 宣言と実際の entry 数を検査し、範囲を通知する。
fn scan(
    body: &[u8],
    mut space: impl FnMut(Space) -> Option<()>,
    mut mapping: impl FnMut(Space, u32) -> Option<()>,
) -> Option<()> {
    let mut lexer = Lexer {
        bytes: body,
        cursor: 0,
    };
    let mut previous = None;
    while let Some(token) = lexer.next() {
        match token {
            Token::Name(b"WMode") => {
                if !matches!(lexer.next(), Some(Token::Word(b"0"))) {
                    return None;
                }
                previous = None;
            }
            Token::Word(
                b"usecmap" | b"beginnotdefchar" | b"beginnotdefrange" | b"beginbfchar"
                | b"beginbfrange",
            )
            | Token::Name(b"UseCMap") => return None,
            Token::Word(
                operator @ (b"begincodespacerange" | b"begincidchar" | b"begincidrange"),
            ) => {
                let count = decimal(previous?)? as usize;
                if count > MAX_CODES {
                    return None;
                }
                for _ in 0..count {
                    let (first, width) = code(&mut lexer)?;
                    let (last, last_width) = if operator == b"begincidchar" {
                        (first, width)
                    } else {
                        code(&mut lexer)?
                    };
                    if width != last_width || last < first {
                        return None;
                    }
                    let range = Space { first, last, width };
                    if operator == b"begincodespacerange" {
                        if !(0..width)
                            .all(|i| ((first >> (i * 8)) & 255) <= ((last >> (i * 8)) & 255))
                        {
                            return None;
                        }
                        space(range)?;
                    } else {
                        let Some(Token::Word(value)) = lexer.next() else {
                            return None;
                        };
                        let cid = decimal(value)?;
                        if cid.checked_add(last - first)? > 65_535 {
                            return None;
                        }
                        mapping(range, cid)?;
                    }
                }
                let end: &[u8] = if operator == b"begincodespacerange" {
                    b"endcodespacerange".as_slice()
                } else if operator == b"begincidchar" {
                    b"endcidchar"
                } else {
                    b"endcidrange"
                };
                if !matches!(lexer.next(), Some(Token::Word(value)) if value == end) {
                    return None;
                }
                previous = None;
            }
            Token::Word(b"endcodespacerange" | b"endcidchar" | b"endcidrange") => return None,
            Token::Word(value) => previous = Some(value),
            _ => previous = None,
        }
    }
    Some(())
}

/// 1〜4 byte の偶数桁 hex code を空白を許して追加 allocation なしに読む。
fn code(lexer: &mut Lexer<'_>) -> Option<(u32, u8)> {
    let (token, span) = lexer.next_spanned()?;
    if !matches!(token, Token::Other) {
        return None;
    }
    let raw = &lexer.bytes[span];
    if raw.first() != Some(&b'<') || raw.last() != Some(&b'>') {
        return None;
    }
    let mut value = 0u32;
    let mut digits = 0u8;
    for &byte in &raw[1..raw.len() - 1] {
        if byte.is_ascii_whitespace() {
            continue;
        }
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        };
        if digits == 8 {
            return None;
        }
        value = (value << 4) | u32::from(digit);
        digits += 1;
    }
    (digits != 0 && digits.is_multiple_of(2)).then_some((value, digits / 2))
}

/// 符号なし decimal 整数を overflow と余計な字句を拒否して読む。
fn decimal(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    bytes.iter().try_fold(0u32, |value, byte| {
        byte.is_ascii_digit().then_some(())?;
        value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 二つのArcは空mappingでも本体とcounterを確保し、途中超過を成功扱いしない。
    #[test]
    fn owned_allocation_encoding_arcs_include_empty_mapping() {
        let body = b"1 begincodespacerange <00> <FF> endcodespacerange";
        let arc = size_of::<Vec<Space>>() + 2 * size_of::<usize>();
        let payload = size_of::<Space>();
        let total = (payload + 2 * arc) as u64;
        for limit in [(payload + arc - 1) as u64, total - 1, total] {
            let budget = DecodeBudget::new(&crate::ParseLimits {
                max_total_allocation_bytes: limit,
                ..Default::default()
            });
            let encoding = Encoding::parse(&budget, body);
            assert_eq!(encoding.is_some(), limit == total);
            if let Some(encoding) = encoding {
                let clone = encoding.clone();
                assert!(Arc::ptr_eq(&encoding.spaces, &clone.spaces));
                assert!(Arc::ptr_eq(&encoding.mappings, &clone.mappings));
                assert!(budget.try_vector_capacity::<u8>(1).is_none());
            }
            let actual = if limit < (payload + arc) as u64 {
                (payload + arc) as u64
            } else {
                total + u64::from(limit == total)
            };
            assert!(
                matches!(budget.check(), Err(crate::ParseError::ResourceLimit {
                resource: "total allocation bytes", actual: found, ..
            }) if found == actual)
            );
            assert!(Encoding::parse(&budget, body).is_none());
        }
    }

    /// 短い code と長い code の prefix が重なる不正 codespace は採用しない。
    #[test]
    fn ambiguous_prefixes_are_rejected() {
        for body in [
            "2 begincodespacerange <00> <FF> <8100> <81FF> endcodespacerange",
            "2 begincodespacerange <8100> <81FF> <812000> <8120FF> endcodespacerange",
            "2 begincodespacerange <00> <7F> <70> <FF> endcodespacerange",
        ] {
            assert!(
                Encoding::parse(&DecodeBudget::default(), body.as_bytes()).is_none(),
                "{body}"
            );
        }
    }

    /// 要求容量ちょうどは許し、1 byte 不足なら配列を作らず停止する。
    #[test]
    fn storage_limit_is_inclusive_and_cumulative() {
        let body = b"1 begincodespacerange <00> <FF> endcodespacerange 1 begincidrange <20> <22> 100 endcidrange";
        let capacity = size_of::<Space>() + 3 * size_of::<Mapping>();
        let budget = DecodeBudget::new(&crate::ParseLimits {
            max_total_font_metrics_bytes: capacity as u64,
            ..Default::default()
        });
        assert!(Encoding::parse(&budget, body).is_some());
        assert!(Encoding::parse(&budget, body).is_none());
        assert!(budget.is_stopped());
        let denied = DecodeBudget::new(&crate::ParseLimits {
            max_total_font_metrics_bytes: capacity as u64 - 1,
            ..Default::default()
        });
        assert!(Encoding::parse(&denied, body).is_none());
        assert!(denied.is_stopped());
    }

    /// range の後の明示 mapping が勝ち、Unicode に関係なく source32 に Tw を適用する。
    #[test]
    fn later_mapping_wins_and_unmapped_codes_use_cid_zero() {
        let encoding = Encoding::parse(&DecodeBudget::default(), b"1 begincodespacerange <00> <FF> endcodespacerange 1 begincidrange <20> <22> 100 endcidrange 1 begincidchar <21> 500 endcidchar").unwrap();
        let mut bytes = [32, 33, 34, 35].as_slice();
        assert_eq!(encoding.next(&mut bytes), Some((100, true)));
        assert_eq!(encoding.next(&mut bytes), Some((500, false)));
        assert_eq!(encoding.next(&mut bytes), Some((102, false)));
        assert_eq!(encoding.next(&mut bytes), Some((0, false)));
        assert_eq!(encoding.next(&mut bytes), None);
    }

    /// byte ごとの codespace 境界を使い、未完の code では入力を消費しない。
    #[test]
    fn rectangular_codespace_and_truncated_input_are_checked() {
        let encoding = Encoding::parse(
            &DecodeBudget::default(),
            b"1 begincodespacerange <3030> <3232> endcodespacerange",
        )
        .unwrap();
        for original in [vec![0x30], vec![0x30, 0x40], vec![0x31, 0x00]] {
            let mut bytes = original.as_slice();
            assert_eq!(encoding.next(&mut bytes), None);
            assert_eq!(bytes, original);
        }
        let mut bytes = [0x31, 0x32].as_slice();
        assert_eq!(encoding.next(&mut bytes), Some((0, false)));
    }

    /// section 数・hex 幅・CID 範囲・UseCMap・縦書きの未対応値を拒否する。
    #[test]
    fn malformed_or_unsupported_sections_are_rejected() {
        let prefix = "1 begincodespacerange <00> <FF> endcodespacerange ";
        for suffix in [
            "1 begincidchar <20> 65536 endcidchar",
            "1 begincidchar <20> -1 endcidchar",
            "1 begincidrange <20> <22> 65534 endcidrange",
            "1 begincidrange <22> <20> 0 endcidrange",
            "1 begincidrange <20> <0021> 0 endcidrange",
            "2 begincidchar <20> 1 endcidchar",
            "1 begincidchar <2> 1 endcidchar",
            "1 begincidchar <GG> 1 endcidchar",
            "1 begincidchar <0100000000> 1 endcidchar",
            "1 begincidchar <0100> 1 endcidchar",
            "1 begincidchar <20> 1 <21> 2 endcidchar",
            "/Identity-H usecmap",
            "/WMode 1 def",
            "1 beginnotdefchar <20> 1 endnotdefchar",
        ] {
            assert!(
                Encoding::parse(
                    &DecodeBudget::default(),
                    format!("{prefix}{suffix}").as_bytes()
                )
                .is_none(),
                "{suffix}"
            );
        }
    }

    /// 最大 CID 範囲は処理でき、上書き前の展開数が上限を超えたら確保前に拒否する。
    #[test]
    fn mapping_count_limit_is_inclusive_and_precedes_allocation() {
        let body = "1 begincodespacerange <0000> <FFFF> endcodespacerange 1 begincidrange <0000> <FFFF> 0 endcidrange";
        let encoding = Encoding::parse(&DecodeBudget::default(), body.as_bytes()).unwrap();
        let mut bytes = [255, 255].as_slice();
        assert_eq!(encoding.next(&mut bytes), Some((65535, false)));
        let denied = DecodeBudget::new(&crate::ParseLimits {
            max_total_font_metrics_bytes: 0,
            ..Default::default()
        });
        let oversized = format!("{body} 1 begincidchar <0000> 1 endcidchar");
        assert!(Encoding::parse(&denied, oversized.as_bytes()).is_none());
        assert!(!denied.is_stopped());
    }

    /// codespace の最大件数は許し、余計な entry は容量予約前に拒否する。
    #[test]
    fn codespace_count_limit_is_inclusive_and_precedes_allocation() {
        let entries = (0..256)
            .map(|code| format!("<{code:02X}> <{code:02X}> "))
            .collect::<String>();
        let body = format!("256 begincodespacerange {entries} endcodespacerange");
        let budget = DecodeBudget::new(&crate::ParseLimits {
            max_total_font_metrics_bytes: (256 * size_of::<Space>()) as u64,
            ..Default::default()
        });
        assert!(Encoding::parse(&budget, body.as_bytes()).is_some());
        let oversized = format!("257 begincodespacerange {entries} <00> <00> endcodespacerange");
        let denied = DecodeBudget::new(&crate::ParseLimits {
            max_total_font_metrics_bytes: 0,
            ..Default::default()
        });
        assert!(Encoding::parse(&denied, oversized.as_bytes()).is_none());
        assert!(!denied.is_stopped());
    }

    /// 正式に検査した stream 範囲を持つ予算を作る。
    fn checked_budget(bytes: &[u8], limits: &crate::ParseLimits) -> DecodeBudget {
        let budget = DecodeBudget::new(limits);
        let streams = crate::structure::validate_structure(
            std::path::Path::new("encoding.pdf"),
            bytes,
            1,
            limits,
            &budget,
        )
        .unwrap();
        budget.cache_streams(bytes, &streams);
        budget
    }

    /// binary を含む一つの Encoding object を作る。
    fn stream_document(body: &[u8], entries: &str) -> Vec<u8> {
        let mut bytes = format!(
            "%PDF-1.7\n1 0 obj\n<< /Length {} {entries} >>\nstream\n",
            body.len()
        )
        .into_bytes();
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendstream\nendobj\n%%EOF");
        bytes
    }

    /// 圧縮本文も元入力に結び付いた境界から解決し、展開上限を回避しない。
    #[test]
    fn checked_compressed_stream_obeys_decode_limit_and_source_identity() {
        use std::io::Write;
        let body =
            b"1 begincodespacerange <00> <FF> endcodespacerange 1 begincidchar <41> 90 endcidchar";
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(body).unwrap();
        let bytes = stream_document(&encoder.finish().unwrap(), "/Filter /FlateDecode");
        let limits = crate::ParseLimits {
            max_decoded_stream_bytes: body.len() as u64,
            ..Default::default()
        };
        let budget = checked_budget(&bytes, &limits);
        let encoding = Encoding::read(&budget, &bytes, 1).unwrap();
        let mut code = b"A".as_slice();
        assert_eq!(encoding.next(&mut code), Some((90, false)));
        assert!(Encoding::read(&budget, &bytes.clone(), 1).is_none());
        assert!(Encoding::read(&DecodeBudget::default(), &bytes, 1).is_none());
        let denied = checked_budget(
            &bytes,
            &crate::ParseLimits {
                max_decoded_stream_bytes: body.len() as u64 - 1,
                ..Default::default()
            },
        );
        assert!(Encoding::read(&denied, &bytes, 1).is_none());
        assert!(denied.is_stopped());
        assert!(Encoding::read(&denied, &bytes, 1).is_none());
    }

    /// 未対応 Filter・継承・縦書き・predictor を水平の plain CMap として読まない。
    #[test]
    fn unsupported_stream_dictionary_is_rejected() {
        let body = b"1 begincodespacerange <00> <FF> endcodespacerange";
        for entries in [
            "/Filter /LZWDecode",
            "/Filter /Unknown /Note (FlateDecode)",
            "/UseCMap /Identity-H",
            "/WMode 1",
            "/DecodeParms << /Predictor 12 >>",
        ] {
            let bytes = stream_document(body, entries);
            let budget = checked_budget(&bytes, &crate::ParseLimits::default());
            assert!(Encoding::read(&budget, &bytes, 1).is_none(), "{entries}");
            assert!(!budget.is_stopped());
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, ..ProptestConfig::default() })]
        /// 四 byte range の展開は独立した加算モデルと一致し、CID を overflow しない。
        #[test]
        fn four_byte_ranges_match_numeric_model(first in 0u32..=u32::MAX-100, count in 1u32..100, cid in 0u32..65000) {
            let last = first + count - 1;
            let body = format!("1 begincodespacerange <00000000> <FFFFFFFF> endcodespacerange 1 begincidrange <{first:08X}> <{last:08X}> {cid} endcidrange");
            let encoding = Encoding::parse(&DecodeBudget::default(), body.as_bytes()).unwrap();
            for offset in 0..count {
                let raw = (first + offset).to_be_bytes();
                let mut bytes = raw.as_slice();
                prop_assert_eq!(encoding.next(&mut bytes), Some(((cid + offset) as usize, false)));
                prop_assert!(bytes.is_empty());
            }
        }
    }
}
