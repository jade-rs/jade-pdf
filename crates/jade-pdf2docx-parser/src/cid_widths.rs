//! 水平 Type0 の CIDFont 幅を解決し、Unicode と独立に文字を進める。
use crate::{
    font_widths::Layout,
    metadata, page_attributes,
    resource_resolver::ResourceResolver,
    syntax::{Lexer, Token},
    text,
};
use std::sync::Arc;

/// Identity の CID 空間は unsigned 16 bit である。
const CID_COUNT: usize = 65_536;
/// W の operands と個別幅を合わせた解析量の上限。
const MAX_WIDTH_VALUES: usize = CID_COUNT * 3;

/// Type0 Font の descendant が持つ水平 CID 幅。
#[derive(Debug, Clone)]
pub(crate) struct Metrics {
    /// W に含まれない CID の幅。省略時は1000 units/em。
    default: f32,
    /// 展開した幅表。局所 resource の clone で同じ allocation を共有する。
    widths: Option<Arc<Vec<f32>>>,
    /// Identity-H 以外の埋め込み可変長 Encoding。
    encoding: Option<crate::cid_encoding::Encoding>,
}

/// W の順番を保った一つの閉区間への幅指定。
struct Assignment {
    /// 最初の CID。
    first: usize,
    /// 最後の CID。
    last: usize,
    /// 区間内で共通する glyph 幅。
    width: f32,
}

impl Metrics {
    /// Encodingのcode境界を保持して、一つのCID幅と元byte列を返す。
    pub(crate) fn next_glyph<'a>(
        &self,
        remaining: &mut &'a [u8],
        size: f32,
    ) -> Option<(&'a [u8], f32, bool)> {
        let original = *remaining;
        let (cid, space) = match &self.encoding {
            Some(encoding) => encoding.next(remaining)?,
            None => {
                let code = remaining.get(..2)?;
                *remaining = &remaining[2..];
                (usize::from(u16::from_be_bytes([code[0], code[1]])), false)
            }
        };
        let code = &original[..original.len() - remaining.len()];
        let width = self
            .widths
            .as_ref()
            .map_or(self.default, |widths| widths[cid]);
        Some((code, width * size / 1000., space))
    }

    /// Type0 Font の Encoding・descendant・W/DW を検査して読む。
    #[cfg(test)]
    pub(crate) fn read<'a>(
        resolver: &mut ResourceResolver<'_, 'a>,
        dictionary: &'a [u8],
        budget: &crate::decode::DecodeBudget,
    ) -> Option<Self> {
        Self::read_with_source(resolver, dictionary, budget, &[])
    }

    /// 検査済み元 PDF を使って Encoding stream と CID 幅を独立に解決する。
    pub(crate) fn read_with_source<'a>(
        resolver: &mut ResourceResolver<'_, 'a>,
        dictionary: &'a [u8],
        budget: &crate::decode::DecodeBudget,
        bytes: &[u8],
    ) -> Option<Self> {
        if budget.is_stopped() {
            return None;
        }
        let (encoding, number) = resolver.resolve(metadata::value(dictionary, b"Encoding")?)?;
        let encoding = if metadata::is_name(encoding, b"Identity-H") {
            None
        } else {
            Some(crate::cid_encoding::Encoding::read(budget, bytes, number?)?)
        };
        let (descendants, _) =
            resolver.resolve(metadata::value(dictionary, b"DescendantFonts")?)?;
        let descendants = metadata::array_values(descendants, 1)?;
        let (descendant, _) = resolver.resolve(descendants.first()?)?;
        let (subtype, _) = resolver.resolve(metadata::value(descendant, b"Subtype")?)?;
        if !metadata::is_name(subtype, b"CIDFontType0")
            && !metadata::is_name(subtype, b"CIDFontType2")
        {
            return None;
        }
        let default = match optional_value(resolver, descendant, b"DW")? {
            Some(value) => page_attributes::number_value(value)?,
            None => 1000.,
        };
        let widths = match optional_value(resolver, descendant, b"W")? {
            Some(array) => read_widths(resolver, array, default, budget)?,
            None => None,
        };
        Some(Self {
            default,
            widths,
            encoding,
        })
    }

    /// Encoding の code ごとに CID 幅と Tc、一 byte の32にだけ Tw を適用する。
    pub(crate) fn layout(
        &self,
        budget: &crate::decode::DecodeBudget,
        raw: &[u8],
        size: f32,
        character: f32,
        word: f32,
    ) -> Option<Layout> {
        let codes = text::pdf_string_bytes(budget, raw)?;
        if self.encoding.is_none() && codes.len() % 2 != 0 {
            return None;
        }
        let mut remaining = codes.as_slice();
        let glyphs = std::iter::from_fn(|| {
            if remaining.is_empty() {
                return None;
            }
            let glyph = self
                .next_glyph(&mut remaining, size)
                .map(|(_, width, space)| (width, space));
            if glyph.is_none() {
                remaining = &[];
            }
            Some(glyph)
        });
        Layout::read(glyphs, character, word)
    }
}

