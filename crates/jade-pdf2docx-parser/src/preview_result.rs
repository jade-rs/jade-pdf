//! 文書previewの重複除去と完成UTF-8を確保前に共有容量へ予約する。
use crate::{decode::DecodeBudget, metadata_map::Map};

/// 候補を逐次受け取り、完成結果だけを保存する。
#[derive(Default)]
pub(super) struct Builder {
    /// 正規化済みの重複索引。
    seen: Map<String, ()>,
    /// 予約済みUTF-8出力。
    preview: Vec<u8>,
    /// 改行を含む出力文字数。
    chars: usize,
}

impl Builder {
    /// 可読候補を追加し、文字数上限に達するとfalseを返す。
    pub(super) fn push(&mut self, budget: &DecodeBudget, literal: impl AsRef<str>) -> Option<bool> {
        let literal = literal.as_ref();
        if budget.is_stopped() {
            return None;
        }
        if !super::is_readable_text_candidate(literal) {
            return Some(true);
        }
        self.push_decoded(budget, literal)
    }

    /// 復号済み本文は短文も保存し、制御文字・空白のみの結果を除く。
    pub(super) fn push_decoded(&mut self, budget: &DecodeBudget, text: &str) -> Option<bool> {
        if budget.is_stopped() {
            return None;
        }
        let normalized = text.trim();
        if normalized.is_empty()
            || normalized.chars().any(|c| {
                c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\u{0008}' | '\u{000C}')
            })
        {
            return Some(true);
        }
        if self.seen.get(normalized).is_some() {
            return Some(true);
        }
        self.seen.prepare(budget, normalized)?;
        let key = crate::metadata_map::copy(budget, normalized)?;
        let separator = usize::from(!self.preview.is_empty());
        let remaining = 1024 - self.chars - separator;
        let end = normalized
            .char_indices()
            .nth(remaining)
            .map_or(normalized.len(), |(offset, _)| offset);
        let shown = &normalized[..end];
        let needed = self
            .preview
            .len()
            .saturating_add(separator)
            .saturating_add(shown.len());
        if needed > self.preview.capacity() {
            let capacity = needed.max(self.preview.capacity().saturating_mul(2));
            let mut replacement = budget.metadata_vector(capacity)?;
            replacement.append(&mut self.preview);
            self.preview = replacement;
        }
        if separator != 0 {
            self.preview.push(b'\n');
        }
        self.preview.extend_from_slice(shown.as_bytes());
        self.chars += separator + shown.chars().count();
        self.seen.insert(budget, key, ())?;
        if self.chars == 1024 {
            return Some(false);
        }
        Some(true)
    }

    /// 停止済みの部分結果を破棄し、完成UTF-8を移動して返す。
    pub(super) fn finish(self, budget: &DecodeBudget) -> Option<String> {
        if budget.is_stopped() {
            return None;
        }
        String::from_utf8(self.preview).ok()
    }
}

/// 候補iteratorを使って容量・停止動作を検証する。
#[cfg(test)]
pub(super) fn collect(
    budget: &DecodeBudget,
    literals: impl IntoIterator<Item = String>,
) -> Option<String> {
    if budget.is_stopped() {
        return None;
    }
    let mut builder = Builder::default();
    for literal in literals {
        if !builder.push(budget, literal)? {
            break;
        }
    }
    builder.finish(budget)
}
