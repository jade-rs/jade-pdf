//! 原文の複数scalarを互換文字へ置換せず、確認済みの合字GIDへ結ぶGSUBを作る。
use crate::decode::DecodeBudget;
#[path = "type1_outline_gsub_writer.rs"]
mod writer;

/// cmapで得るcomponent GID列と、原本の外形を持つ出力GID。
pub(super) struct Rule<'a> {
    /// 論理順のcomponent。先頭を含め二つ以上で、.notdefを使わない。
    pub(super) components: &'a [u16],
    /// 原本の合字外形に対応するGID。
    pub(super) glyph: u16,
}

/// 長い列を優先し、16bit境界を越える内容はtype7の32bit offsetで参照する。
pub(super) fn encode(
    budget: &DecodeBudget,
    rules: &[Rule<'_>],
    glyph_count: u16,
) -> Option<Vec<u8>> {
    if budget.is_stopped() || glyph_count == 0 || glyph_count > 4096 || rules.len() > 256 {
        return None;
    }
    if rules.is_empty() {
        return Some(Vec::new());
    }
    for rule in rules {
        if rule.components.len() < 2
            || rule.components.len() > 65535
            || rule.glyph == 0
            || rule.glyph >= glyph_count
        {
            return None;
        }
        for &component in rule.components {
            if budget.is_stopped() || component == 0 || component >= glyph_count {
                return None;
            }
        }
    }
    let mut order = budget.font_vector(rules.len())?;
    order.extend(rules.iter());
    order.sort_unstable_by(|a, b| {
        a.components[0]
            .cmp(&b.components[0])
            .then_with(|| b.components.len().cmp(&a.components.len()))
            .then_with(|| a.components.cmp(b.components))
            .then_with(|| a.glyph.cmp(&b.glyph))
    });
    if budget.is_stopped()
        || order
            .windows(2)
            .any(|pair| pair[0].components == pair[1].components && pair[0].glyph != pair[1].glyph)
    {
        return None;
    }
    order.dedup_by(|a, b| a.components == b.components && a.glyph == b.glyph);
    let mut size = 52usize.checked_add(order.len().checked_mul(30)?)?;
    for rule in &order {
        size = size.checked_add(rule.components.len().checked_mul(2)?)?;
    }
    if size > crate::parser::MAX_EMBEDDED_FONT_BYTES {
        return None;
    }
    writer::encode(budget, &order, size)
}

#[cfg(test)]
#[path = "type1_outline_gsub_tests.rs"]
mod tests;
