//! 文字結果の属性コピー・結合領域を確保前のmetadata累積容量へ予約する。
use crate::decode::DecodeBudget;

#[cfg(test)]
#[path = "text_aggregate_budget_tests.rs"]
mod aggregate_budget_tests;

/// 集約配列の新容量全体を予約し、文字列を複製せず結果を移動する。
pub(crate) fn append(
    budget: &DecodeBudget,
    values: &mut Vec<crate::ParsedTextElement>,
    mut addition: Vec<crate::ParsedTextElement>,
) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    if values.capacity() == 0 {
        *values = addition;
        return Some(());
    }
    let needed = values.len().checked_add(addition.len())?;
    if needed > values.capacity() {
        let capacity = needed.max(values.capacity().saturating_mul(2));
        let mut replacement = budget.metadata_vector(capacity)?;
        replacement.append(values);
        *values = replacement;
    }
    values.append(&mut addition);
    Some(())
}

/// 保存が必要な属性だけを、要求byte全体の予約後に複製する。
pub(crate) fn copy(budget: &DecodeBudget, text: Option<&str>) -> Option<Option<String>> {
    let Some(text) = text else {
        return (!budget.is_stopped()).then_some(None);
    };
    let mut output = budget.metadata_vector(text.len())?;
    output.extend_from_slice(text.as_bytes());
    Some(Some(String::from_utf8(output).ok()?))
}

/// 足りない場合は倍増後の全領域を予約し、旧文字列を移動して結合する。
pub(crate) fn extend(budget: &DecodeBudget, text: &mut String, suffix: &str) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    let needed = text.len().checked_add(suffix.len())?;
    if needed > text.capacity() {
        let capacity = needed.max(text.capacity().saturating_mul(2));
        let mut output = budget.metadata_vector(capacity)?;
        output.append(&mut std::mem::take(text).into_bytes());
        *text = String::from_utf8(output).ok()?;
    }
    text.push_str(suffix);
    Some(())
}
