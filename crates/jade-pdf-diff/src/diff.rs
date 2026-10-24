//! ページ → テキストブロック → 文字 の 3 段で比較する。
//!
//! ページを先に対応付けるので、途中に 1 ページ挟まっても以降が全部差分に
//! なることはない。ページ内はブロックを対応付けてから中身を比べる。

use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hash, Hasher};

use diff_align::{pair_with_anchors, Pairing, Step};
use diff_support::{trimmed_lcs_diff, TextProfile};
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

use crate::ir::{BlockIr, Bounds, DocumentIr, PageIr};

/// 出力 JSON の契約バージョン。
pub const DIFF_SCHEMA_VERSION: u32 = 1;

/// Intern complete ordered block text without copying page strings. A crop can
/// change furniture classification while every text block stays identical.
/// Hashes only select candidate classes; actual strings establish equality.
fn complete_text_classes<'a>(pages: impl Iterator<Item = &'a PageIr>) -> Vec<Option<usize>> {
    let state = std::collections::hash_map::RandomState::new();
    complete_text_classes_by(pages, |page| {
        let mut hasher = state.build_hasher();
        page.blocks.len().hash(&mut hasher);
        for block in &page.blocks {
            block.text.hash(&mut hasher);
        }
        hasher.finish()
    })
}

fn complete_text_classes_by<'a>(
    pages: impl Iterator<Item = &'a PageIr>,
    fingerprint: impl Fn(&PageIr) -> u64,
) -> Vec<Option<usize>> {
    let mut classes: HashMap<u64, Vec<(&PageIr, usize)>> = HashMap::new();
    let mut next = 0;
    pages
        .map(|page| {
            if page.blocks.iter().all(|b| b.text.trim().is_empty()) {
                return None;
            }
            let candidates = classes.entry(fingerprint(page)).or_default();
            if let Some((_, id)) = candidates.iter().find(|(other, _)| {
                other
                    .blocks
                    .iter()
                    .map(|b| b.text.as_str())
                    .eq(page.blocks.iter().map(|b| b.text.as_str()))
            }) {
                return Some(*id);
            }
            let id = next;
            next += 1;
            candidates.push((page, id));
            Some(id)
        })
        .collect()
}

/// Bounded bottom-k samples of ordered character shingles. Character bigrams
/// alone overvalue unrelated worksheets that repeat the same short vocabulary.
struct OrderedTextProfile {
    shingles: Box<[u64]>,
}

impl OrderedTextProfile {
    const SAMPLE: usize = 1024;

    fn new(text: &str) -> Self {
        let chars: Vec<_> = text.chars().filter(|c| !c.is_whitespace()).collect();
        let mut shingles: Vec<u64> = chars
            .windows(8)
            .map(|window| {
                window.iter().fold(0xcbf29ce484222325u64, |hash, c| {
                    (hash ^ u64::from(u32::from(*c))).wrapping_mul(0x100000001b3)
                })
            })
            .collect();
        shingles.sort_unstable();
        shingles.dedup();
        shingles.truncate(Self::SAMPLE);
        // Do not retain the full page-sized Vec capacity in the lazy cache.
        Self {
            shingles: shingles.into_boxed_slice(),
        }
    }

    fn similarity(&self, other: &Self) -> Option<f64> {
        if self.shingles.is_empty() || other.shingles.is_empty() {
            return None;
        }
        let (mut a, mut b, mut union, mut common) = (0, 0, 0, 0);
        while union < Self::SAMPLE && (a < self.shingles.len() || b < other.shingles.len()) {
            match (self.shingles.get(a), other.shingles.get(b)) {
                (Some(x), Some(y)) if x == y => {
                    a += 1;
                    b += 1;
                    common += 1;
                }
                (Some(x), Some(y)) if x < y => a += 1,
                (Some(_), Some(_)) => b += 1,
                (Some(_), None) => a += 1,
                (None, Some(_)) => b += 1,
                (None, None) => break,
            }
            union += 1;
        }
        let jaccard = common as f64 / union as f64;
        Some(2.0 * jaccard / (1.0 + jaccard))
    }
}

fn ordered_page_score(bigram: f64, old: &OrderedTextProfile, new: &OrderedTextProfile) -> f64 {
    // Prefer retained phrases over shared short-word vocabulary. Otherwise a
    // newly inserted worksheet can steal a page split across two editions.
    old.similarity(new)
        .map_or(bigram, |ordered| 0.2 * bigram + 0.8 * ordered)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffOptions {
    /// File/byte APIs additionally compare rendered pages. None requests only
    /// structured text and widget values. Pure-IR APIs cannot render appearances.
    pub visual: Option<crate::VisualDiffOptions>,
    /// ページ同士を同じページとみなす最小スコア。
    pub page_match_score: f64,
    /// ブロック同士を同じブロックとみなす最小スコア。
    pub block_match_score: f64,
    /// 初回の対応付けに残った削除 + 追加を「変更」に畳む最小の文字類似度。
    /// 同じ位置の置換は position_overlap による別経路でも回収する。
    pub text_change_score: f64,
    /// 位置の変化をこの pt 以下なら無視する (既定 = 1pt)。
    pub position_tolerance: f64,
    /// 文字サイズの変化をこの pt 以下なら無視する (既定 = 0.25pt)。
    pub font_size_tolerance: f64,
    /// 位置だけで拾い直すときに要求する矩形の重なり (小さいほうの面積比)。
    pub position_overlap: f64,
    /// 1 区間あたりに許す総当たりの組数。
    ///
    /// 内容が一致するページ / ブロックは先に錨として固定するので、総当たりに
    /// なるのは変更のあった区間だけ。それでも巨大な文書では効いてくるため
    /// 上限を設ける。超えた区間の一致を証明できない組は削除/追加のまま残し、
    /// diagnostics に対応付けが不完全なことを記録する。0 は総当たりを無効にする。
    pub max_pairwise: usize,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            visual: Some(crate::VisualDiffOptions::default()),
            page_match_score: 0.34,
            block_match_score: 0.34,
            text_change_score: 0.34,
            position_tolerance: 1.0,
            font_size_tolerance: 0.25,
            position_overlap: 0.6,
            max_pairwise: 40_000,
        }
    }
}

impl DiffOptions {
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(visual) = &self.visual {
            visual.validate()?;
        }
        for (name, score) in [
            ("page_match_score", self.page_match_score),
            ("block_match_score", self.block_match_score),
            ("text_change_score", self.text_change_score),
            ("position_overlap", self.position_overlap),
        ] {
            anyhow::ensure!(
                score.is_finite() && (0.0..=1.0).contains(&score),
                "{name} must be finite and between 0 and 1"
            );
        }
        for (name, tolerance) in [
            ("position_tolerance", self.position_tolerance),
            ("font_size_tolerance", self.font_size_tolerance),
        ] {
            anyhow::ensure!(
                tolerance.is_finite() && tolerance >= 0.0,
                "{name} must be finite and non-negative"
            );
        }
        Ok(())
    }
}

// The shared aligner pairs by ordinal position when its work budget is exceeded.
// Never present those unscored pairs as proven associations. Exact keys remain
// valid evidence, including anchors that need no similarity calculation.
fn verified_pairings<K: Hash + Eq>(
    old: &[K],
    new: &[K],
    threshold: f64,
    budget: usize,
    mut score: impl FnMut(usize, usize) -> f64,
) -> (Vec<Pairing>, bool) {
    let mut scored_old = vec![false; old.len()];
    let pairs = pair_with_anchors(old, new, threshold, budget, |i, j| {
        scored_old[i] = true;
        score(i, j)
    });
    let mut limited = false;
    let mut verified = Vec::with_capacity(pairs.len());
    for pair in pairs {
        if let Pairing::Matched { old: i, new: j, .. } = pair {
            if !scored_old[i] && old[i] != new[j] {
                limited = true;
                verified.extend([Pairing::Removed(i), Pairing::Added(j)]);
                continue;
            }
        }
        verified.push(pair);
    }
    (verified, limited)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Added,
    Removed,
    Modified,
    Moved,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
    Moved,
    Resized,
    Formatted,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub kind: SegmentKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockChange {
    pub kind: ChangeKind,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
    pub old_text: Option<String>,
    pub new_text: Option<String>,
    /// 抽出できた文字サイズ (pt)。
    pub old_font_size: Option<f64>,
    pub new_font_size: Option<f64>,
    /// ハイライトを重ねるための矩形 (PDF ユーザ空間)。
    pub old_bounds: Option<Bounds>,
    pub new_bounds: Option<Bounds>,
    /// 位置だけが動いたか。
    pub moved: bool,
    /// 文字領域の幅または高さが変わったか。
    pub resized: bool,
    /// 抽出できた文字サイズが変わったか。
    pub format_changed: bool,
    /// ページ番号のように、数字だけが変わったページ端の要素か。
    ///
    /// 中身は普通に報告するが、これだけしか変わっていないページを「変更あり」
    /// として数えると、1 枚挟んだだけで以降が全部変更に見えてしまう。
    pub minor: bool,
    /// `changed` のときだけ入る文字単位の内訳。
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
    pub user_unit: Option<f64>,
    pub rotation: u64,
    pub view_box: Bounds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDiff {
    /// 1 始まりのページ番号。
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
    pub status: Status,
    pub match_confidence: Option<f64>,
    pub old_size: Option<PageSize>,
    pub new_size: Option<PageSize>,
    /// 用紙の幅・高さ・表示回転・表示領域が変わったか。
    pub geometry_changed: bool,
    /// ブロックの対応付けが計算予算に制限されたか。
    pub block_alignment_limited: bool,
    pub visual: Option<crate::VisualPageDiff>,
    pub form_changes: Vec<crate::FormFieldChange>,
    pub blocks: Vec<BlockChange>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffSummary {
    /// Logical field records, including named parent nodes; separate from widgets.
    pub canonical_form_fields_changed: usize,
    pub pages_visual_changed: usize,
    /// Added, removed or modified widget field records.
    pub form_fields_changed: usize,
    pub pages_added: usize,
    pub pages_removed: usize,
    pub pages_moved: usize,
    pub pages_modified: usize,
    pub pages_geometry_changed: usize,
    pub blocks_added: usize,
    pub blocks_removed: usize,
    pub blocks_changed: usize,
    pub blocks_moved: usize,
    pub blocks_resized: usize,
    pub blocks_format_changed: usize,
    /// ページ番号の繰り上がりなど、数字だけの差。
    pub blocks_minor: usize,
}

/// 結果を読むときに知っておくべきこと。
///
/// PDF は「文字が入っていない」ことがあり (スキャンしただけのものなど)、
/// そのときは差分が出ないのが正しい動作になってしまう。黙って「差分なし」と
/// 出すと誤読につながるので、取り出せたかどうかを添えて返す。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Diagnostics {
    pub old_pages: usize,
    pub new_pages: usize,
    /// テキストが 1 つも取り出せなかったページ数。
    pub old_pages_without_text: usize,
    pub new_pages_without_text: usize,
    /// SDK mapping flags or Unicode replacement characters indicate unmapped text.
    /// Intentional replacement characters also require review conservatively.
    pub old_pages_with_unmapped_text: usize,
    pub new_pages_with_unmapped_text: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub old_pages_with_uncertain_reading_order: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub new_pages_with_uncertain_reading_order: usize,
    /// SDK text objects were not represented in the extracted character stream.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub old_pages_with_unobserved_text_objects: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub new_pages_with_unobserved_text_objects: usize,
    /// Encoded source glyph lower bound exceeds nongenerated SDK records.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub old_pages_with_missing_source_characters: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub new_pages_with_missing_source_characters: usize,
    /// Independent source analysis was unsupported, malformed or over budget.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub old_pages_with_unavailable_source_accounting: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub new_pages_with_unavailable_source_accounting: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub old_pages_with_source_unicode_mismatch: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub new_pages_with_source_unicode_mismatch: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub old_pages_with_unavailable_source_unicode: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub new_pages_with_unavailable_source_unicode: usize,
    pub old_pages_with_unknown_units: usize,
    pub new_pages_with_unknown_units: usize,
    pub page_alignment_limited: bool,
    pub block_alignment_limited_pages: usize,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

impl Diagnostics {
    pub fn alignment_limited(&self) -> bool {
        self.page_alignment_limited || self.block_alignment_limited_pages > 0
    }
    pub fn has_unmapped_text(&self) -> bool {
        self.old_pages_with_unmapped_text > 0 || self.new_pages_with_unmapped_text > 0
    }
    pub fn has_unobserved_text_objects(&self) -> bool {
        self.old_pages_with_unobserved_text_objects > 0
            || self.new_pages_with_unobserved_text_objects > 0
    }
    pub fn has_incomplete_source_text(&self) -> bool {
        self.old_pages_with_missing_source_characters > 0
            || self.new_pages_with_missing_source_characters > 0
            || self.old_pages_with_unavailable_source_accounting > 0
            || self.new_pages_with_unavailable_source_accounting > 0
            || self.old_pages_with_source_unicode_mismatch > 0
            || self.new_pages_with_source_unicode_mismatch > 0
            || self.old_pages_with_unavailable_source_unicode > 0
            || self.new_pages_with_unavailable_source_unicode > 0
    }
    pub fn has_uncertain_reading_order(&self) -> bool {
        self.old_pages_with_uncertain_reading_order > 0
            || self.new_pages_with_uncertain_reading_order > 0
    }
    /// 一部のページでも文字を比較できていないか。空白ページも含むため、
    /// 呼び出し側で描画確認または OCR の必要性を判断する。
    pub fn has_pages_without_text(&self) -> bool {
        self.old_pages_without_text > 0 || self.new_pages_without_text > 0
    }

    /// 片側でもテキストが全く取れていないか。
    pub fn text_missing(&self) -> bool {
        (self.old_pages > 0 && self.old_pages_without_text == self.old_pages)
            || (self.new_pages > 0 && self.new_pages_without_text == self.new_pages)
    }
}

// The decoder accepts a known constant instead of borrowing from input JSON.
type CoordinateSystem = &'static str;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentDiff {
    /// Document-level changes, including fields without any page widget.
    pub canonical_form_changes: Vec<crate::CanonicalFormChange>,
    pub schema_version: u32,
    /// 座標系の明示。PDFユーザ空間 (原点は左下、物理倍率は各ページのuser_unit)。
    #[serde(deserialize_with = "deserialize_coordinate_system")]
    pub coordinate_system: CoordinateSystem,
    pub old_file: String,
    pub new_file: String,
    pub summary: DiffSummary,
    pub diagnostics: Diagnostics,
    pub comparison: Option<crate::ComparisonCoverage>,
    pub pages: Vec<PageDiff>,
}

fn deserialize_coordinate_system<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<&'static str, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value == "pdf_user_space" {
        Ok("pdf_user_space")
    } else {
        Err(serde::de::Error::custom(
            "Unsupported PDF coordinate system",
        ))
    }
}