/// 省略/null と参照失敗を区別して optional 値を解決する。
fn optional_value<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    dictionary: &'a [u8],
    key: &[u8],
) -> Option<Option<&'a [u8]>> {
    let Some(raw) = metadata::value(dictionary, key) else {
        return Some(None);
    };
    let (value, _) = resolver.resolve(raw)?;
    Some((!page_attributes::is_null(value)).then_some(value))
}

/// 有限の整数を unsigned 16 bit の CID として解決する。
fn cid<'a>(resolver: &mut ResourceResolver<'_, 'a>, value: &'a [u8]) -> Option<usize> {
    let (value, _) = resolver.resolve(value)?;
    let number = page_attributes::number_value(value)?;
    (number.fract() == 0. && (0. ..CID_COUNT as f32).contains(&number)).then_some(number as usize)
}

/// 二種類の W group を検査し、CID 数だけの幅表へ展開する。
fn read_widths<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    array: &'a [u8],
    default: f32,
    budget: &crate::decode::DecodeBudget,
) -> Option<Option<Arc<Vec<f32>>>> {
    let mut count = 0;
    scan_assignments(resolver, array, |_| count += 1)?;
    if count == 0 {
        return Some(None);
    }
    // 全 group の検証後、一時配列を確保する前に必要な合計容量を予約する。
    let workspace_bytes =
        count * std::mem::size_of::<Assignment>() + (CID_COUNT + 1) * std::mem::size_of::<usize>();
    budget.charge_font_width_workspace_bytes(workspace_bytes)?;
    budget.charge_font_metrics_bytes(CID_COUNT * std::mem::size_of::<f32>())?;
    let mut assignments = budget.try_vector_capacity(count)?;
    scan_assignments(resolver, array, |assignment| assignments.push(assignment))?;
    let mut widths = budget.try_vector_capacity(CID_COUNT)?;
    widths.resize(CID_COUNT, default);
    let mut successors = budget.try_vector_capacity(CID_COUNT + 1)?;
    successors.extend(0..=CID_COUNT);
    // 逆順に、未設定 CID だけを訪れる。範囲の重複回数にかかわらず各幅を一度だけ設定する。
    for assignment in assignments.into_iter().rev() {
        let mut index = next_unfilled(&mut successors, assignment.first);
        while index <= assignment.last {
            widths[index] = assignment.width;
            successors[index] = next_unfilled(&mut successors, index + 1);
            index = successors[index];
        }
    }
    Some(Some(budget.budgeted_arc(widths)?))
}

