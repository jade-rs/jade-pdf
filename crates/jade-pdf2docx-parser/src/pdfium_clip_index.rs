//! 公開object幾何がページ内で一意な場合だけ、共有文字indexから所属を検索する。
use crate::{ParseError, decode::DecodeBudget, metadata_workspace};
use pdfium_render::prelude::*;

/// SDKの公開矩形・変換行列だけから作る有限の幾何key。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    /// private handleを読み取らず、10個の値をbit列で保存する。
    values: [u32; 10],
}

/// 所属keyと、共有配列に対応する元ページの文字index。
pub(super) struct Entry {
    /// 所属候補の公開幾何。
    key: Key,
    /// 文字列や位置から作り直さない元のSDK index。
    pub(super) index: usize,
}

/// 全text objectの一意性と、文字から得た所属を一度だけ保存する。
pub(super) struct Index<'a> {
    /// 生成空白を含めて保持する共有ページ文字配列。
    characters: PdfPageTextChars<'a>,
    /// 重複も残す全objectのkey。重複は最適化の対象にしない。
    objects: Vec<Key>,
    /// key・元index順に並べた文字の所属。
    entries: Vec<Entry>,
    /// object列や幾何が不明な場合は既存のSDK所属検索へ戻す。
    complete: bool,
}

impl<'a> Index<'a> {
    /// 一度だけ全objectと全文字を調べ、確保を既存のmetadata予算へ予約する。
    pub(super) fn new<'o>(
        budget: &DecodeBudget,
        objects: &'o impl PdfPageObjectsCommon<'o>,
        page_text: &'a PdfPageText<'_>,
    ) -> Result<Self, ParseError> {
        let characters = crate::pdfium_text::characters(budget, page_text)?;
        let mut keys = Vec::new();
        let mut complete = true;
        collect_keys(budget, objects, 0, &mut keys, &mut complete)?;
        keys.sort_unstable();
        let mut entries = metadata_workspace::vector(budget, characters.len())?;
        for character in characters.iter() {
            budget.check()?;
            let Ok(object) = character.text_object() else {
                // SDKで所属objectのない生成空白等は、従来の所属検索でも一致しない。
                continue;
            };
            if let Some(key) = key(&object) {
                entries.push(Entry {
                    key,
                    index: character.index(),
                });
            } else {
                complete = false;
            }
        }
        entries.sort_unstable_by_key(|entry| (entry.key, entry.index));
        Ok(Self {
            characters,
            objects: keys,
            entries,
            complete,
        })
    }

    /// 全object列で幾何が一意な場合だけ、同じ所属の元indexを返す。
    pub(super) fn entries(&self, object: &PdfPageTextObject<'_>) -> Option<&[Entry]> {
        if !self.complete {
            return None;
        }
        let key = key(object)?;
        let first = self.objects.partition_point(|item| *item < key);
        let end = self.objects.partition_point(|item| *item <= key);
        if end - first != 1 {
            return None;
        }
        let first = self.entries.partition_point(|item| item.key < key);
        let end = self.entries.partition_point(|item| item.key <= key);
        Some(&self.entries[first..end])
    }

    /// 新しい全index配列を作らず、保存済みのページ文字を借りる。
    pub(super) fn character(&self, index: usize) -> Result<PdfPageTextChar<'_>, PdfiumError> {
        self.characters.get(index)
    }
}

/// 無効な値をkeyにせず、符号付きゼロだけを同じ幾何として扱う。
fn key<'a>(object: &impl PdfPageObjectCommon<'a>) -> Option<Key> {
    let rect = object.bounds().ok()?.to_rect();
    let matrix = object.matrix().ok()?;
    let values = [
        rect.left().value,
        rect.bottom().value,
        rect.right().value,
        rect.top().value,
        matrix.a(),
        matrix.b(),
        matrix.c(),
        matrix.d(),
        matrix.e(),
        matrix.f(),
    ];
    values.iter().all(|v| v.is_finite()).then(|| Key {
        values: values.map(|v| if v == 0. { 0 } else { v.to_bits() }),
    })
}

/// Form内も含めて全所属候補を数え、同じ幾何を持つ別objectを省略しない。
fn collect_keys<'a>(
    budget: &DecodeBudget,
    objects: &'a impl PdfPageObjectsCommon<'a>,
    depth: usize,
    keys: &mut Vec<Key>,
    complete: &mut bool,
) -> Result<(), ParseError> {
    let _ = budget.check_form_depth(depth);
    budget.check()?;
    for index in objects.as_range() {
        budget.check()?;
        let Ok(object) = objects.get(index) else {
            *complete = false;
            continue;
        };
        if let Some(form) = object.as_x_object_form_object() {
            collect_keys(budget, form, depth + 1, keys, complete)?;
        } else if let Some(text) = object.as_text_object() {
            if let Some(key) = key(text) {
                metadata_workspace::push(budget, keys, key)?;
            } else {
                *complete = false;
            }
        }
    }
    Ok(())
}