impl DocumentDiff {
    /// All requested layers completed at the configured raster resolution.
    /// This is not mathematical equivalence of arbitrary PDF objects. Interactive
    /// semantics not covered by widget reads require review unless source bytes
    /// are identical. Pure-IR comparisons do not supply appearance coverage.
    pub fn is_comparison_complete(&self) -> bool {
        let Some(coverage) = &self.comparison else {
            return false;
        };
        !self.diagnostics.alignment_limited()
            && (coverage.identical_source
                || (coverage.visual_dpi.is_some()
                    && coverage.skipped_page_pairs == 0
                    && coverage.interactive_pages_requiring_review == 0
                    && coverage.canonical_form_review_reasons.is_empty()
                    && self.diagnostics.old_pages_with_unknown_units == 0
                    && self.diagnostics.new_pages_with_unknown_units == 0
                    && !self.diagnostics.has_unmapped_text()
                    && !self.diagnostics.has_unobserved_text_objects()
                    && !self.diagnostics.has_incomplete_source_text()
                    && !self.diagnostics.has_uncertain_reading_order()))
    }
    /// Whether all pages supplied extractable text without replacement glyphs,
    /// and alignment completed within the configured work budget.
    /// This does not establish image, drawing or annotation equivalence.
    pub fn is_text_comparison_complete(&self) -> bool {
        !self.diagnostics.has_pages_without_text()
            && !self.diagnostics.has_unmapped_text()
            && !self.diagnostics.has_unobserved_text_objects()
            && !self.diagnostics.has_incomplete_source_text()
            && !self.diagnostics.has_uncertain_reading_order()
            && !self.diagnostics.alignment_limited()
    }
    pub fn has_changes(&self) -> bool {
        !self.canonical_form_changes.is_empty()
            || self
                .pages
                .iter()
                .any(|page| page.status != Status::Unchanged)
    }
}

/// 事前に検証したIRとオプションを比較する。
/// 外部入力には [`crate::try_diff_documents`] を使う。
pub fn diff_documents(
    old_file: &str,
    new_file: &str,
    old: &DocumentIr,
    new: &DocumentIr,
    options: &DiffOptions,
) -> DocumentDiff {
    // ページ本文を錨の鍵にする。変わっていないページは総当たりに回さずに
    // 固定できるので、1000 ページ級でも現実的な時間で終わる。
    // ページ番号は鍵から外す (1 枚挟むと以降が全部ずれて錨が消えるため)。
    let old_body: Vec<String> = old.pages.iter().map(PageIr::body_text).collect();
    let new_body: Vec<String> = new.pages.iter().map(PageIr::body_text).collect();
    let mut repeated = HashSet::new();
    for texts in [&old_body, &new_body] {
        let mut seen = HashSet::new();
        for text in texts {
            if !seen.insert(text.as_str()) {
                repeated.insert(text.as_str());
            }
        }
    }
    // Use the same text for anchors AND similarity. Pages with only short
    // headers must still match after a crop or rotation changes their key.
    let matching_text = |pages: &[PageIr], texts: &[String]| {
        pages
            .iter()
            .zip(texts)
            .map(|(page, text)| {
                if text.is_empty() || repeated.contains(text.as_str()) {
                    page.text()
                } else {
                    text.clone()
                }
            })
            .collect::<Vec<_>>()
    };
    let old_text = matching_text(&old.pages, &old_body);
    let new_text = matching_text(&new.pages, &new_body);
    // 同じ本文のページを並べ替えた場合も、表示回転や用紙寸法が同じものを
    // 錨として優先する。形状だけを編集した場合は本文のスコアで対応できる。
    let keys = |pages: &[PageIr], texts: &[String]| {
        pages
            .iter()
            .zip(texts)
            .map(|(page, text)| {
                (
                    // 同じ本文の扉や白紙は、柱・ページ番号で識別する。一意な本文
                    // のページでは番号を外し、挿入による改番に耐える錨を保つ。
                    text.clone(),
                    (page.width * 100.0).round() as i64,
                    (page.height * 100.0).round() as i64,
                    page.rotation % 360,
                    page.user_unit.map(f64::to_bits),
                    {
                        let b = page.effective_view_box();
                        [b.x0, b.y0, b.x1, b.y1].map(|v| (v * 100.0).round() as i64)
                    },
                )
            })
            .collect::<Vec<_>>()
    };
    let old_keys = keys(&old.pages, &old_text);
    let new_keys = keys(&new.pages, &new_text);
    let complete = complete_text_classes(old.pages.iter().chain(&new.pages));
    let (old_complete, new_complete) = complete.split_at(old.pages.len());
    // 錨の無い区間は総当たりになる。ページ本文の bigram は組ごとに作り直さず
    // 1 回だけ作る。錨になったページは比べないので、比べるときに初めて作る。
    let old_profiles: Vec<OnceCell<TextProfile>> =
        old_text.iter().map(|_| OnceCell::new()).collect();
    let new_profiles: Vec<OnceCell<TextProfile>> =
        new_text.iter().map(|_| OnceCell::new()).collect();
    let old_ordered: Vec<OnceCell<OrderedTextProfile>> =
        old_text.iter().map(|_| OnceCell::new()).collect();
    let new_ordered: Vec<OnceCell<OrderedTextProfile>> =
        new_text.iter().map(|_| OnceCell::new()).collect();
    let threshold = options.page_match_score;
    let (mut pairings, page_alignment_limited) = verified_pairings(
        &old_keys,
        &new_keys,
        // Reserve 1% for exact keys to break ties between duplicate pages with
        // identical text but different geometry. Scale the threshold equally so
        // geometry edits still qualify at the caller's original text threshold.
        threshold * 0.99,
        options.max_pairwise,
        |i, j| {
            if old_keys[i] == new_keys[j] {
                return 1.0;
            }
            if old_complete[i].is_some() && old_complete[i] == new_complete[j] {
                // Exact geometry keys still win ties between repeated pages.
                // Equal full text survives shifts of the furniture bands.
                return 0.99;
            }
            let old_profile = old_profiles[i].get_or_init(|| TextProfile::new(&old_text[i]));
            let new_profile = new_profiles[j].get_or_init(|| TextProfile::new(&new_text[j]));
            let bigram_score = old_profile.similarity_at_least(new_profile, threshold);
            let text_score = if bigram_score > 0.0 {
                ordered_page_score(
                    bigram_score,
                    old_ordered[i].get_or_init(|| OrderedTextProfile::new(&old_text[i])),
                    new_ordered[j].get_or_init(|| OrderedTextProfile::new(&new_text[j])),
                )
            } else {
                bigram_score
            };
            // A sparse page can consist entirely of the changed number or word.
            // Retain a weak, geometry-based candidate instead of automatically
            // reporting a deletion and an insertion. Strict text thresholds > .5
            // intentionally disable this fallback.
            if text_score < threshold
                && threshold <= 0.5
                && i.abs_diff(j) <= old.pages.len().abs_diff(new.pages.len()) + 1
                && (same_page_layout(&old.pages[i], &new.pages[j], options)
                    // No text evidence exists for these pages. With equal page
                    // counts, retain the positional geometry edit, but keep
                    // match_confidence=None and the incomplete-text diagnostic.
                    || (i == j && old.pages.len() == new.pages.len()
                        && old.pages[i].blocks.is_empty() && new.pages[j].blocks.is_empty()))
            {
                0.5 * 0.99
            } else {
                text_score * 0.99
            }
        },
    );
    // 錨の外へ移った完全一致ページは、共有 aligner から削除/追加として返る。
    // 区間をまたいで同じ鍵を回収し、本文を再比較せず移動として報告する。
    let mut removed_by_key: HashMap<_, Vec<usize>> = HashMap::new();
    for pair in pairings.iter().rev() {
        if let Pairing::Removed(i) = *pair {
            removed_by_key.entry(&old_keys[i]).or_default().push(i);
        }
    }
    let mut recovered_old = vec![false; old.pages.len()];
    for pair in &mut pairings {
        if let Pairing::Added(j) = *pair {
            if let Some(i) = removed_by_key.get_mut(&new_keys[j]).and_then(Vec::pop) {
                recovered_old[i] = true;
                *pair = Pairing::Matched {
                    old: i,
                    new: j,
                    moved: true,
                };
            }
        }
    }
    pairings.retain(|pair| !matches!(pair, Pairing::Removed(i) if recovered_old[*i]));

    let mut pages: Vec<PageDiff> = Vec::new();
    for pairing in &pairings {
        match *pairing {
            Pairing::Matched {
                old: o,
                new: n,
                moved,
            } => pages.push(diff_page(&old.pages[o], &new.pages[n], moved, options)),
            Pairing::Removed(o) => pages.push(one_sided_page(&old.pages[o], true)),
            Pairing::Added(n) => pages.push(one_sided_page(&new.pages[n], false)),
        }
    }

    // 新しい方の並びで見せる。削除ページは旧の並びで直前だったページの後ろへ。
    pages.sort_by_key(|page| {
        (
            page.new_index.unwrap_or(usize::MAX),
            page.old_index.unwrap_or(0),
        )
    });
    reposition_removed(&mut pages);

    let mut summary = DiffSummary::default();
    for page in &pages {
        accumulate(&mut summary, page);
    }

    let diagnostics = Diagnostics {
        page_alignment_limited,
        block_alignment_limited_pages: pages.iter().filter(|p| p.block_alignment_limited).count(),
        old_pages: old.pages.len(),
        new_pages: new.pages.len(),
        old_pages_without_text: old.pages.iter().filter(|p| p.blocks.is_empty()).count(),
        new_pages_without_text: new.pages.iter().filter(|p| p.blocks.is_empty()).count(),
        old_pages_with_unmapped_text: old.pages.iter().filter(|p| p.has_unmapped_text()).count(),
        new_pages_with_unmapped_text: new.pages.iter().filter(|p| p.has_unmapped_text()).count(),
        old_pages_with_uncertain_reading_order: old
            .pages
            .iter()
            .filter(|p| p.reading_order_requires_review)
            .count(),
        new_pages_with_uncertain_reading_order: new
            .pages
            .iter()
            .filter(|p| p.reading_order_requires_review)
            .count(),
        old_pages_with_unobserved_text_objects: old
            .pages
            .iter()
            .filter(|p| p.sdk_text_objects_without_characters > 0)
            .count(),
        new_pages_with_unobserved_text_objects: new
            .pages
            .iter()
            .filter(|p| p.sdk_text_objects_without_characters > 0)
            .count(),
        old_pages_with_missing_source_characters: old
            .pages
            .iter()
            .filter(|p| p.source_text_characters_missing > 0)
            .count(),
        new_pages_with_missing_source_characters: new
            .pages
            .iter()
            .filter(|p| p.source_text_characters_missing > 0)
            .count(),
        old_pages_with_unavailable_source_accounting: old
            .pages
            .iter()
            .filter(|p| p.source_text_accounting_unavailable)
            .count(),
        new_pages_with_unavailable_source_accounting: new
            .pages
            .iter()
            .filter(|p| p.source_text_accounting_unavailable)
            .count(),
        old_pages_with_source_unicode_mismatch: old
            .pages
            .iter()
            .filter(|p| p.source_unicode_scalars_unmatched > 0)
            .count(),
        new_pages_with_source_unicode_mismatch: new
            .pages
            .iter()
            .filter(|p| p.source_unicode_scalars_unmatched > 0)
            .count(),
        old_pages_with_unavailable_source_unicode: old
            .pages
            .iter()
            .filter(|p| p.source_unicode_accounting_unavailable)
            .count(),
        new_pages_with_unavailable_source_unicode: new
            .pages
            .iter()
            .filter(|p| p.source_unicode_accounting_unavailable)
            .count(),
        old_pages_with_unknown_units: old.pages.iter().filter(|p| p.user_unit.is_none()).count(),
        new_pages_with_unknown_units: new.pages.iter().filter(|p| p.user_unit.is_none()).count(),
    };

    DocumentDiff {
        canonical_form_changes: Vec::new(),
        schema_version: DIFF_SCHEMA_VERSION,
        coordinate_system: "pdf_user_space",
        old_file: old_file.to_string(),
        new_file: new_file.to_string(),
        summary,
        diagnostics,
        comparison: None,
        pages,
    }
}

