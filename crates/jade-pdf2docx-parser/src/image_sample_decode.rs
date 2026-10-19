//! source Decodeの有限なPDF数値を解決し、色成分とpalette添字へ写像する。
use super::{DecodeBudget, ObjectBytes, image_attribute, object_body_for_number};
use crate::syntax::{Lexer, Token};

/// 8 bit source sampleの成分別写像と、Indexedの有効添字上限。
pub(super) struct SampleDecode {
    /// Device色では正規化値、Indexedでは添字へ写像する端点。
    pairs: [[f64; 2]; 4],
    /// sourceの成分数。
    components: usize,
    /// Indexedの場合のpalette最大添字。
    high: Option<u8>,
}

/// 単一PDF keywordを、前後の空白・コメントを除いて検査する。
fn is_keyword(value: &[u8], expected: &[u8]) -> bool {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    matches!(lexer.next(), Some(Token::Word(word)) if word == expected) && lexer.next().is_none()
}

/// 直接値または一段の間接scalarを、有限なPDF整数・実数として読む。
pub(super) fn number(budget: &DecodeBudget, pdf: &[u8], value: &[u8]) -> Option<f64> {
    let value = if let Some(object) = budget.reference(pdf, value) {
        object_body_for_number(budget, pdf, object)?
    } else {
        ObjectBytes::Borrowed(value)
    };
    let mut lexer = Lexer {
        bytes: &value,
        cursor: 0,
    };
    let Token::Word(word) = lexer.next()? else {
        return None;
    };
    if lexer.next().is_some() {
        return None;
    }
    let unsigned = word
        .strip_prefix(b"+")
        .or_else(|| word.strip_prefix(b"-"))
        .unwrap_or(word);
    let mut digits = 0;
    let mut dots = 0;
    for &byte in unsigned {
        if byte.is_ascii_digit() {
            digits += 1;
        } else if byte == b'.' {
            dots += 1;
        } else {
            return None;
        }
    }
    if digits == 0 || dots > 1 {
        return None;
    }
    let number = std::str::from_utf8(word).ok()?.parse::<f64>().ok()?;
    number.is_finite().then_some(number)
}

impl SampleDecode {
    /// 検証済みCMYK ICC sourceの四成分Decodeを読む。
    pub(super) fn read_cmyk(budget: &DecodeBudget, pdf: &[u8], dictionary: &[u8]) -> Option<Self> {
        Self::read_pairs(budget, pdf, dictionary, 4, None)
    }

    /// 検証済みICC Gray sourceの一成分Decodeを読む。
    pub(super) fn read_gray(budget: &DecodeBudget, pdf: &[u8], dictionary: &[u8]) -> Option<Self> {
        Self::read_pairs(budget, pdf, dictionary, 1, None)
    }

    /// 検証済みSeparation sourceの一成分Decodeを読む。
    pub(super) fn read_separation(
        budget: &DecodeBudget,
        pdf: &[u8],
        dictionary: &[u8],
    ) -> Option<Self> {
        Self::read_pairs(budget, pdf, dictionary, 1, None)
    }

    /// 検証済みICC RGB sourceの成分別Decodeを読む。
    pub(super) fn read_rgb(budget: &DecodeBudget, pdf: &[u8], dictionary: &[u8]) -> Option<Self> {
        Self::read_pairs(budget, pdf, dictionary, 3, None)
    }

    /// stencilは既定/identityか反転の二通りだけを認め、sampleを塗る側を返す。
    pub(super) fn stencil_inverse(
        budget: &DecodeBudget,
        pdf: &[u8],
        dictionary: &[u8],
    ) -> Option<bool> {
        if crate::metadata::value(dictionary, b"BitsPerComponent").is_some()
            && number(
                budget,
                pdf,
                &image_attribute(budget, pdf, dictionary, b"BitsPerComponent")?,
            )? != 1.0
        {
            return None;
        }
        let mapping = Self::read_pairs(budget, pdf, dictionary, 1, None)?;
        match mapping.pairs[0] {
            [0.0, 1.0] => Some(false),
            [1.0, 0.0] => Some(true),
            _ => None,
        }
    }

    /// 色を持つ1 bit CCITT画像のGray写像だけを読み、stencilや別色空間を拒否する。
    pub(super) fn read_ccitt(budget: &DecodeBudget, pdf: &[u8], dictionary: &[u8]) -> Option<Self> {
        let color = image_attribute(budget, pdf, dictionary, b"ColorSpace")?;
        if !crate::metadata::is_name(&color, b"DeviceGray")
            || number(
                budget,
                pdf,
                &image_attribute(budget, pdf, dictionary, b"BitsPerComponent")?,
            )? != 1.0
        {
            return None;
        }
        if crate::metadata::value(dictionary, b"ImageMask").is_some() {
            let value = image_attribute(budget, pdf, dictionary, b"ImageMask")?;
            if !is_keyword(&value, b"false") && !is_keyword(&value, b"null") {
                return None;
            }
        }
        Self::read(budget, pdf, dictionary)
    }

    /// codecが復元した成分数・bit深度をPDF属性と照合し、Device色だけを写像する。
    pub(super) fn read_codec(
        budget: &DecodeBudget,
        pdf: &[u8],
        dictionary: &[u8],
        components: usize,
        bits: u8,
    ) -> Option<Self> {
        if number(
            budget,
            pdf,
            &image_attribute(budget, pdf, dictionary, b"BitsPerComponent")?,
        )? != f64::from(bits)
        {
            return None;
        }
        let mapping = Self::read(budget, pdf, dictionary)?;
        (mapping.high.is_none() && mapping.components == components).then_some(mapping)
    }

