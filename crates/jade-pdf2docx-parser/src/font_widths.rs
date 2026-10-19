//! encoded glyph 幅を読み、simple Font と水平 CIDFont の metrics を区別する。
use crate::{
    metadata,
    page_attributes::{self, Objects},
    resource_resolver::ResourceResolver,
    text,
};

#[cfg(test)]
#[path = "font_name_allocation_tests.rs"]
mod name_allocation_tests;

#[cfg(test)]
#[path = "font_owned_allocation_tests.rs"]
mod owned_allocation_tests;

/// 現在対応する標準 Font の幅表。
#[derive(Debug, Clone, Copy)]
enum Standard {
    /// Courier 系の固定600 units/em。
    Courier,
    /// Helvetica/Oblique の ASCII AFM 幅。
    Helvetica {
        /// StandardEncoding の左右引用符を使うか。
        standard_encoding: bool,
    },
}

/// 局所 Font resource の glyph 幅。未対応 Font は推定へ戻す。
#[derive(Debug, Clone, Default)]
pub(crate) struct Metrics {
    /// FirstChar/Widths と MissingWidth から作る最大256 glyph の幅。
    widths: Option<Box<[f32; 256]>>,
    /// Widths がないときの標準 Font。
    standard: Option<Standard>,
    /// Widths の有無によらず一 byte の character code を持つか。
    simple: bool,
    /// 実効バージョンによる描画モード判定に使うType3識別。
    type3: bool,
    /// 対応済み水平 composite Font の CID 幅。
    cid: Option<crate::cid_widths::Metrics>,
}

/// glyph 矩形全体の範囲と、末尾の間隔を含む局所 advance。
pub(crate) struct Layout {
    /// 最も左にある glyph の端。
    pub(crate) left: f32,
    /// 最も右にある glyph の端。
    pub(crate) right: f32,
    /// 次の glyph 原点までの移動量。
    pub(crate) advance: f32,
}

impl Layout {
    /// glyph ごとの幅と word 判定から、負の間隔も含めた外接範囲を求める。
    pub(crate) fn read(
        glyphs: impl Iterator<Item = Option<(f32, bool)>>,
        character: f32,
        word: f32,
    ) -> Option<Self> {
        let mut layout = Self {
            left: f32::INFINITY,
            right: f32::NEG_INFINITY,
            advance: 0.,
        };
        for glyph in glyphs {
            let (width, space) = glyph?;
            let end = layout.advance + width;
            layout.left = layout.left.min(layout.advance.min(end));
            layout.right = layout.right.max(layout.advance.max(end));
            layout.advance = end + character + if space { word } else { 0. };
        }
        [layout.left, layout.right, layout.advance]
            .iter()
            .all(|n| n.is_finite())
            .then_some(layout)
    }

    /// 幅表を持たない Font は従来の Unicode 幅推定を均等配分する。
    pub(crate) fn estimated(text: &str, width: f32, character: f32, word: f32) -> Self {
        let count = text.chars().count();
        Self::read(
            text.chars()
                .map(|ch| Some((width / count as f32, ch == ' '))),
            character,
            word,
        )
        .unwrap_or(Self {
            left: 0.,
            right: width,
            advance: width,
        })
    }
}