fn same_page_layout(old: &PageIr, new: &PageIr, options: &DiffOptions) -> bool {
    !physical_frame_changed(old, new, options.position_tolerance)
        && !old.blocks.is_empty()
        && old.blocks.len() <= 64
        && old.blocks.len() == new.blocks.len()
        && old.blocks.iter().zip(&new.blocks).all(|(a, b)| {
            position_overlap_score(
                &a.bounds.scaled(old.user_unit.unwrap_or(1.0)),
                &b.bounds.scaled(new.user_unit.unwrap_or(1.0)),
                options.position_tolerance,
            ) >= options.position_overlap
        })
}

fn reposition_removed(pages: &mut Vec<PageDiff>) {
    let mut removed: Vec<PageDiff> = Vec::new();
    let mut kept: Vec<PageDiff> = Vec::new();
    for page in pages.drain(..) {
        if page.new_index.is_none() {
            removed.push(page);
        } else {
            kept.push(page);
        }
    }
    removed.sort_by_key(|page| page.old_index.unwrap_or(0));

    // removed は旧順なので、挿入先は後ろへ進むことしかない。各要素を Vec::insert
    // するとページ数の二乗回の移動になり得るため、挿入先ごとの bucket を作って
    // 一度だけ連結する。
    let mut buckets: Vec<Vec<PageDiff>> = (0..=kept.len()).map(|_| Vec::new()).collect();
    let mut position = 0usize;
    for page in removed {
        let old_index = page.old_index.unwrap_or(0);
        while position < kept.len()
            && kept[position]
                .old_index
                .is_none_or(|index| index <= old_index)
        {
            position += 1;
        }
        buckets[position].push(page);
    }

    let mut ordered = Vec::with_capacity(kept.len() + buckets.iter().map(Vec::len).sum::<usize>());
    for (position, page) in kept.into_iter().enumerate() {
        ordered.append(&mut buckets[position]);
        ordered.push(page);
    }
    ordered.append(buckets.last_mut().expect("末尾 bucket"));
    *pages = ordered;
}

fn page_size(page: &PageIr) -> PageSize {
    PageSize {
        user_unit: page.user_unit,
        width: page.width,
        height: page.height,
        rotation: page.rotation,
        view_box: page.effective_view_box(),
    }
}

fn one_sided_page(page: &PageIr, removed: bool) -> PageDiff {
    let blocks = page
        .blocks
        .iter()
        .map(|block| BlockChange {
            kind: if removed {
                ChangeKind::Removed
            } else {
                ChangeKind::Added
            },
            old_index: removed.then_some(block.index),
            new_index: (!removed).then_some(block.index),
            old_text: removed.then(|| block.text.clone()),
            new_text: (!removed).then(|| block.text.clone()),
            old_font_size: removed.then_some(block.size).flatten(),
            new_font_size: (!removed).then_some(block.size).flatten(),
            old_bounds: removed.then_some(block.bounds),
            new_bounds: (!removed).then_some(block.bounds),
            moved: false,
            resized: false,
            format_changed: false,
            minor: false,
            segments: Vec::new(),
        })
        .collect();

    PageDiff {
        old_index: removed.then_some(page.index),
        new_index: (!removed).then_some(page.index),
        status: if removed {
            Status::Removed
        } else {
            Status::Added
        },
        match_confidence: None,
        old_size: removed.then(|| page_size(page)),
        new_size: (!removed).then(|| page_size(page)),
        geometry_changed: false,
        block_alignment_limited: false,
        visual: None,
        form_changes: Vec::new(),
        blocks,
    }
}

/// ページ端にある要素で、数字だけが変わったか (ページ番号の繰り上がりなど)。
fn is_minor_change(
    old_page: &PageIr,
    new_page: &PageIr,
    old_block: &BlockIr,
    new_block: &BlockIr,
) -> bool {
    // 金額・日付・文書番号もページ端にある。数字が変わっただけでは隠さず、
    // 実際のページ挿入/削除に伴ってページ番号が変わったと確認できる場合に限る。
    old_page.index != new_page.index
        && pagination_number(&old_block.text) == Some(old_page.index)
        && pagination_number(&new_block.text) == Some(new_page.index)
        && old_page.is_furniture(old_block)
        && new_page.is_furniture(new_block)
        && old_block.text != new_block.text
        && digit_skeleton(&old_block.text) == digit_skeleton(&new_block.text)
}

