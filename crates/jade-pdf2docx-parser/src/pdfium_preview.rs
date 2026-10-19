//! Pdfium previewの連結と重複索引をmetadata累積予算へ予約する。
use crate::{decode::DecodeBudget, metadata_map::Map};

/// 改行を含む新しい全容量を予約し、失敗時は既存本文を変更しない。
pub(super) fn append(budget: &DecodeBudget, target: &mut String, page_text: &str) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    let trimmed = page_text.trim();
    if trimmed.is_empty() {
        return Some(());
    }
    let separator = usize::from(!target.is_empty());
    let needed = target
        .len()
        .saturating_add(separator)
        .saturating_add(trimmed.len());
    if needed > target.capacity() {
        let capacity = needed.max(target.capacity().saturating_mul(2));
        let mut replacement = budget.metadata_vector(capacity)?;
        replacement.append(&mut std::mem::take(target).into_bytes());
        *target = String::from_utf8(replacement).ok()?;
    }
    if separator != 0 {
        target.push('\n');
    }
    target.push_str(trimmed);
    Some(())
}

/// 行キーは元本文を借用し、初出順のLF出力だけを予算付きで保存する。
pub(super) fn normalize(budget: &DecodeBudget, text: &str) -> Option<String> {
    if budget.is_stopped() {
        return None;
    }
    let mut seen = Map::<&str, ()>::default();
    let mut output = String::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if seen.get(line).is_some() {
            continue;
        }
        seen.insert(budget, line, ())?;
        append(budget, &mut output, line)?;
    }
    Some(output)
}
