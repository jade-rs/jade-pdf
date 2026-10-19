//! 原文を保持し、明示辞書と取り違え規則からOCR補正候補を生成する。
//!
//! [`Corrector::analyze`] は候補だけを返す。本文を変更するには、呼び出し側が
//! 候補を確認して [`CorrectionReport::apply_selected`] へ番号を渡す必要がある。
//! 信頼度はOCRエンジンの値であり、補正の正解確率としては扱わない。
//! 辞書・規則は対象文書に合わせて明示し、Unicode正規化や挿入・削除は行わない。

#![doc = include_str!("../README.md")]

mod engine;
mod report;
mod types;

pub use engine::Corrector;
pub use report::{CorrectionCandidate, CorrectionReport};
pub use types::{CorrectionError, CorrectionLimits, CorrectionOptions};

#[cfg(test)]
mod tests;