/// 保守的に認識するページ番号。金額の区切り、日付、型番は認識しない。
fn pagination_number(text: &str) -> Option<usize> {
    let text = text.trim();
    let text = text
        .strip_prefix("Page ")
        .or_else(|| text.strip_prefix("page "))
        .unwrap_or(text);
    let text = text.trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '–' || c == '—');
    let text = text
        .strip_suffix('頁')
        .or_else(|| text.strip_suffix("ページ"))
        .unwrap_or(text)
        .trim();
    (!text.is_empty() && text.chars().all(|c| c.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn diff_page(old: &PageIr, new: &PageIr, moved: bool, options: &DiffOptions) -> PageDiff {
    let (raw_old, raw_new) = (old, new);
    let old_physical = physical_page(old);
    let new_physical = physical_page(new);
    let (old, new) = (&*old_physical, &*new_physical);
    // PDF の表示回転や保存し直しで抽出順が変わることがある。同じ本文・同じ
    // 場所の要素を先に確定し、繰り返す表セルや多段組みの行を取り違えない。
    let (mut matched, remaining_old, remaining_new) = stationary_blocks(old, new, options);
    let old_text: Vec<&str> = remaining_old
        .iter()
        .map(|&i| old.blocks[i].text.as_str())
        .collect();
    let new_text: Vec<&str> = remaining_new
        .iter()
        .map(|&i| new.blocks[i].text.as_str())
        .collect();
    let old_profiles: Vec<TextProfile> =
        old_text.iter().map(|text| TextProfile::new(text)).collect();
    let new_profiles: Vec<TextProfile> =
        new_text.iter().map(|text| TextProfile::new(text)).collect();
    let threshold = options.block_match_score;
    let (pairings, mut block_alignment_limited) = verified_pairings(
        &old_text,
        &new_text,
        threshold,
        options.max_pairwise,
        |i, j| {
            block_score(
                &old.blocks[remaining_old[i]],
                &new.blocks[remaining_new[j]],
                &old_profiles[i],
                &new_profiles[j],
                threshold,
            )
        },
    );

    let mut leftover_old: Vec<usize> = Vec::new();
    let mut leftover_new: Vec<usize> = Vec::new();
    for pairing in &pairings {
        match *pairing {
            Pairing::Matched { old: o, new: n, .. } => {
                matched.push((remaining_old[o], remaining_new[n]))
            }
            Pairing::Removed(o) => leftover_old.push(remaining_old[o]),
            Pairing::Added(n) => leftover_new.push(remaining_new[n]),
        }
    }
    block_alignment_limited |= recover_text_changes(
        old,
        new,
        &mut matched,
        &mut leftover_old,
        &mut leftover_new,
        options,
    );
    // 文字が似ていなくても、同じ場所に残っているものは同じブロックとみなす。
    // ページ番号 ("- 5 -" → "- 6 -") のように短い文字列は、書き換えでも
    // 類似度が 0 になってしまい、このひと押しが無いと削除 + 追加に割れる。
    pair_leftovers_by_position(
        old,
        new,
        &mut matched,
        &mut leftover_old,
        &mut leftover_new,
        options,
    );

    let mut blocks: Vec<BlockChange> = Vec::new();
    for (o, n) in matched {
        let mut change = diff_block(&old.blocks[o], &new.blocks[n], options);
        // Compute decisions in physical points; public geometry stays in each
        // source's raw PDF coordinate system for projection/highlighting.
        change.old_bounds = Some(raw_old.blocks[o].bounds);
        change.new_bounds = Some(raw_new.blocks[n].bounds);
        change.old_font_size = raw_old.blocks[o].size;
        change.new_font_size = raw_new.blocks[n].size;
        change.minor = !change.format_changed
            && !numeric_changed(
                old.blocks[o].bounds.y0,
                new.blocks[n].bounds.y0,
                options.position_tolerance,
            )
            && is_minor_change(old, new, &old.blocks[o], &new.blocks[n]);
        blocks.push(change);
    }
    for o in leftover_old {
        blocks.push(one_sided_block(&raw_old.blocks[o], true));
    }
    for n in leftover_new {
        blocks.push(one_sided_block(&raw_new.blocks[n], false));
    }
    blocks.sort_by_key(|block| {
        (
            block.new_index.unwrap_or(usize::MAX),
            block.old_index.unwrap_or(0),
        )
    });

    let geometry_changed = page_geometry_changed(raw_old, raw_new, options.position_tolerance);
    let changed = geometry_changed
        || blocks
            .iter()
            .any(|block| block.kind != ChangeKind::Unchanged && !block.minor);

    PageDiff {
        old_index: Some(old.index),
        new_index: Some(new.index),
        status: if changed {
            Status::Modified
        } else if moved {
            Status::Moved
        } else {
            Status::Unchanged
        },
        match_confidence: if old.blocks.is_empty() || new.blocks.is_empty() {
            None
        } else {
            let a = old.text();
            let b = new.text();
            Some(if a == b {
                1.0
            } else {
                ordered_page_score(
                    TextProfile::new(&a).similarity(&TextProfile::new(&b)),
                    &OrderedTextProfile::new(&a),
                    &OrderedTextProfile::new(&b),
                )
            })
        },
        old_size: Some(page_size(raw_old)),
        new_size: Some(page_size(raw_new)),
        geometry_changed,
        block_alignment_limited,
        visual: None,
        form_changes: Vec::new(),
        blocks,
    }
}

fn recover_text_changes(
    old: &PageIr,
    new: &PageIr,
    matched: &mut Vec<(usize, usize)>,
    left_old: &mut Vec<usize>,
    left_new: &mut Vec<usize>,
    options: &DiffOptions,
) -> bool {
    if left_old.is_empty() || left_new.is_empty() {
        return false;
    }
    if left_old.len().saturating_mul(left_new.len()) > options.max_pairwise {
        return true;
    }
    let old_profiles: Vec<_> = left_old
        .iter()
        .map(|&i| TextProfile::new(&old.blocks[i].text))
        .collect();
    let new_profiles: Vec<_> = left_new
        .iter()
        .map(|&i| TextProfile::new(&new.blocks[i].text))
        .collect();
    let pairs = diff_align::pair_by_score(
        left_old.len(),
        left_new.len(),
        options.text_change_score,
        |i, j| old_profiles[i].similarity_at_least(&new_profiles[j], options.text_change_score),
    );
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for pair in pairs {
        match pair {
            Pairing::Matched { old: i, new: j, .. } => matched.push((left_old[i], left_new[j])),
            Pairing::Removed(i) => removed.push(left_old[i]),
            Pairing::Added(j) => added.push(left_new[j]),
        }
    }
    *left_old = removed;
    *left_new = added;
    false
}

type BlockPairs = (Vec<(usize, usize)>, Vec<usize>, Vec<usize>);

fn stationary_blocks(old: &PageIr, new: &PageIr, options: &DiffOptions) -> BlockPairs {
    // 1pt の格子を用いて近傍だけを探索する。繰り返しの多い帳票でも本文ごとの
    // 全組合せにはしない。厳しい許容値を指定した呼び出し側の意図も守る。
    let tolerance = if options.position_tolerance.is_finite() {
        options.position_tolerance.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let cell = |b: &BlockIr| (b.bounds.x0.floor() as i64, b.bounds.y0.floor() as i64);
    let mut buckets: HashMap<(&str, i64, i64), Vec<usize>> = HashMap::new();
    for (i, block) in new.blocks.iter().enumerate() {
        let (x, y) = cell(block);
        buckets.entry((&block.text, x, y)).or_default().push(i);
    }
    let mut paired = Vec::new();
    let mut remaining_old = Vec::new();
    let mut used_new = vec![false; new.blocks.len()];
    let mut work = 0usize;
    for (i, block) in old.blocks.iter().enumerate() {
        if work >= options.max_pairwise {
            remaining_old.extend(i..old.blocks.len());
            break;
        }
        let (x, y) = cell(block);
        let mut best: Option<(usize, f64)> = None;
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                let key = (
                    block.text.as_str(),
                    x.saturating_add(dx),
                    y.saturating_add(dy),
                );
                if let Some(candidates) = buckets.get(&key) {
                    for &j in candidates {
                        if work >= options.max_pairwise {
                            break;
                        }
                        work += 1;
                        let b = &new.blocks[j];
                        let distance_x = (block.bounds.x0 - b.bounds.x0).abs();
                        let distance_y = (block.bounds.y0 - b.bounds.y0).abs();
                        if !used_new[j] && distance_x <= tolerance && distance_y <= tolerance {
                            let distance = distance_x + distance_y;
                            if best.is_none_or(|(index, score)| {
                                distance < score || (distance == score && j < index)
                            }) {
                                best = Some((j, distance));
                            }
                        }
                    }
                }
            }
        }
        if let Some((j, _)) = best {
            paired.push((i, j));
            used_new[j] = true;
        } else {
            remaining_old.push(i);
        }
    }
    let remaining_new = (0..new.blocks.len()).filter(|&j| !used_new[j]).collect();
    (paired, remaining_old, remaining_new)
}

fn physical_page(page: &PageIr) -> Cow<'_, PageIr> {
    let unit = page.user_unit.unwrap_or(1.0);
    if unit == 1.0 {
        return Cow::Borrowed(page);
    }
    let mut physical = page.clone();
    physical.width *= unit;
    physical.height *= unit;
    physical.view_box = Some(page.effective_view_box().scaled(unit));
    physical.user_unit = Some(1.0);
    for block in &mut physical.blocks {
        block.bounds = block.bounds.scaled(unit);
        block.size = block.size.map(|size| size * unit);
    }
    Cow::Owned(physical)
}

fn page_geometry_changed(old: &PageIr, new: &PageIr, tolerance: f64) -> bool {
    matches!((old.user_unit,new.user_unit),(Some(a),Some(b)) if a != b)
        || physical_frame_changed(old, new, tolerance)
}

fn physical_frame_changed(old: &PageIr, new: &PageIr, tolerance: f64) -> bool {
    let (au, bu) = (old.user_unit.unwrap_or(1.0), new.user_unit.unwrap_or(1.0));
    let a = old.effective_view_box().scaled(au);
    let b = new.effective_view_box().scaled(bu);
    numeric_changed(old.width * au, new.width * bu, tolerance)
        || numeric_changed(old.height * au, new.height * bu, tolerance)
        || old.rotation % 360 != new.rotation % 360
        || [(a.x0, b.x0), (a.y0, b.y0), (a.x1, b.x1), (a.y1, b.y1)]
            .into_iter()
            .any(|(x, y)| numeric_changed(x, y, tolerance))
}

fn numeric_changed(old: f64, new: f64, tolerance: f64) -> bool {
    let tolerance = if tolerance.is_finite() {
        tolerance.max(0.0)
    } else {
        0.0
    };
    !old.is_finite() || !new.is_finite() || (old - new).abs() > tolerance
}

/// 対応が付かなかったブロック同士を、位置の重なりだけで拾い直す。
///
/// 本文と取り違えないよう、ほぼ同じ矩形に収まっているものに限る。候補を
/// 重なり率の高い順で試しつつ増加路で組み替え、対応できる件数を最大にする。
fn pair_leftovers_by_position(
    old: &PageIr,
    new: &PageIr,
    matched: &mut Vec<(usize, usize)>,
    leftover_old: &mut Vec<usize>,
    leftover_new: &mut Vec<usize>,
    options: &DiffOptions,
) {
    if leftover_old.is_empty() || leftover_new.is_empty() {
        return;
    }
    // 錨による対応付けと同じ予算を守る。全く異なる巨大ページで候補行列を
    // 無制限に作らず、対応が不明なブロックを削除/追加として残す。
    if leftover_old.len().saturating_mul(leftover_new.len()) > options.max_pairwise {
        return;
    }
    let candidates: Vec<Vec<usize>> = leftover_old
        .iter()
        .map(|o| {
            let source = &old.blocks[*o].bounds;
            let mut choices: Vec<(usize, f64)> = leftover_new
                .iter()
                .enumerate()
                .map(|(position, n)| {
                    (
                        position,
                        position_overlap_score(
                            source,
                            &new.blocks[*n].bounds,
                            options.position_tolerance,
                        ),
                    )
                })
                .filter(|(_, ratio)| *ratio >= options.position_overlap)
                .collect();
            choices.sort_by(|a, b| {
                b.1.total_cmp(&a.1)
                    .then_with(|| leftover_new[a.0].cmp(&leftover_new[b.0]))
            });
            choices.into_iter().map(|(position, _)| position).collect()
        })
        .collect();

    let mut old_match: Vec<Option<usize>> = vec![None; leftover_old.len()];
    let mut new_match: Vec<Option<usize>> = vec![None; leftover_new.len()];
    for old_position in 0..leftover_old.len() {
        augment_position_match(old_position, &candidates, &mut old_match, &mut new_match);
    }

    for (new_position, old_position) in new_match.iter().enumerate() {
        if let Some(old_position) = old_position {
            matched.push((leftover_old[*old_position], leftover_new[new_position]));
        }
    }

    let still_old: Vec<usize> = leftover_old
        .iter()
        .enumerate()
        .filter(|(position, _)| old_match[*position].is_none())
        .map(|(_, index)| *index)
        .collect();
    let still_new: Vec<usize> = leftover_new
        .iter()
        .enumerate()
        .filter(|(position, _)| new_match[*position].is_none())
        .map(|(_, index)| *index)
        .collect();
    *leftover_old = still_old;
    *leftover_new = still_new;
    matched.sort_unstable();
}