/// W の operands と個別幅を借用し、検査済みの assignment を順に通知する。
fn scan_assignments<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    array: &'a [u8],
    mut visit: impl FnMut(Assignment),
) -> Option<()> {
    let mut values = metadata::ArrayValues::new(array, MAX_WIDTH_VALUES)?;
    let mut operands = 0;
    while let Some(first) = next_operand(&mut values, &mut operands)? {
        let first = cid(resolver, first)?;
        let (second, _) = resolver.resolve(next_operand(&mut values, &mut operands)??)?;
        let mut lexer = Lexer {
            bytes: second,
            cursor: 0,
        };
        if matches!(lexer.next(), Some(Token::Open(false))) {
            let mut widths = metadata::ArrayValues::new(second, CID_COUNT - first)?;
            let mut offset = 0;
            while let Some(width) = next_operand(&mut widths, &mut operands)? {
                let (width, _) = resolver.resolve(width)?;
                visit(Assignment {
                    first: first + offset,
                    last: first + offset,
                    width: page_attributes::number_value(width)?,
                });
                offset += 1;
            }
        } else {
            let last = cid(resolver, second)?;
            if last < first {
                return None;
            }
            let (width, _) = resolver.resolve(next_operand(&mut values, &mut operands)??)?;
            visit(Assignment {
                first,
                last,
                width: page_attributes::number_value(width)?,
            });
        }
    }
    Some(())
}

/// 外側 operands と内側の個別幅を同じ inclusive 解析量上限へ数える。
fn next_operand<'a>(
    values: &mut metadata::ArrayValues<'a>,
    operands: &mut usize,
) -> Option<Option<&'a [u8]>> {
    let value = values.next_value()?;
    if value.is_some() {
        *operands += 1;
        if *operands > MAX_WIDTH_VALUES {
            return None;
        }
    }
    Some(value)
}

