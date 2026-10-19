//! 単入力・8 bit・線形補間のsampled tint関数を有界に評価する。
use super::super::{DecodeBudget, ImageFilter, image_attribute, image_sample_decode::number};

/// 最大四成分のsampled tint関数。
pub(super) struct SampledTint {
    /// 入力のclip区間。
    domain: [f64; 2],
    /// table位置への写像端点。
    encode: [f64; 2],
    /// 出力のclip区間。
    range: [[f64; 2]; 4],
    /// sample値の写像端点。
    decode: [[f64; 2]; 4],
    /// 出力成分数。
    components: usize,
    /// 8 bit sample table。
    samples: Vec<u8>,
}

/// 直接・間接の有限数値配列を固定長の領域へ読む。
fn pairs(
    budget: &DecodeBudget,
    pdf: &[u8],
    dict: &[u8],
    key: &[u8],
    count: usize,
) -> Option<[[f64; 2]; 4]> {
    let value = image_attribute(budget, pdf, dict, key)?;
    let mut entries = crate::metadata::array_entries(&value, count * 2)?;
    let mut result = [[0.0; 2]; 4];
    for pair in result.iter_mut().take(count) {
        *pair = [
            number(budget, pdf, entries.next()??)?,
            number(budget, pdf, entries.next()??)?,
        ];
    }
    entries.next().is_none().then_some(result)
}

impl SampledTint {
    /// 次元・bit深度・table長・補間方式を検証してstreamを読む。
    pub(super) fn read(
        budget: &DecodeBudget,
        pdf: &[u8],
        reference: &[u8],
        components: usize,
    ) -> Option<Self> {
        if !matches!(components, 1 | 3 | 4) {
            return None;
        }
        let object = budget.reference(pdf, reference)?;
        let (dict, data) = super::super::extract_stream_for_object(budget, pdf, object)?;
        for (key, expected) in [(b"FunctionType".as_slice(), 0.0), (b"BitsPerSample", 8.0)] {
            if number(budget, pdf, &image_attribute(budget, pdf, dict, key)?)? != expected {
                return None;
            }
        }
        if crate::metadata::value(dict, b"Order").is_some()
            && number(budget, pdf, &image_attribute(budget, pdf, dict, b"Order")?)? != 1.0
        {
            return None;
        }
        let size = image_attribute(budget, pdf, dict, b"Size")?;
        let mut sizes = crate::metadata::array_entries(&size, 1)?;
        let size = number(budget, pdf, sizes.next()??)?;
        if sizes.next().is_some() || !(1.0..=4096.0).contains(&size) || size.fract() != 0.0 {
            return None;
        }
        let size = size as usize;
        let domain = pairs(budget, pdf, dict, b"Domain", 1)?[0];
        if domain[0] >= domain[1] || !(domain[1] - domain[0]).is_finite() {
            return None;
        }
        let range = pairs(budget, pdf, dict, b"Range", components)?;
        if range[..components]
            .iter()
            .any(|pair| pair[0] > pair[1] || !(pair[1] - pair[0]).is_finite())
        {
            return None;
        }
        let encode = if crate::metadata::value(dict, b"Encode").is_some() {
            pairs(budget, pdf, dict, b"Encode", 1)?[0]
        } else {
            [0.0, (size - 1) as f64]
        };
        let decode = if crate::metadata::value(dict, b"Decode").is_some() {
            pairs(budget, pdf, dict, b"Decode", components)?
        } else {
            range
        };
        if !((encode[1] - encode[0]).is_finite())
            || decode[..components]
                .iter()
                .any(|p| !(p[1] - p[0]).is_finite())
        {
            return None;
        }
        // Function streamにimage用Predictorは適用しない。
        if crate::metadata::value(dict, b"DecodeParms").is_some()
            && !super::super::image_icc::is_null(&image_attribute(
                budget,
                pdf,
                dict,
                b"DecodeParms",
            )?)
        {
            return None;
        }
        let samples = if crate::metadata::value(dict, b"Filter").is_none() {
            budget.copy_stream_bytes(data)?
        } else if ImageFilter::read(dict)? == ImageFilter::Flate {
            super::super::decode_zlib(data, budget)?
        } else {
            return None;
        };
        if samples.len() != size.checked_mul(components)? || budget.is_stopped() {
            return None;
        }
        Some(Self {
            domain,
            encode,
            range,
            decode,
            components,
            samples,
        })
    }

    /// clip→Encode→線形補間→Decode→Rangeの順に、途中で整数化せず評価する。
    pub(super) fn evaluate(&self, input: f64) -> [f64; 4] {
        let ratio = (input.clamp(self.domain[0], self.domain[1]) - self.domain[0])
            / (self.domain[1] - self.domain[0]);
        let last = self.samples.len() / self.components - 1;
        let position =
            (self.encode[0] + ratio * (self.encode[1] - self.encode[0])).clamp(0.0, last as f64);
        let lower = position.floor() as usize;
        let upper = (lower + 1).min(last);
        let weight = position - lower as f64;
        let mut output = [0.0; 4];
        for (channel, value) in output.iter_mut().enumerate().take(self.components) {
            let first = f64::from(self.samples[lower * self.components + channel]);
            let second = f64::from(self.samples[upper * self.components + channel]);
            let sample = (first + weight * (second - first)) / 255.0;
            *value = (self.decode[channel][0]
                + sample * (self.decode[channel][1] - self.decode[channel][0]))
                .clamp(self.range[channel][0], self.range[channel][1]);
        }
        output
    }
}