fn augment_position_match(
    root: usize,
    candidates: &[Vec<usize>],
    old_match: &mut [Option<usize>],
    new_match: &mut [Option<usize>],
) -> bool {
    let mut visited_old = vec![false; candidates.len()];
    let mut visited_new = vec![false; new_match.len()];
    let mut parent_new: Vec<Option<usize>> = vec![None; new_match.len()];
    let mut stack = vec![(root, 0usize)];
    visited_old[root] = true;

    while let Some((old_position, next_choice)) = stack.last_mut() {
        let Some(&new_position) = candidates[*old_position].get(*next_choice) else {
            stack.pop();
            continue;
        };
        *next_choice += 1;
        if visited_new[new_position] {
            continue;
        }
        visited_new[new_position] = true;
        parent_new[new_position] = Some(*old_position);

        match new_match[new_position] {
            None => {
                // 見つけた増加路を終点から反転し、既存ペアを別候補へ押し出す。
                let mut cursor_new = new_position;
                loop {
                    let cursor_old = parent_new[cursor_new].expect("増加路の親");
                    let previous_new = old_match[cursor_old];
                    old_match[cursor_old] = Some(cursor_new);
                    new_match[cursor_new] = Some(cursor_old);
                    match previous_new {
                        Some(previous_new) => cursor_new = previous_new,
                        None => break,
                    }
                }
                return true;
            }
            Some(next_old) if !visited_old[next_old] => {
                visited_old[next_old] = true;
                stack.push((next_old, 0));
            }
            Some(_) => {}
        }
    }
    false
}

/// 2 つの矩形の重なりを、小さいほうの面積に対する割合で返す。
// Zero-area source regions are anchors, so area overlap supplies no evidence.
// Permit only matching degeneracy and nearly identical physical coordinates.
// Positive rectangles retain the existing overlap criterion unchanged.
fn position_overlap_score(a: &Bounds, b: &Bounds, tolerance: f64) -> f64 {
    let zero_a = (a.width() == 0.0, a.height() == 0.0);
    let zero_b = (b.width() == 0.0, b.height() == 0.0);
    if (zero_a.0 || zero_a.1) && zero_a == zero_b {
        let tolerance = if tolerance.is_finite() {
            tolerance.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if [(a.x0, b.x0), (a.y0, b.y0), (a.x1, b.x1), (a.y1, b.y1)]
            .into_iter()
            .all(|(a, b)| !numeric_changed(a, b, tolerance))
        {
            return 1.0;
        }
    }
    overlap_ratio(a, b)
}

fn overlap_ratio(a: &Bounds, b: &Bounds) -> f64 {
    let width = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0);
    let height = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    if a.width() <= 0.0 || a.height() <= 0.0 || b.width() <= 0.0 || b.height() <= 0.0 {
        return 0.0;
    }
    // Dividing before multiplying avoids overflow for large external OCR bounds.
    ((width / a.width()) * (height / a.height())).max((width / b.width()) * (height / b.height()))
}

/// ブロック同士の近さ。文字の一致に、同じ位置にあるかを少し足す。
///
/// `threshold` に届かないと長さだけで分かる組は、本文を突き合わせずに 0 を
/// 返す (閾値未満はどのみち対応付けに使われない)。
fn block_score(
    old: &BlockIr,
    new: &BlockIr,
    old_profile: &TextProfile,
    new_profile: &TextProfile,
    threshold: f64,
) -> f64 {
    let (ox, oy) = old.bounds.center();
    let (nx, ny) = new.bounds.center();
    // 半インチ (36pt) 以内なら同じ場所とみなして加点する。ヘッダやページ番号の
    // ように、文字は似ていても位置が違うものを取り違えないため。
    let near = ((ox - nx).abs() < 36.0 && (oy - ny).abs() < 36.0) as u8 as f64;
    let bonus = 0.12 * near;
    if old_profile.similarity_upper_bound(new_profile) + bonus < threshold {
        return 0.0;
    }
    (old_profile.similarity(new_profile) + bonus).min(1.0)
}

fn one_sided_block(block: &BlockIr, removed: bool) -> BlockChange {
    BlockChange {
        kind: if removed {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        },
        old_index: removed.then_some(block.index),
        new_index: (!removed).then_some(block.index),
        old_text: removed.then(|| block.text.clone()),
        new_text: (!removed).then(|| block.text.clone()),
        old_font_size: removed.then_some(block.size).flatten(),
        new_font_size: (!removed).then_some(block.size).flatten(),
        old_bounds: removed.then_some(block.bounds),
        new_bounds: (!removed).then_some(block.bounds),
        moved: false,
        resized: false,
        format_changed: false,
        minor: false,
        segments: Vec::new(),
    }
}

/// 数字を伏せた骨組み。"- 5 -" と "- 10 -" は同じ骨組みになる。
///
/// 連続した数字はまとめて 1 つに潰す。桁が繰り上がっても同じ骨組みとして
/// 扱いたい (9 ページ目の次は 10 ページ目になる)。
fn digit_skeleton(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_digits = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
            continue;
        }
        in_digits = false;
        out.push(c);
    }
    out
}

fn diff_block(old: &BlockIr, new: &BlockIr, options: &DiffOptions) -> BlockChange {
    let text_changed = old.text != new.text;
    let moved = numeric_changed(old.bounds.x0, new.bounds.x0, options.position_tolerance)
        || numeric_changed(old.bounds.y0, new.bounds.y0, options.position_tolerance);
    let resized = numeric_changed(
        old.bounds.width(),
        new.bounds.width(),
        options.position_tolerance,
    ) || numeric_changed(
        old.bounds.height(),
        new.bounds.height(),
        options.position_tolerance,
    );
    let format_changed = font_size_changed(old.size, new.size, options.font_size_tolerance);

    BlockChange {
        kind: if text_changed {
            ChangeKind::Changed
        } else if moved {
            ChangeKind::Moved
        } else if resized {
            ChangeKind::Resized
        } else if format_changed {
            ChangeKind::Formatted
        } else {
            ChangeKind::Unchanged
        },
        old_index: Some(old.index),
        new_index: Some(new.index),
        old_text: Some(old.text.clone()),
        new_text: Some(new.text.clone()),
        old_font_size: old.size,
        new_font_size: new.size,
        old_bounds: Some(old.bounds),
        new_bounds: Some(new.bounds),
        moved,
        resized,
        format_changed,
        // 呼び出し側がページを見て判定する。
        minor: false,
        segments: if text_changed {
            segment_text(&old.text, &new.text)
        } else {
            Vec::new()
        },
    }
}

fn font_size_changed(old: Option<f64>, new: Option<f64>, tolerance: f64) -> bool {
    match (old, new) {
        (Some(old), Some(new)) => numeric_changed(old, new, tolerance),
        // 片側で抽出できなかっただけなら、書式変更とは断定しない。
        _ => false,
    }
}

/// 書記素クラスタ単位の内訳。結合文字や ZWJ 絵文字を途中で切らない。
pub fn segment_text(old: &str, new: &str) -> Vec<Segment> {
    let old_graphemes: Vec<&str> = old.graphemes(true).collect();
    let new_graphemes: Vec<&str> = new.graphemes(true).collect();
    let steps = trimmed_lcs_diff(&old_graphemes, &new_graphemes);

    let mut out: Vec<Segment> = Vec::new();
    for step in steps {
        let (kind, grapheme) = match step {
            Step::Equal(o, _) => (SegmentKind::Equal, old_graphemes[o]),
            Step::Removed(o) => (SegmentKind::Delete, old_graphemes[o]),
            Step::Added(n) => (SegmentKind::Insert, new_graphemes[n]),
        };
        match out.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(grapheme),
            _ => out.push(Segment {
                kind,
                text: grapheme.to_string(),
            }),
        }
    }
    out
}

pub(crate) fn refresh_summary(diff: &mut DocumentDiff) {
    let mut summary = DiffSummary::default();
    for page in &diff.pages {
        accumulate(&mut summary, page);
    }
    diff.summary = summary;
}

