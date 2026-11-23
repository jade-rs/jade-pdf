//! 元program・code計画・実出力の属性を含め、予約font名を使わない派生名を決める。
use super::{cff, gsub, opentype, plan::Plan};
use crate::decode::DecodeBudget;

/// 同じ元programでも異なるPDF幅・文字対応を区別する情報。
pub(super) struct Source<'a> {
    /// 展開後の元FontFile。hintや元の権利表示も含む。
    pub(super) program: &'a [u8],
    /// 元code順・未確認状態・幅違いを失っていない計画。
    pub(super) plan: &'a Plan,
}

/// すべての出力入力を長さで区切って識別する。元のfamily名は派生名へ再利用しない。
pub(super) fn derive(
    budget: &DecodeBudget,
    source: &Source<'_>,
    font: &cff::Font<'_>,
    info: &opentype::Info<'_>,
    rules: &[gsub::Rule<'_>],
) -> Option<String> {
    if budget.is_stopped()
        || source.program.len() > crate::parser::MAX_EMBEDDED_FONT_BYTES
        || source.plan.codes.len() != 256
        || source.plan.glyphs.is_empty()
        || source.plan.glyphs.len() > 4096
        || font.glyphs.is_empty()
        || font.glyphs.len() > 4096
        || info.metrics.len() != font.glyphs.len()
        || info.mappings.len() > 65535
        || rules.len() > 256
    {
        return None;
    }
    let mut input = Input {
        budget,
        hash: hmac_sha256::Hash::new(),
    };
    input.bytes(b"jade-pdf Type1 portable font identity\0v1\0")?;
    input.source(source)?;
    input.value(u64::from(font.units_per_em));
    input.bytes(font.notice)?;
    for &value in &font.bbox {
        input.value(value as i64 as u64);
    }
    input.value(font.italic_angle.to_bits());
    input.value(u64::from(font.is_fixed_pitch));
    input.value(font.glyphs.len() as u64);
    for glyph in font.glyphs {
        input.bytes(glyph.name)?;
        input.bytes(glyph.program)?;
    }
    input.output(info)?;
    input.value(rules.len() as u64);
    for rule in rules {
        if rule.components.len() > 65535 || budget.is_stopped() {
            return None;
        }
        input.value(u64::from(rule.glyph));
        input.value(rule.components.len() as u64);
        for &component in rule.components {
            input.value(u64::from(component));
        }
    }
    let digest = input.hash.finalize();
    let mut name = budget.font_vector(59)?;
    name.extend_from_slice(b"JadeT1-");
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut accumulator, mut bits) = (0u32, 0);
    for byte in digest {
        accumulator = (accumulator << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            name.push(alphabet[((accumulator >> bits) & 31) as usize]);
        }
    }
    if bits != 0 {
        name.push(alphabet[((accumulator << (5 - bits)) & 31) as usize]);
    }
    if budget.is_stopped() {
        None
    } else {
        String::from_utf8(name).ok()
    }
}

/// 大きい元programも中止を確認しながら読み、連結境界を長さで区切る。
struct Input<'a> {
    /// 呼出元と同じ中止・累積予約の状態。
    budget: &'a DecodeBudget,
    /// OSやRustのhash seedに依存しないSHA-256状態。
    hash: hmac_sha256::Hash,
}

impl Input<'_> {
    /// 値ごとに八byteを使い、整数の長さやホストendiannessに依存しない。
    fn value(&mut self, value: u64) {
        self.hash.update(value.to_be_bytes());
    }

    /// byte列は長さを先に置き、4KiBごとに中止を確認する。
    fn bytes(&mut self, bytes: &[u8]) -> Option<()> {
        if self.budget.is_stopped() {
            return None;
        }
        self.value(u64::try_from(bytes.len()).ok()?);
        for chunk in bytes.chunks(4096) {
            if self.budget.is_stopped() {
                return None;
            }
            self.hash.update(chunk);
        }
        Some(())
    }

    /// 元の全codeを順序・明示性・空と未知の区別を含めて識別する。
    fn source(&mut self, source: &Source<'_>) -> Option<()> {
        self.bytes(source.program)?;
        self.value(source.plan.glyphs.len() as u64);
        for glyph in &source.plan.glyphs {
            self.value(u64::from(glyph.original));
            self.value(u64::from(glyph.pdf_width.is_some()));
            if let Some(width) = glyph.pdf_width {
                if !width.is_finite() || width < 0. {
                    return None;
                }
                self.value(width.to_bits());
            }
        }
        self.value(source.plan.codes.len() as u64);
        for code in &source.plan.codes {
            if self.budget.is_stopped()
                || !code.mapping.source.width.is_finite()
                || code.mapping.source.width < 0.
                || code
                    .glyph
                    .is_some_and(|g| g == 0 || usize::from(g) >= source.plan.glyphs.len())
            {
                return None;
            }
            self.value(code.mapping.source.glyph.map_or(0, |g| u64::from(g) + 1));
            self.value(code.mapping.source.width.to_bits());
            self.value(u64::from(code.mapping.source.explicit_width));
            self.value(code.glyph.map_or(0, |g| u64::from(g) + 1));
            self.value(u64::from(code.mapping.from_tounicode));
            self.value(u64::from(code.mapping.scalars.is_some()));
            if let Some(scalars) = &code.mapping.scalars {
                if scalars.len() > 65535 {
                    return None;
                }
                self.value(scalars.len() as u64);
                for &scalar in scalars {
                    if self.budget.is_stopped() || char::from_u32(scalar).is_none() {
                        return None;
                    }
                    self.value(u64::from(scalar));
                }
            }
        }
        Some(())
    }

    /// CFF以外の全metric・style・許諾・文字対応を実出力と同じ入力から読む。
    fn output(&mut self, info: &opentype::Info<'_>) -> Option<()> {
        self.value(info.metrics.len() as u64);
        for metric in info.metrics {
            if self.budget.is_stopped() {
                return None;
            }
            self.value(u64::from(metric.advance));
            self.value(metric.left as i64 as u64);
            self.value(u64::from(metric.bounds.is_some()));
            if let Some(bounds) = metric.bounds {
                for value in bounds {
                    self.value(value as i64 as u64);
                }
            }
        }
        self.value(info.mappings.len() as u64);
        for mapping in info.mappings {
            if self.budget.is_stopped() {
                return None;
            }
            self.value(u64::from(mapping.scalar));
            self.value(u64::from(mapping.glyph));
        }
        for value in [
            info.ascent,
            info.descent,
            info.line_gap,
            info.underline_position,
            info.underline_thickness,
            info.x_height,
            info.cap_height,
        ] {
            self.value(value as i64 as u64);
        }
        self.value(u64::from(info.weight));
        self.value(u64::from(info.bold));
        self.value(u64::from(info.italic));
        self.value(u64::from(info.embedding_flags));
        self.bytes(info.license.as_bytes())?;
        self.bytes(info.license_url.as_bytes())
    }
}

#[cfg(test)]
#[path = "type1_outline_identity_tests.rs"]
mod tests;
