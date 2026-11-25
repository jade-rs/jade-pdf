//! SDKが報告した語間を、同順のUnicodeが確認できたspan内だけへ保存する。
use super::Page;
use crate::{ParsedTextElement, decode::DecodeBudget, metadata_map::Map};

/// 連続するSDK本文をページごとに読み、通常空白の前後の文字と位置を保持する。
#[derive(Default)]
pub(super) struct State {
    /// 既に読んだ非空白scalar数。
    index: usize,
    /// 直前のbase文字。改行や制御空白をまたいで語間を作らない。
    previous: Option<char>,
    /// base文字の後に通常空白が一つ以上あったか。
    pending: bool,
}

impl State {
    /// 次のscalarを読み、通常空白に続く語間文字の非空白位置だけを返す。
    fn observe(&mut self, scalar: char) -> Option<usize> {
        if scalar.is_whitespace() {
            if scalar == ' ' {
                self.pending = self
                    .previous
                    .is_some_and(crate::text_word_gap::word_spaced_character);
            } else {
                self.previous = None;
                self.pending = false;
            }
            return None;
        }
        let index = self.index;
        self.index = self.index.saturating_add(1);
        if super::variation_selector(scalar) {
            return None;
        }
        let boundary = self.pending && crate::text_word_gap::word_spaced_character(scalar);
        self.previous = Some(scalar);
        self.pending = false;
        boundary.then_some(index)
    }
}

/// 件数を先に数え、Unicodeが完全一致したページの語間位置だけを予約して保存する。
pub(super) fn gather(
    budget: &DecodeBudget,
    native: &[ParsedTextElement],
    index: &Map<usize, usize>,
    pages: &mut [Page],
) -> Option<()> {
    for element in native {
        let page = &mut pages[*index.get(&element.page)?];
        if page.compatible && page.unicode_agrees {
            for scalar in element.text.chars() {
                if budget.is_stopped() {
                    return None;
                }
                if page.space_state.observe(scalar).is_some() {
                    page.space_count = page.space_count.checked_add(1)?;
                }
            }
        }
    }
    for page in pages.iter_mut() {
        if page.space_count > 0 {
            page.spaces = budget.metadata_vector(page.space_count)?;
        }
        page.space_state = State::default();
    }
    for element in native {
        let page = &mut pages[*index.get(&element.page)?];
        if page.compatible && page.unicode_agrees {
            for scalar in element.text.chars() {
                if budget.is_stopped() {
                    return None;
                }
                if let Some(position) = page.space_state.observe(scalar) {
                    page.spaces.push(position);
                }
            }
        }
    }
    Some(())
}