fn accumulate(summary: &mut DiffSummary, page: &PageDiff) {
    summary.form_fields_changed += page.form_changes.len();
    if page
        .visual
        .as_ref()
        .is_some_and(|v| v.status == crate::VisualStatus::Changed)
    {
        summary.pages_visual_changed += 1;
    }
    match page.status {
        Status::Added => summary.pages_added += 1,
        Status::Removed => summary.pages_removed += 1,
        Status::Moved => summary.pages_moved += 1,
        Status::Modified => summary.pages_modified += 1,
        Status::Unchanged => {}
    }

    // まるごと足された / 消えたページの中身は数えない。「1 ページ追加」で
    // 済む話を「1 ページ追加・行 8 個追加」と並べると、何ページ直したのかが
    // 読み取れなくなる。中身は各ページの明細に入っている。
    if matches!(page.status, Status::Added | Status::Removed) {
        return;
    }

    if page.geometry_changed {
        summary.pages_geometry_changed += 1;
    }

    for block in &page.blocks {
        if block.minor {
            summary.blocks_minor += 1;
            continue;
        }
        match block.kind {
            ChangeKind::Added => summary.blocks_added += 1,
            ChangeKind::Removed => summary.blocks_removed += 1,
            ChangeKind::Changed => summary.blocks_changed += 1,
            ChangeKind::Moved => summary.blocks_moved += 1,
            ChangeKind::Resized => {}
            ChangeKind::Formatted => {}
            ChangeKind::Unchanged => {}
        }
        if block.resized {
            summary.blocks_resized += 1;
        }
        if block.format_changed {
            summary.blocks_format_changed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;

    fn unicode_text() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop_oneof![
                3 => any::<char>().prop_map(|character| character.to_string()),
                2 => prop::sample::select(vec![
                    "a", "日", "\u{0301}", "\u{200d}", "👩", "💻", "🏽", "🇯", "🇵", "\r",
                    "\n", "　",
                ])
                .prop_map(str::to_owned),
            ],
            0..24,
        )
        .prop_map(|parts| parts.concat())
    }

    fn boundaries(text: &str) -> BTreeSet<usize> {
        std::iter::once(0)
            .chain(
                text.grapheme_indices(true)
                    .map(|(start, grapheme)| start + grapheme.len()),
            )
            .collect()
    }

    fn block(index: usize, y0: f64, y1: f64, text: &str) -> BlockIr {
        BlockIr {
            index,
            text: text.to_string(),
            bounds: Bounds {
                x0: 60.0,
                y0,
                x1: 300.0,
                y1,
            },
            size: Some(11.0),
        }
    }

    fn block_at(index: usize, x0: f64, x1: f64, text: &str) -> BlockIr {
        BlockIr {
            index,
            text: text.to_string(),
            bounds: Bounds {
                x0,
                y0: 100.0,
                x1,
                y1: 110.0,
            },
            size: Some(11.0),
        }
    }

    #[test]
    fn anchor_matching_requires_identical_degeneracy_and_bounded_positions() {
        let point = Bounds {
            x0: 30.,
            y0: 150.,
            x1: 30.,
            y1: 150.,
        };
        let shifted = Bounds {
            x0: 30.5,
            y0: 150.,
            x1: 30.5,
            y1: 150.,
        };
        assert_eq!(position_overlap_score(&point, &point, 0.), 1.);
        assert_eq!(position_overlap_score(&point, &shifted, 1.), 1.);
        assert_eq!(position_overlap_score(&point, &shifted, 0.), 0.);
        let far = Bounds {
            x0: 40.,
            x1: 40.,
            ..point
        };
        assert_eq!(position_overlap_score(&point, &far, 100.), 0.);
        let line = Bounds { x1: 45., ..point };
        assert_eq!(position_overlap_score(&point, &line, 1.), 0.);
        assert_eq!(position_overlap_score(&line, &line, 0.), 1.);
        let rectangle = Bounds { y1: 160., ..line };
        assert_eq!(position_overlap_score(&line, &rectangle, 1.), 0.);
        assert_eq!(
            position_overlap_score(&rectangle, &rectangle, 0.),
            overlap_ratio(&rectangle, &rectangle)
        );
    }

    fn page(blocks: Vec<BlockIr>) -> PageIr {
        PageIr {
            index: 1,
            width: 595.0,
            height: 842.0,
            rotation: 0,
            view_box: None,
            user_unit: Some(1.0),
            sdk_unicode_mapping_errors: 0,
            sdk_text_objects_without_characters: 0,
            source_text_characters_missing: 0,
            source_text_accounting_unavailable: false,
            source_unicode_scalars_unmatched: 0,
            source_unicode_accounting_unavailable: false,
            reading_order_requires_review: false,
            blocks,
        }
    }

    #[test]
    fn reading_order_uncertainty_roundtrips_and_gates_both_completion_methods() {
        let old = DocumentIr {
            pages: vec![page(vec![block(1, 100.0, 112.0, "kept")])],
        };
        let mut new = old.clone();
        new.pages[0].reading_order_requires_review = true;
        let mut diff = diff_documents("old", "new", &old, &new, &DiffOptions::default());
        assert_eq!(diff.diagnostics.old_pages_with_uncertain_reading_order, 0);
        assert_eq!(diff.diagnostics.new_pages_with_uncertain_reading_order, 1);
        diff.comparison = Some(crate::ComparisonCoverage {
            visual_dpi: Some(144),
            ..Default::default()
        });
        assert!(!diff.is_comparison_complete());
        assert!(!diff.is_text_comparison_complete());
        let decoded: DocumentDiff =
            serde_json::from_slice(&serde_json::to_vec(&diff).unwrap()).unwrap();
        assert!(decoded.diagnostics.has_uncertain_reading_order());
        diff.comparison.as_mut().unwrap().identical_source = true;
        assert!(diff.is_comparison_complete());
        assert!(!diff.is_text_comparison_complete());
        let mut clean = diff_documents("old", "same", &old, &old, &DiffOptions::default());
        clean.comparison = Some(crate::ComparisonCoverage {
            visual_dpi: Some(144),
            ..Default::default()
        });
        assert!(clean.is_comparison_complete());
        assert!(clean.is_text_comparison_complete());
        let legacy_page: PageIr = serde_json::from_value(serde_json::json!({
            "index":1,"width":612,"height":792,"rotation":0,"blocks":[]
        }))
        .unwrap();
        assert!(!legacy_page.reading_order_requires_review);
        let legacy_layer: crate::TextRun = serde_json::from_value(serde_json::json!({
            "text":"x","sdk_unicode_mapping_errors":0,"size":12,
            "bounds":{"x0":1,"y0":1,"x1":2,"y1":2},"chars":[{"x0":1,"x1":2}]
        }))
        .unwrap();
        assert!(!legacy_layer.reading_order_requires_review);
        assert_eq!(legacy_layer.sdk_text_objects_without_characters, 0);
    }

    #[test]
    fn source_accounting_diagnostics_roundtrip_and_gate_pure_ir_text_completion() {
        let old = DocumentIr {
            pages: vec![page(vec![block(1, 10.0, 20.0, "C")])],
        };
        for unavailable in [false, true] {
            let mut new = old.clone();
            new.pages[0].source_text_characters_missing = usize::from(!unavailable);
            new.pages[0].source_text_accounting_unavailable = unavailable;
            let result = diff_documents("old", "new", &old, &new, &DiffOptions::default());
            assert!(!result.is_text_comparison_complete());
            assert!(result.diagnostics.has_incomplete_source_text());
            let decoded: DocumentDiff =
                serde_json::from_value(serde_json::to_value(result).unwrap()).unwrap();
            assert!(decoded.diagnostics.has_incomplete_source_text());
        }
        let value = serde_json::to_value(&old.pages[0]).unwrap();
        assert!(value.get("source_text_characters_missing").is_none());
        assert!(value.get("source_text_accounting_unavailable").is_none());
    }

    #[test]
    fn unobserved_text_objects_roundtrip_and_gate_completion() {
        let old = DocumentIr {
            pages: vec![page(vec![block(1, 100.0, 112.0, "kept")])],
        };
        let mut new = old.clone();
        new.pages[0].sdk_text_objects_without_characters = 2;
        let mut diff = diff_documents("old", "new", &old, &new, &DiffOptions::default());
        diff.comparison = Some(crate::ComparisonCoverage {
            visual_dpi: Some(144),
            ..Default::default()
        });
        assert!(!diff.is_comparison_complete());
        assert!(!diff.is_text_comparison_complete());
        let decoded: DocumentDiff =
            serde_json::from_slice(&serde_json::to_vec(&diff).unwrap()).unwrap();
        assert_eq!(
            decoded.diagnostics.new_pages_with_unobserved_text_objects,
            1
        );
        let page: PageIr =
            serde_json::from_slice(&serde_json::to_vec(&new.pages[0]).unwrap()).unwrap();
        assert_eq!(page.sdk_text_objects_without_characters, 2);
        diff.comparison.as_mut().unwrap().identical_source = true;
        assert!(diff.is_comparison_complete());
        assert!(!diff.is_text_comparison_complete());
        let clean = diff_documents("old", "old", &old, &old, &DiffOptions::default());
        assert!(clean.is_text_comparison_complete());
        let value = serde_json::to_value(&old.pages[0]).unwrap();
        assert!(value.get("sdk_text_objects_without_characters").is_none());
        assert_eq!(
            serde_json::from_value::<PageIr>(value)
                .unwrap()
                .sdk_text_objects_without_characters,
            0
        );
    }

    fn indexed_page(index: usize, text: &str) -> PageIr {
        let mut page = page(vec![block(1, 400.0, 415.0, text)]);
        page.index = index;
        page
    }

    #[test]
    fn removed_pages_are_repositioned_in_linear_pass_without_changing_order() {
        let options = DiffOptions::default();
        let matched = |old_index, new_index, text| {
            diff_page(
                &indexed_page(old_index, text),
                &indexed_page(new_index, text),
                old_index != new_index,
                &options,
            )
        };
        // kept は新順で、旧順は移動により単調とは限らない。追加ページには
        // old_index がない。削除ページは diff_documents と同じく末尾に置く。
        let mut pages = vec![
            one_sided_page(&indexed_page(1, "added"), false),
            matched(4, 2, "old 4"),
            matched(2, 3, "old 2"),
            matched(7, 4, "old 7"),
            one_sided_page(&indexed_page(1, "removed 1"), true),
            one_sided_page(&indexed_page(3, "removed 3"), true),
            one_sided_page(&indexed_page(5, "removed 5"), true),
            one_sided_page(&indexed_page(6, "removed 6"), true),
            one_sided_page(&indexed_page(8, "removed 8"), true),
        ];

        reposition_removed(&mut pages);

        assert_eq!(
            pages.iter().map(|page| page.old_index).collect::<Vec<_>>(),
            vec![
                None,
                Some(1),
                Some(3),
                Some(4),
                Some(2),
                Some(5),
                Some(6),
                Some(7),
                Some(8)
            ]
        );
    }

    #[test]
    fn 本文が同じでもページサイズと回転の変更を報告する() {
        let old = page(vec![block(1, 400.0, 415.0, "同じ本文")]);
        let mut new = old.clone();
        new.width += 20.0;
        new.height += 30.0;
        new.rotation = 90;

        let diff = diff_page(&old, &new, false, &DiffOptions::default());
        assert_eq!(diff.status, Status::Modified, "{diff:?}");
        assert!(diff.geometry_changed);
    }

    #[test]
    fn 本文と起点が同じでも文字領域のサイズ変更を報告する() {
        let old = page(vec![block_at(1, 60.0, 300.0, "同じ本文")]);
        let mut new = old.clone();
        new.blocks[0].bounds.x1 += 20.0;
        new.blocks[0].bounds.y1 += 5.0;

        let diff = diff_page(&old, &new, false, &DiffOptions::default());
        assert_eq!(diff.status, Status::Modified, "{diff:?}");
        assert_eq!(diff.blocks[0].kind, ChangeKind::Resized);
        assert!(diff.blocks[0].resized);
        assert!(!diff.blocks[0].moved);
    }

    #[test]
    fn 文字領域サイズの微小差は許容する() {
        let old = page(vec![block_at(1, 60.0, 300.0, "同じ本文")]);
        let mut within = old.clone();
        within.blocks[0].bounds.x1 += 1.0;
        within.blocks[0].bounds.y1 -= 1.0;
        let diff = diff_page(&old, &within, false, &DiffOptions::default());
        assert_eq!(diff.blocks[0].kind, ChangeKind::Unchanged);

        let mut outside = within;
        outside.blocks[0].bounds.x1 += 0.001;
        let diff = diff_page(&old, &outside, false, &DiffOptions::default());
        assert_eq!(diff.blocks[0].kind, ChangeKind::Resized);
    }

    #[test]
    fn 本文変更と文字領域サイズ変更を両方集計する() {
        let old_page = page(vec![block_at(1, 60.0, 300.0, "変更前")]);
        let mut new_page = page(vec![block_at(1, 60.0, 300.0, "変更後")]);
        new_page.blocks[0].bounds.x1 += 20.0;

        let diff = diff_documents(
            "old.pdf",
            "new.pdf",
            &DocumentIr {
                pages: vec![old_page],
            },
            &DocumentIr {
                pages: vec![new_page],
            },
            &DiffOptions::default(),
        );
        assert_eq!(diff.pages[0].blocks[0].kind, ChangeKind::Changed);
        assert!(diff.pages[0].blocks[0].resized);
        assert_eq!(diff.summary.blocks_changed, 1);
        assert_eq!(diff.summary.blocks_resized, 1);
    }

    #[test]
    fn 本文と領域が同じでも文字サイズ変更を報告する() {
        let old = page(vec![block_at(1, 60.0, 300.0, "同じ本文")]);
        let mut new = old.clone();
        new.blocks[0].size = Some(14.0);

        let diff = diff_page(&old, &new, false, &DiffOptions::default());
        assert_eq!(diff.status, Status::Modified, "{diff:?}");
        assert_eq!(diff.blocks[0].kind, ChangeKind::Formatted);
        assert!(diff.blocks[0].format_changed);
        assert_eq!(diff.blocks[0].old_font_size, Some(11.0));
        assert_eq!(diff.blocks[0].new_font_size, Some(14.0));
    }

    #[test]
    fn 文字サイズの微小差と片側の抽出失敗は許容する() {
        let old = page(vec![block_at(1, 60.0, 300.0, "同じ本文")]);
        let mut within = old.clone();
        within.blocks[0].size = Some(11.25);
        let diff = diff_page(&old, &within, false, &DiffOptions::default());
        assert_eq!(diff.blocks[0].kind, ChangeKind::Unchanged);

        let mut outside = old.clone();
        outside.blocks[0].size = Some(11.251);
        let diff = diff_page(&old, &outside, false, &DiffOptions::default());
        assert_eq!(diff.blocks[0].kind, ChangeKind::Formatted);

        let mut unavailable = old.clone();
        unavailable.blocks[0].size = None;
        let diff = diff_page(&old, &unavailable, false, &DiffOptions::default());
        assert_eq!(diff.blocks[0].kind, ChangeKind::Unchanged);
    }

    #[test]
    fn 本文変更と文字サイズ変更を両方集計する() {
        let old_page = page(vec![block_at(1, 60.0, 300.0, "変更前")]);
        let mut new_page = page(vec![block_at(1, 60.0, 300.0, "変更後")]);
        new_page.blocks[0].size = Some(14.0);

        let diff = diff_documents(
            "old.pdf",
            "new.pdf",
            &DocumentIr {
                pages: vec![old_page],
            },
            &DocumentIr {
                pages: vec![new_page],
            },
            &DiffOptions::default(),
        );
        assert_eq!(diff.pages[0].blocks[0].kind, ChangeKind::Changed);
        assert!(diff.pages[0].blocks[0].format_changed);
        assert_eq!(diff.summary.blocks_changed, 1);
        assert_eq!(diff.summary.blocks_format_changed, 1);
    }

    #[test]
    fn ページ寸法の微小差と一周分の回転は許容する() {
        let old = page(Vec::new());
        let mut within = old.clone();
        within.width += 1.0;
        within.height -= 1.0;
        within.rotation = 360;
        assert!(!page_geometry_changed(&old, &within, 1.0));

        let mut outside = within;
        outside.width += 0.001;
        assert!(page_geometry_changed(&old, &outside, 1.0));
    }

    #[test]
    fn the_digit_skeleton_survives_a_carry() {
        // 9 ページ目の次は 10 ページ目。桁が増えても同じ骨組み。
        assert_eq!(digit_skeleton("- 9 -"), digit_skeleton("- 10 -"));
        assert_eq!(digit_skeleton("第 3 版"), digit_skeleton("第 12 版"));
    }

    #[test]
    fn the_digit_skeleton_keeps_the_words_apart() {
        assert_ne!(digit_skeleton("第 1 章"), digit_skeleton("第 1 節"));
    }

    #[test]
    fn a_shifted_page_number_is_minor() {
        let mut old = page(vec![block(1, 40.0, 55.0, "- 9 -")]);
        let mut new = page(vec![block(1, 40.0, 55.0, "- 10 -")]);
        old.index = 9;
        new.index = 10;
        // 桁が増えると文字領域も自然に広がりうる。
        new.blocks[0].bounds.x1 += 10.0;
        let diff = diff_page(&old, &new, false, &DiffOptions::default());

        assert_eq!(diff.blocks[0].kind, ChangeKind::Changed);
        assert!(diff.blocks[0].resized);
        assert!(diff.blocks[0].minor, "ページ番号の繰り上がりのはず");
        // 数字が動いただけのページを「変更あり」にはしない。
        assert_eq!(diff.status, Status::Unchanged);
    }

    #[test]
    fn footer_amounts_dates_and_identifiers_are_substantive_changes() {
        for (before, after) in [
            ("2,970", "3,970"),
            ("100", "200"),
            ("(2025)", "(2026)"),
            ("2/10/04", "2/11/04"),
            ("OMB No. 1545-0074", "OMB No. 1545-0075"),
            ("第 3 条", "第 4 条"),
        ] {
            let old = page(vec![block(1, 30.0, 45.0, before)]);
            let new = page(vec![block(1, 30.0, 45.0, after)]);
            let diff = diff_page(&old, &new, false, &DiffOptions::default());
            assert_eq!(diff.status, Status::Modified, "{before} -> {after}");
            assert!(!diff.blocks[0].minor);
        }
    }

    #[test]
    fn pagination_does_not_hide_font_or_vertical_position_changes() {
        let mut old = page(vec![block(1, 30.0, 45.0, "9")]);
        let mut new = page(vec![block(1, 30.0, 45.0, "10")]);
        old.index = 9;
        new.index = 10;
        new.blocks[0].size = Some(20.0);
        assert_eq!(
            diff_page(&old, &new, false, &DiffOptions::default()).status,
            Status::Modified
        );
        new.blocks[0].size = old.blocks[0].size;
        new.blocks[0].bounds.y0 += 5.0;
        new.blocks[0].bounds.y1 += 5.0;
        assert_eq!(
            diff_page(&old, &new, false, &DiffOptions::default()).status,
            Status::Modified
        );
    }

    #[test]
    fn position_recovery_respects_the_pairwise_budget() {
        let old = page(vec![block(1, 300.0, 315.0, "old")]);
        let new = page(vec![block(1, 300.0, 315.0, "new")]);
        let mut matched = Vec::new();
        let mut left_old = vec![0];
        let mut left_new = vec![0];
        pair_leftovers_by_position(
            &old,
            &new,
            &mut matched,
            &mut left_old,
            &mut left_new,
            &DiffOptions {
                max_pairwise: 0,
                ..DiffOptions::default()
            },
        );
        assert!(matched.is_empty());
        assert_eq!(left_old, vec![0]);
        assert_eq!(left_new, vec![0]);
    }

    #[test]
    fn repeated_text_matches_its_position_when_extraction_order_changes() {
        let old = page(vec![
            block_at(1, 40.0, 100.0, "Other"),
            block_at(2, 200.0, 260.0, "Other"),
            block_at(3, 380.0, 440.0, "Other"),
        ]);
        let mut new = old.clone();
        new.blocks.reverse();
        for (i, block) in new.blocks.iter_mut().enumerate() {
            block.index = i + 1;
        }
        let result = diff_page(&old, &new, false, &DiffOptions::default());
        assert_eq!(result.status, Status::Unchanged, "{result:?}");
        assert_eq!(result.blocks.len(), 3);
        assert!(result.blocks.iter().all(|b| b.old_bounds == b.new_bounds));
    }

    #[test]
    fn repeated_page_text_uses_geometry_to_identify_reordered_pages() {
        let a = page(vec![block(1, 300.0, 315.0, "Same text")]);
        let mut b = a.clone();
        b.index = 2;
        b.rotation = 90;
        let old = DocumentIr { pages: vec![a, b] };
        let mut new = old.clone();
        new.pages.reverse();
        for (i, p) in new.pages.iter_mut().enumerate() {
            p.index = i + 1;
        }
        let result = diff_documents("a", "b", &old, &new, &DiffOptions::default());
        assert_eq!(result.summary.pages_modified, 0, "{result:?}");
        assert_eq!(result.summary.pages_geometry_changed, 0);
        assert_eq!(result.pages[0].old_index, Some(2));
        assert_eq!(result.pages[1].old_index, Some(1));
    }

    #[test]
    fn identical_complete_text_matches_across_changed_furniture_bands() {
        for include_body in [false, true] {
            let mut blocks = vec![
                block(1, 610.0, 625.0, "Cells"),
                block(2, 675.0, 690.0, "Header"),
            ];
            if include_body {
                blocks.insert(0, block(3, 300.0, 315.0, "Body"));
            }
            for (index, block) in blocks.iter_mut().enumerate() {
                block.index = index + 1;
            }
            let mut old = page(blocks);
            old.height = 700.0;
            old.view_box = Some(Bounds {
                x0: 0.0,
                y0: 0.0,
                x1: old.width,
                y1: 700.0,
            });
            let mut new = old.clone();
            new.height = 640.0;
            new.view_box.as_mut().unwrap().y1 = 640.0;
            assert_ne!(old.body_text(), new.body_text());
            assert_eq!(old.text(), new.text());
            let options = DiffOptions {
                page_match_score: 1.0,
                ..DiffOptions::default()
            };
            let result = diff_documents(
                "old",
                "new",
                &DocumentIr {
                    pages: vec![old.clone()],
                },
                &DocumentIr {
                    pages: vec![new.clone()],
                },
                &options,
            );
            assert_eq!(result.summary.pages_added, 0);
            assert_eq!(result.summary.pages_removed, 0);
            assert_eq!(result.summary.pages_geometry_changed, 1);
            assert_eq!(result.summary.blocks_changed, 0);
            new.blocks.last_mut().unwrap().text = "Different header".into();
            let result = diff_documents(
                "old",
                "new",
                &DocumentIr { pages: vec![old] },
                &DocumentIr { pages: vec![new] },
                &options,
            );
            assert_eq!(result.summary.pages_added, 1);
            assert_eq!(result.summary.pages_removed, 1);
        }
    }

    #[test]
    fn complete_text_classes_keep_order_and_verify_actual_strings() {
        let a = page(vec![
            block(1, 100.0, 110.0, "A"),
            block(2, 200.0, 210.0, "B"),
        ]);
        let mut b = a.clone();
        b.rotation = 90;
        b.blocks[0].bounds.x0 += 50.0;
        let mut reversed = a.clone();
        reversed.blocks.reverse();
        for (index, block) in reversed.blocks.iter_mut().enumerate() {
            block.index = index + 1;
        }
        let blank = page(vec![]);
        let classes = complete_text_classes([&a, &b, &reversed, &blank].into_iter());
        assert_eq!(classes[0], classes[1]);
        assert_ne!(classes[0], classes[2]);
        assert_eq!(classes[3], None);
        let collisions = complete_text_classes_by([&a, &b, &reversed, &blank].into_iter(), |_| 0);
        assert_eq!(
            collisions, classes,
            "A fingerprint collision must still compare every actual string"
        );
    }

    #[test]
    fn repeated_page_bodies_use_furniture_to_disambiguate_reordering() {
        let a = page(vec![
            block(1, 300.0, 315.0, "Same body"),
            block(2, 30.0, 45.0, "i"),
        ]);
        let mut b = a.clone();
        b.index = 2;
        b.blocks[1].text = "xvii".into();
        let old = DocumentIr { pages: vec![a, b] };
        let mut new = old.clone();
        new.pages.reverse();
        for (i, p) in new.pages.iter_mut().enumerate() {
            p.index = i + 1;
        }
        let result = diff_documents("a", "b", &old, &new, &DiffOptions::default());
        assert_eq!(result.summary.pages_modified, 0, "{result:?}");
        assert_eq!(result.summary.pages_added, 0);
        assert_eq!(result.summary.pages_removed, 0);
        assert_eq!(result.pages[0].old_index, Some(2));
        assert_eq!(result.pages[1].old_index, Some(1));
    }

    #[test]
    fn body_text_at_the_edge_is_not_minor() {
        let long = "この条項は本文であって、ページ番号でも柱でもない長さを持つ。";
        let old = page(vec![block(1, 40.0, 55.0, long)]);
        let new = page(vec![block(
            1,
            40.0,
            55.0,
            "この条項は本文であって、ページ番号でも柱でもない長さを持った。",
        )]);
        let diff = diff_page(&old, &new, false, &DiffOptions::default());

        assert!(!diff.blocks[0].minor);
        assert_eq!(diff.status, Status::Modified);
    }

    #[test]
    fn a_changed_page_number_in_the_body_is_not_minor() {
        // ページの真ん中にある「第 3 条」は、数字だけの差でも本文の変更。
        let old = page(vec![block(1, 400.0, 415.0, "第 3 条")]);
        let new = page(vec![block(1, 400.0, 415.0, "第 4 条")]);
        let diff = diff_page(&old, &new, false, &DiffOptions::default());

        assert!(!diff.blocks[0].minor);
        assert_eq!(diff.status, Status::Modified);
    }

    #[test]
    fn short_blocks_pair_up_by_position() {
        // "- 5 -" と "- 6 -" は字面が短すぎて似ていないと判定される。
        // 同じ場所にある以上、消えて生えたのではなく書き換わったとみなす。
        let old = page(vec![block(1, 40.0, 55.0, "- 5 -")]);
        let new = page(vec![block(1, 40.0, 55.0, "- 6 -")]);
        let diff = diff_page(&old, &new, false, &DiffOptions::default());

        assert_eq!(diff.blocks.len(), 1, "{:?}", diff.blocks);
        assert_eq!(diff.blocks[0].kind, ChangeKind::Changed);
    }

    #[test]
    fn far_apart_blocks_do_not_pair_up() {
        let old = page(vec![block(1, 700.0, 715.0, "あいう")]);
        let new = page(vec![block(1, 40.0, 55.0, "かきく")]);
        let diff = diff_page(&old, &new, false, &DiffOptions::default());

        let kinds: Vec<_> = diff.blocks.iter().map(|b| b.kind).collect();
        assert!(
            kinds.contains(&ChangeKind::Removed) && kinds.contains(&ChangeKind::Added),
            "{kinds:?}"
        );
    }

    #[test]
    fn position_pairing_does_not_let_an_early_block_take_the_only_candidate() {
        let old = page(vec![
            block_at(1, 0.0, 10.0, "AAAA"),
            block_at(2, 4.0, 14.0, "BBBB"),
        ]);
        // N1 は O0 とだけ、N0 は O0/O1 の両方と十分に重なる。O0 が先に
        // N0 を取ると O1 が余るが、O0-N1 / O1-N0 なら両方を対応できる。
        let new = page(vec![
            block_at(1, -1.0, 7.0, "XXXX"),
            block_at(2, 1.0, 11.0, "YYYY"),
        ]);
        let diff = diff_page(&old, &new, false, &DiffOptions::default());

        assert_eq!(diff.blocks.len(), 2, "{:?}", diff.blocks);
        assert!(
            diff.blocks
                .iter()
                .all(|block| block.kind == ChangeKind::Changed),
            "{:?}",
            diff.blocks
        );
    }

    fn brute_force_position_pairs(
        old: &[BlockIr],
        new: &[BlockIr],
        threshold: f64,
        old_at: usize,
        used_new: &mut [bool],
    ) -> usize {
        if old_at == old.len() {
            return 0;
        }
        let mut best = brute_force_position_pairs(old, new, threshold, old_at + 1, used_new);
        for new_at in 0..new.len() {
            if used_new[new_at]
                || overlap_ratio(&old[old_at].bounds, &new[new_at].bounds) < threshold
            {
                continue;
            }
            used_new[new_at] = true;
            best =
                best.max(1 + brute_force_position_pairs(old, new, threshold, old_at + 1, used_new));
            used_new[new_at] = false;
        }
        best
    }

    proptest! {
        #[test]
        fn pdf文字サイズの変更は本文と領域が同じでも必ず報告される(
            base_hundredths in 2_001u16..10_000,
            delta_hundredths in 26u16..2_000,
            grows in any::<bool>(),
        ) {
            let mut old_block = block_at(1, 60.0, 300.0, "同じ本文");
            old_block.bounds.y0 = 300.0;
            old_block.bounds.y1 = 320.0;
            old_block.size = Some(f64::from(base_hundredths) / 100.0);
            let old_page = page(vec![old_block]);
            let mut new_page = old_page.clone();
            let delta = f64::from(delta_hundredths) / 100.0;
            let old_size = old_page.blocks[0].size.expect("generated size");
            new_page.blocks[0].size = Some(if grows {
                old_size + delta
            } else {
                old_size - delta
            });

            let diff = diff_documents(
                "old.pdf",
                "new.pdf",
                &DocumentIr { pages: vec![old_page] },
                &DocumentIr { pages: vec![new_page] },
                &DiffOptions::default(),
            );
            prop_assert_eq!(diff.summary.pages_modified, 1);
            prop_assert_eq!(diff.summary.blocks_format_changed, 1);
            prop_assert_eq!(diff.summary.blocks_changed, 0);
            prop_assert_eq!(diff.summary.blocks_resized, 0);
            prop_assert_eq!(diff.pages[0].blocks[0].kind, ChangeKind::Formatted);
            prop_assert!(diff.pages[0].blocks[0].format_changed);
        }

        #[test]
        fn pdf文字領域の平行移動をサイズ変更と誤判定しない(
            width in 20u16..1_000,
            height in 5u16..300,
            x_delta in 2u16..100,
            y_delta in 2u16..100,
            x_positive in any::<bool>(),
            y_positive in any::<bool>(),
        ) {
            let mut old_block = block_at(1, 60.0, 60.0 + f64::from(width), "同じ本文");
            old_block.bounds.y0 = 300.0;
            old_block.bounds.y1 = old_block.bounds.y0 + f64::from(height);
            let old_page = page(vec![old_block]);
            let mut new_page = old_page.clone();
            let dx = f64::from(x_delta) * if x_positive { 1.0 } else { -1.0 };
            let dy = f64::from(y_delta) * if y_positive { 1.0 } else { -1.0 };
            new_page.blocks[0].bounds.x0 += dx;
            new_page.blocks[0].bounds.x1 += dx;
            new_page.blocks[0].bounds.y0 += dy;
            new_page.blocks[0].bounds.y1 += dy;

            let diff = diff_documents(
                "old.pdf",
                "new.pdf",
                &DocumentIr { pages: vec![old_page] },
                &DocumentIr { pages: vec![new_page] },
                &DiffOptions::default(),
            );
            prop_assert_eq!(diff.summary.pages_modified, 1);
            prop_assert_eq!(diff.summary.blocks_moved, 1);
            prop_assert_eq!(diff.summary.blocks_resized, 0);
            prop_assert_eq!(diff.pages[0].blocks[0].kind, ChangeKind::Moved);
            prop_assert!(diff.pages[0].blocks[0].moved);
            prop_assert!(!diff.pages[0].blocks[0].resized);
        }

        #[test]
        fn pdf文字領域のサイズ変更は必ず報告される(
            width in 20u16..1_000,
            height in 5u16..300,
            width_delta in 2u16..100,
            height_delta in 2u16..100,
        ) {
            let mut old_block = block_at(1, 60.0, 60.0 + f64::from(width), "同じ本文");
            old_block.bounds.y1 = old_block.bounds.y0 + f64::from(height);
            let old_page = page(vec![old_block]);
            let mut new_page = old_page.clone();
            new_page.blocks[0].bounds.x1 += f64::from(width_delta);
            new_page.blocks[0].bounds.y1 += f64::from(height_delta);

            let diff = diff_documents(
                "old.pdf",
                "new.pdf",
                &DocumentIr { pages: vec![old_page] },
                &DocumentIr { pages: vec![new_page] },
                &DiffOptions::default(),
            );
            prop_assert_eq!(diff.summary.pages_modified, 1);
            prop_assert_eq!(diff.summary.blocks_resized, 1);
            prop_assert_eq!(diff.summary.blocks_changed, 0);
            prop_assert_eq!(diff.summary.blocks_moved, 0);
            prop_assert_eq!(diff.pages[0].blocks[0].kind, ChangeKind::Resized);
            prop_assert!(diff.pages[0].blocks[0].resized);
            prop_assert!(!diff.pages[0].blocks[0].moved);
        }

        #[test]
        fn pdfページ形状の変更は本文が同じでも必ず報告される(
            width in 200u16..1_200,
            height in 200u16..1_200,
            delta in 2u16..100,
            quarter_turn in 0u8..4,
        ) {
            let mut old_page = page(vec![block(1, 100.0, 115.0, "同じ本文")]);
            old_page.width = f64::from(width);
            old_page.height = f64::from(height);
            old_page.rotation = u64::from(quarter_turn) * 90;
            let mut new_page = old_page.clone();
            new_page.width += f64::from(delta);
            new_page.height += f64::from(delta);
            new_page.rotation = u64::from((quarter_turn + 1) % 4) * 90;

            let diff = diff_documents(
                "old.pdf",
                "new.pdf",
                &DocumentIr { pages: vec![old_page] },
                &DocumentIr { pages: vec![new_page] },
                &DiffOptions::default(),
            );
            prop_assert_eq!(diff.summary.pages_modified, 1);
            prop_assert_eq!(diff.summary.pages_geometry_changed, 1);
            prop_assert_eq!(diff.summary.blocks_changed, 0);
            prop_assert!(diff.pages[0].geometry_changed);
        }

        #[test]
        fn position_pairing_finds_the_maximum_number_of_pairs(
            old_ranges in prop::collection::vec((0u8..30, 1u8..16), 0..7),
            new_ranges in prop::collection::vec((0u8..30, 1u8..16), 0..7),
        ) {
            let old = page(old_ranges
                .iter()
                .enumerate()
                .map(|(index, (x, width))| {
                    block_at(index + 1, f64::from(*x), f64::from(*x + *width), "old")
                })
                .collect());
            let new = page(new_ranges
                .iter()
                .enumerate()
                .map(|(index, (x, width))| {
                    block_at(index + 1, f64::from(*x), f64::from(*x + *width), "new")
                })
                .collect());
            let options = DiffOptions::default();
            let mut matched = Vec::new();
            let mut leftover_old: Vec<_> = (0..old.blocks.len()).collect();
            let mut leftover_new: Vec<_> = (0..new.blocks.len()).collect();
            pair_leftovers_by_position(
                &old,
                &new,
                &mut matched,
                &mut leftover_old,
                &mut leftover_new,
                &options,
            );
            let expected = brute_force_position_pairs(
                &old.blocks,
                &new.blocks,
                options.position_overlap,
                0,
                &mut vec![false; new.blocks.len()],
            );
            prop_assert_eq!(matched.len(), expected);

            let mut reverse_matched = Vec::new();
            let mut reverse_old: Vec<_> = (0..new.blocks.len()).collect();
            let mut reverse_new: Vec<_> = (0..old.blocks.len()).collect();
            pair_leftovers_by_position(
                &new,
                &old,
                &mut reverse_matched,
                &mut reverse_old,
                &mut reverse_new,
                &options,
            );
            prop_assert_eq!(reverse_matched.len(), expected);
        }

        #[test]
        fn unicode_segments_reconstruct_both_sides_at_grapheme_boundaries(
            old in unicode_text(),
            new in unicode_text(),
        ) {
            let segments = segment_text(&old, &new);
            let old_boundaries = boundaries(&old);
            let new_boundaries = boundaries(&new);
            let mut old_at = 0;
            let mut new_at = 0;

            for segment in &segments {
                prop_assert!(!segment.text.is_empty());
                match segment.kind {
                    SegmentKind::Equal => {
                        prop_assert!(old[old_at..].starts_with(&segment.text));
                        prop_assert!(new[new_at..].starts_with(&segment.text));
                        old_at += segment.text.len();
                        new_at += segment.text.len();
                        prop_assert!(old_boundaries.contains(&old_at));
                        prop_assert!(new_boundaries.contains(&new_at));
                    }
                    SegmentKind::Delete => {
                        prop_assert!(old[old_at..].starts_with(&segment.text));
                        old_at += segment.text.len();
                        prop_assert!(old_boundaries.contains(&old_at));
                    }
                    SegmentKind::Insert => {
                        prop_assert!(new[new_at..].starts_with(&segment.text));
                        new_at += segment.text.len();
                        prop_assert!(new_boundaries.contains(&new_at));
                    }
                }
            }
            prop_assert_eq!(old_at, old.len());
            prop_assert_eq!(new_at, new.len());
            for pair in segments.windows(2) {
                prop_assert_ne!(pair[0].kind, pair[1].kind);
            }
        }
    }
}