/// 未設定 CID の successor を反復走査し、経路圧縮して再走査量を抑える。
fn next_unfilled(successors: &mut [usize], mut index: usize) -> usize {
    let mut root = index;
    while successors[root] != root {
        root = successors[root];
    }
    while successors[index] != index {
        let next = successors[index];
        successors[index] = root;
        index = next;
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page_attributes::Objects;
    use proptest::prelude::*;

    /// 幅表配列と一時配列に加えてArcのVec本体とcounterも共通容量へ数える。
    #[test]
    fn owned_allocation_cid_width_arc_is_inclusive() {
        let total = (std::mem::size_of::<Assignment>()
            + CID_COUNT * std::mem::size_of::<f32>()
            + (CID_COUNT + 1) * std::mem::size_of::<usize>()
            + std::mem::size_of::<Vec<f32>>()
            + 2 * std::mem::size_of::<usize>()) as u64;
        for limit in [total - 1, total] {
            let budget = crate::decode::DecodeBudget::new(&crate::ParseLimits {
                max_total_allocation_bytes: limit,
                ..Default::default()
            });
            let objects = Objects::new();
            let mut resolver = ResourceResolver::new(&objects, &budget);
            let widths = read_widths(&mut resolver, b"[65 [600]]", 1000., &budget);
            assert_eq!(widths.is_some(), limit == total);
            if let Some(Some(widths)) = widths {
                let clone = Arc::clone(&widths);
                assert!(Arc::ptr_eq(&widths, &clone));
                assert_eq!(widths[65], 600.);
                assert!(budget.try_vector_capacity::<u8>(1).is_none());
            }
            assert!(
                matches!(budget.check(), Err(crate::ParseError::ResourceLimit {
                resource: "total allocation bytes", actual, ..
            }) if actual == total + u64::from(limit == total))
            );
            assert!(read_widths(&mut resolver, b"[65 [600]]", 1000., &budget).is_none());
        }
    }

    /// 直接 descendant 辞書を持つ局所 Font から metrics を読む。
    fn read(entries: &str) -> Option<Metrics> {
        let dictionary = format!(
            "<< /Encoding /Identity-H /DescendantFonts [<< /Subtype /CIDFontType2 {entries} >>] >>"
        );
        Metrics::read(
            &mut ResourceResolver::new(&Objects::new(), &crate::decode::DecodeBudget::default()),
            dictionary.as_bytes(),
            &crate::decode::DecodeBudget::default(),
        )
    }

    /// 予算超過時は CID 表を作らず、以後の読み直しも停止する。
    #[test]
    fn denied_width_payload_stops_before_allocation() {
        let budget = crate::decode::DecodeBudget::new(&crate::ParseLimits {
            max_total_font_metrics_bytes: 0,
            ..Default::default()
        });
        let dictionary = b"<< /Encoding /Identity-H /DescendantFonts [<< /Subtype /CIDFontType2 /W [65 [600]] >>] >>";
        let objects = Objects::new();
        assert!(
            Metrics::read(
                &mut ResourceResolver::new(&objects, &budget),
                dictionary,
                &budget
            )
            .is_none()
        );
        assert!(budget.is_stopped());
        assert!(
            Metrics::read(
                &mut ResourceResolver::new(&objects, &budget),
                dictionary,
                &budget
            )
            .is_none()
        );
        assert!(matches!(
            budget.check(),
            Err(crate::ParseError::ResourceLimit {
                resource: "font metrics bytes",
                limit: 0,
                actual: 262_144,
            })
        ));
    }

    /// 不正 CID・配列・数値を幅表として採用しない。
    #[test]
    fn malformed_widths_are_not_used() {
        for entries in [
            "/DW /bad",
            "/DW 1e999",
            "/W /bad",
            "/W [65]",
            "/W [65 66]",
            "/W [-1 [600]]",
            "/W [65536 [600]]",
            "/W [1.5 [600]]",
            "/W [65535 [600 600]]",
            "/W [65 64 600]",
            "/W [65 65536 600]",
            "/W [65 [/bad]]",
            "/W [65 [1e999]]",
            "/W [65 66 /bad]",
            "/W [65 66 99 67]",
            "/W 99 0 R",
            "/DW 99 0 R",
        ] {
            assert!(read(entries).is_none(), "{entries}");
        }
    }

    /// CID 0/65535 と負・ゼロの有限幅を扱い、奇数 byte 列は採用しない。
    #[test]
    fn cid_boundaries_and_incomplete_codes_are_checked() {
        let metrics = read("/W [0 [0] 65535 [-200]]").unwrap();
        let layout = metrics
            .layout(
                &crate::decode::DecodeBudget::default(),
                b"<0000FFFF>",
                10.,
                1.,
                100.,
            )
            .unwrap();
        assert_eq!((layout.left, layout.right, layout.advance), (-1., 1., 0.));
        assert!(
            metrics
                .layout(
                    &crate::decode::DecodeBudget::default(),
                    b"<0041FF>",
                    10.,
                    0.,
                    0.
                )
                .is_none()
        );
        assert!(
            metrics
                .layout(&crate::decode::DecodeBudget::default(), b"<>", 10., 0., 0.)
                .is_none()
        );
    }

    /// 未対応 Encoding と不正な descendant を Identity-H として扱わない。
    #[test]
    fn unsupported_encoding_and_descendant_are_not_used() {
        for dictionary in [
            "<< /Encoding /Identity-V /DescendantFonts [<< /Subtype /CIDFontType2 >>] >>",
            "<< /Encoding /UniJIS-UTF16-H /DescendantFonts [<< /Subtype /CIDFontType2 >>] >>",
            "<< /Encoding /Identity-H /DescendantFonts [] >>",
            "<< /Encoding /Identity-H /DescendantFonts [<< /Subtype /CIDFontType2 >> << /Subtype /CIDFontType2 >>] >>",
            "<< /Encoding /Identity-H /DescendantFonts [<< /Subtype /Type1 >>] >>",
            "<< /Encoding /Identity-H /DescendantFonts [99 0 R] >>",
        ] {
            assert!(
                Metrics::read(
                    &mut ResourceResolver::new(
                        &Objects::new(),
                        &crate::decode::DecodeBudget::default()
                    ),
                    dictionary.as_bytes(),
                    &crate::decode::DecodeBudget::default(),
                )
                .is_none()
            );
        }
    }

    /// null の optional W/DW は省略時と同じ1000 units/emになる。
    #[test]
    fn null_optional_widths_use_defaults() {
        let metrics = read("/W null /DW null").unwrap();
        let layout = metrics
            .layout(
                &crate::decode::DecodeBudget::default(),
                b"<0041>",
                10.,
                2.,
                50.,
            )
            .unwrap();
        assert_eq!((layout.right, layout.advance), (10., 12.));
    }

    /// 重複した最大 CID 範囲の展開は CID 空間より大きな表を確保しない。
    #[test]
    fn repeated_full_ranges_have_bounded_storage() {
        let metrics = read(&format!("/W [{}]", "0 65535 500 ".repeat(512))).unwrap();
        assert_eq!(
            metrics
                .layout(
                    &crate::decode::DecodeBudget::default(),
                    b"<FFFF>",
                    10.,
                    0.,
                    0.
                )
                .unwrap()
                .right,
            5.
        );
        let clone = metrics.clone();
        let widths = metrics.widths.as_ref().unwrap();
        assert_eq!(widths.len(), CID_COUNT);
        assert!(Arc::ptr_eq(widths, clone.widths.as_ref().unwrap()));
    }

    /// W の解析量上限は境界を含み、それを超える繰り返し operands は採用しない。
    #[test]
    fn width_operand_limit_is_inclusive() {
        let entries = "0 0 500 ".repeat(MAX_WIDTH_VALUES / 3);
        assert!(read(&format!("/W [{entries}]")).is_some());
        assert!(read(&format!("/W [{entries} 0 0 500]")).is_none());
    }

    /// 幅表の参照失敗・世代不一致・循環を部分的な成功として採用しない。
    #[test]
    fn stale_and_cyclic_width_references_are_not_used() {
        let objects = Objects::from([(1, (1, b"1 1 R".as_slice())), (2, (1, b"500".as_slice()))]);
        for entries in ["/W 1 1 R", "/W [65 [2 0 R]]", "/DW 1 1 R"] {
            let dictionary = format!(
                "<< /Encoding /Identity-H /DescendantFonts [<< /Subtype /CIDFontType2 {entries} >>] >>"
            );
            assert!(
                Metrics::read(
                    &mut ResourceResolver::new(&objects, &crate::decode::DecodeBudget::default()),
                    dictionary.as_bytes(),
                    &crate::decode::DecodeBudget::default()
                )
                .is_none(),
                "{entries}"
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, ..ProptestConfig::default() })]
        /// 混在する範囲/個別幅の後勝ち適用は、独立した逐次更新モデルと一致する。
        #[test]
        fn mixed_width_groups_match_sequential_model(groups in prop::collection::vec((0u16..32, 0u16..32, -200i16..1500, any::<bool>()), 1..20)) {
            let mut model = [1000i16; 32];
            let mut entries = String::new();
            for (a, b, width, individual) in groups {
                let first = a.min(b) as usize;
                let last = a.max(b) as usize;
                if individual {
                    entries.push_str(&format!(" {first} [{}]", format!("{width} ").repeat(last-first+1)));
                } else {
                    entries.push_str(&format!(" {first} {last} {width}"));
                }
                model[first..=last].fill(width);
            }
            let metrics = read(&format!("/W [{entries}]")).unwrap();
            for (code, width) in model.into_iter().enumerate() {
                let layout = metrics.layout(&crate::decode::DecodeBudget::default(), format!("<{code:04X}>").as_bytes(), 1000., 0., 0.).unwrap();
                prop_assert_eq!(layout.advance, width as f32);
            }
        }
    }
}