impl Metrics {
    /// 復号済みbyte列から一つのencoded glyphと幅・word spacing判定を読む。
    pub(crate) fn next_glyph<'a>(
        &self,
        remaining: &mut &'a [u8],
        size: f32,
    ) -> Option<(&'a [u8], f32, bool)> {
        if let Some(cid) = &self.cid {
            return cid.next_glyph(remaining, size);
        }
        let code = *remaining.first()?;
        let width = match &self.widths {
            Some(widths) => widths[usize::from(code)],
            None => match self.standard? {
                Standard::Courier => 600.,
                Standard::Helvetica { standard_encoding }
                    if standard_encoding && matches!(code, 39 | 96) =>
                {
                    222.
                }
                Standard::Helvetica { .. } => f32::from(helvetica_glyph_width(char::from(code))?),
            },
        };
        let bytes = &remaining[..1];
        *remaining = &remaining[1..];
        Some((bytes, width * size / 1000., code == 32))
    }

    /// CharProcによるglyph描画を使うFontかを返す。
    pub(crate) fn is_type3(&self) -> bool {
        self.type3
    }

    /// 検査済み Font 辞書と有効 object index から幅を解決する。
    pub(crate) fn read<'a>(
        objects: &Objects<'a>,
        dictionary: &'a [u8],
        budget: &crate::decode::DecodeBudget,
        bytes: &[u8],
    ) -> Self {
        let mut resolver = ResourceResolver::new(objects, budget);
        let mut metrics = Self::default();
        if budget.is_stopped() {
            return metrics;
        }
        let subtype = metadata::value(dictionary, b"Subtype")
            .and_then(|value| resolver.resolve(value))
            .map(|(value, _)| value);
        metrics.type3 = subtype.is_some_and(|v| metadata::is_name(v, b"Type3"));
        if subtype.is_some_and(|v| metadata::is_name(v, b"Type0")) {
            metrics.cid = crate::cid_widths::Metrics::read_with_source(
                &mut resolver,
                dictionary,
                budget,
                bytes,
            );
            return metrics;
        }
        if !subtype.is_some_and(|v| {
            [b"Type1".as_slice(), b"MMType1", b"TrueType"]
                .iter()
                .any(|s| metadata::is_name(v, s))
        }) {
            return metrics;
        }
        metrics.simple = true;
        // Differences の glyph 名解決は未対応なので標準の code 表を流用しない。
        let encoding = metadata::value(dictionary, b"Encoding")
            .and_then(|v| resolver.resolve(v))
            .map(|(encoding, _)| encoding);
        let has_differences =
            encoding.is_some_and(|encoding| metadata::value(encoding, b"Differences").is_some());
        let base_encoding = encoding.and_then(|value| {
            if value.starts_with(b"<<") {
                metadata::value(value, b"BaseEncoding")
            } else {
                Some(value)
            }
        });
        let standard_encoding = match base_encoding {
            None => Some(true),
            Some(value) if metadata::is_name(value, b"StandardEncoding") => Some(true),
            Some(value)
                if metadata::is_name(value, b"WinAnsiEncoding")
                    || metadata::is_name(value, b"MacRomanEncoding") =>
            {
                Some(false)
            }
            _ => None,
        };
        if subtype.is_some_and(|v| metadata::is_name(v, b"Type1")) && !has_differences {
            metrics.standard = crate::metadata_names::name(budget, dictionary, b"BaseFont")
                .and_then(|name| match name.as_str() {
                    "Courier" | "Courier-Bold" | "Courier-Oblique" | "Courier-BoldOblique" => {
                        Some(Standard::Courier)
                    }
                    "Helvetica" | "Helvetica-Oblique" => standard_encoding
                        .map(|standard_encoding| Standard::Helvetica { standard_encoding }),
                    _ => None,
                });
        }
        let Some(first) = number(&mut resolver, dictionary, b"FirstChar") else {
            return metrics;
        };
        if first.fract() != 0. || !(0. ..=255.).contains(&first) {
            return metrics;
        }
        let Some((array, _)) =
            metadata::value(dictionary, b"Widths").and_then(|v| resolver.resolve(v))
        else {
            return metrics;
        };
        let Some(mut values) = metadata::ArrayValues::new(array, 256 - first as usize) else {
            return metrics;
        };
        let missing = metadata::value(dictionary, b"FontDescriptor")
            .and_then(|v| resolver.resolve(v))
            .and_then(|(descriptor, _)| number(&mut resolver, descriptor, b"MissingWidth"))
            .unwrap_or(0.);
        let mut widths = [missing; 256];
        let mut index = first as usize;
        loop {
            let value = match values.next_value() {
                Some(Some(value)) => value,
                Some(None) => break,
                None => return metrics,
            };
            let Some(width) = resolver
                .resolve(value)
                .and_then(|(v, _)| page_attributes::number_value(v))
            else {
                return metrics;
            };
            widths[index] = width;
            index += 1;
        }
        if budget
            .charge_font_metrics_bytes(std::mem::size_of_val(&widths))
            .is_none()
        {
            return metrics;
        }
        metrics.widths = budget.budgeted_box(widths);
        metrics
    }

    /// 未対応幅の simple Font も spacing だけは一 byte code に従って適用する。
    pub(crate) fn estimated_layout(
        &self,
        budget: &crate::decode::DecodeBudget,
        raw: &[u8],
        width: f32,
        character: f32,
        word: f32,
    ) -> Option<Layout> {
        if !self.simple {
            return None;
        }
        let codes = text::pdf_string_bytes(budget, raw)?;
        let count = codes.len();
        Layout::read(
            codes
                .into_iter()
                .map(|code| Some((width / count as f32, code == 32))),
            character,
            word,
        )
    }

    /// Unicode への復号前の code 列へ spacing を適用し、glyph の範囲と advance を返す。
    pub(crate) fn layout(
        &self,
        budget: &crate::decode::DecodeBudget,
        raw: &[u8],
        size: f32,
        character: f32,
        word: f32,
    ) -> Option<Layout> {
        if let Some(cid) = &self.cid {
            return cid.layout(budget, raw, size, character, word);
        }
        if self.widths.is_none() && self.standard.is_none() {
            return None;
        }
        let codes = text::pdf_string_bytes(budget, raw)?;
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

/// 間接参照を解決し、有限の辞書数値を読む。
fn number<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    dictionary: &'a [u8],
    key: &[u8],
) -> Option<f32> {
    let (value, _) = resolver.resolve(metadata::value(dictionary, key)?)?;
    page_attributes::number_value(value)
}

/// Helvetica AFMに定義されたASCII glyph幅（1000 units/em）を返す。
pub(crate) fn helvetica_glyph_width(ch: char) -> Option<u16> {
    Some(match ch {
        ' ' | '!' | ',' | '.' | '/' | ':' | ';' | 'I' | '[' | '\\' | ']' | 'f' | 't' => 278,
        '"' => 355,
        '#'
        | '$'
        | '0'..='9'
        | '?'
        | 'L'
        | '_'
        | 'a'
        | 'b'
        | 'd'
        | 'e'
        | 'g'
        | 'h'
        | 'n'
        | 'o'
        | 'p'
        | 'q'
        | 'u' => 556,
        '%' => 889,
        '&' | 'A' | 'B' | 'E' | 'K' | 'P' | 'S' | 'V' | 'X' | 'Y' => 667,
        '\'' => 191,
        '(' | ')' | '-' | '`' | 'r' => 333,
        '*' => 389,
        '+' | '<' | '=' | '>' | '~' => 584,
        '@' => 1015,
        'C' | 'D' | 'H' | 'N' | 'R' | 'U' | 'w' => 722,
        'F' | 'T' | 'Z' => 611,
        'G' | 'O' | 'Q' => 778,
        'J' | 'c' | 'k' | 's' | 'v' | 'x' | 'y' | 'z' => 500,
        'M' | 'm' => 833,
        'W' => 944,
        '^' => 469,
        'i' | 'j' | 'l' => 222,
        '{' | '}' => 334,
        '|' => 260,
        _ => return None,
    })
}