    /// DeviceRGB/GrayまたはIndexed DeviceRGBの既定・明示Decodeを解決する。
    pub(super) fn read(budget: &DecodeBudget, pdf: &[u8], dictionary: &[u8]) -> Option<Self> {
        if budget.is_stopped() {
            return None;
        }
        let color = image_attribute(budget, pdf, dictionary, b"ColorSpace")?;
        let (components, high) = if crate::metadata::is_name(&color, b"DeviceRGB") {
            (3, None)
        } else if crate::metadata::is_name(&color, b"DeviceGray") {
            (1, None)
        } else {
            let mut entries = crate::metadata::array_entries(&color, 4)?;
            if !crate::metadata::is_name(entries.next()??, b"Indexed") {
                return None;
            }
            let base = entries.next()??;
            let base = if let Some(object) = budget.reference(pdf, base) {
                object_body_for_number(budget, pdf, object)?
            } else {
                ObjectBytes::Borrowed(base)
            };
            if !crate::metadata::is_name(&base, b"DeviceRGB") {
                return None;
            }
            let high = number(budget, pdf, entries.next()??)?;
            if !(0.0..=255.0).contains(&high) || high.fract() != 0.0 {
                return None;
            }
            entries.next()??;
            if entries.next().is_some() {
                return None;
            }
            (1, Some(high as u8))
        };
        Self::read_pairs(budget, pdf, dictionary, components, high)
    }

    /// 確定したsource成分数に対し、既定値または有限なDecode端点を読む。
    fn read_pairs(
        budget: &DecodeBudget,
        pdf: &[u8],
        dictionary: &[u8],
        components: usize,
        high: Option<u8>,
    ) -> Option<Self> {
        let mut result = Self {
            pairs: [[0.0, if high.is_some() { 255.0 } else { 1.0 }]; 4],
            components,
            high,
        };
        if crate::metadata::value(dictionary, b"Decode").is_none() {
            return Some(result);
        }
        let value = image_attribute(budget, pdf, dictionary, b"Decode")?;
        if is_keyword(&value, b"null") {
            return Some(result);
        }
        let mut entries = crate::metadata::array_entries(&value, components * 2)?;
        for pair in result.pairs.iter_mut().take(components) {
            for endpoint in pair {
                *endpoint = number(budget, pdf, entries.next()??)?;
            }
        }
        if entries.next().is_some() || budget.is_stopped() {
            return None;
        }
        Some(result)
    }

    /// palette展開の前に添字へ写像する画像かを返す。
    pub(super) fn is_indexed(&self) -> bool {
        self.high.is_some()
    }

    /// 既存RGB codecで安全に保持できるDevice色のidentityかを返す。
    fn is_device_identity(&self) -> bool {
        self.high.is_none()
            && self.pairs[..self.components]
                .iter()
                .all(|pair| *pair == [0.0, 1.0])
    }

    /// 差のoverflowを避け、sampleの割合で有限な端点を加重平均する。
    fn mapped(&self, sample: u8, component: usize) -> f64 {
        let [low, high] = self.pairs[component];
        let t = f64::from(sample) / 255.0;
        low * (1.0 - t) + high * t
    }

    /// Device色をMatte前の255倍値へ写像し、途中では整数へ丸めない。
    pub(super) fn rgb_component(&self, sample: u8, component: usize) -> f64 {
        let component = if self.components == 1 { 0 } else { component };
        if self.pairs[component] == [0.0, 1.0] {
            return f64::from(sample);
        }
        self.mapped(sample, component).clamp(0.0, 1.0) * 255.0
    }

    /// Predictor復元後のsource配列内で写像し、追加の画素配列を確保しない。
    pub(super) fn apply(&self, budget: &DecodeBudget, samples: &mut [u8]) -> Option<()> {
        if !samples.chunks_exact(self.components).remainder().is_empty() || budget.is_stopped() {
            return None;
        }
        for (index, sample) in samples.iter_mut().enumerate() {
            if index % 4096 == 0 && budget.is_stopped() {
                return None;
            }
            *sample = if let Some(high) = self.high {
                self.mapped(*sample, 0).clamp(0.0, f64::from(high)).round() as u8
            } else {
                self.rgb_component(*sample, index % self.components).round() as u8
            };
        }
        (!budget.is_stopped()).then_some(())
    }
}

/// source成分へ未接続のcodecでは非identity Decodeを品質拒否へ渡す。
pub(super) fn require_codec_identity(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    if crate::metadata::value(dictionary, b"Decode").is_none() {
        return Some(());
    }
    let value = image_attribute(budget, pdf, dictionary, b"Decode")?;
    if is_keyword(&value, b"null") {
        return Some(());
    }
    // stencilのDecodeは色空間でなくmaskの1成分に対して定義される。
    let is_stencil = crate::metadata::value(dictionary, b"ImageMask").is_some()
        && is_keyword(
            &image_attribute(budget, pdf, dictionary, b"ImageMask")?,
            b"true",
        );
    let decode = if is_stencil {
        if crate::metadata::value(dictionary, b"BitsPerComponent").is_some()
            && number(
                budget,
                pdf,
                &image_attribute(budget, pdf, dictionary, b"BitsPerComponent")?,
            )? != 1.0
        {
            return None;
        }
        SampleDecode::read_pairs(budget, pdf, dictionary, 1, None)?
    } else {
        SampleDecode::read(budget, pdf, dictionary)?
    };
    decode.is_device_identity().then_some(())
}
