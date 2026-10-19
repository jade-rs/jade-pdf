//! ページ → テキストブロック → 文字 の 3 段で比較する。
//!
//! ページを先に対応付けるので、途中に 1 ページ挟まっても以降が全部差分に
//! なることはない。ページ内はブロックを対応付けてから中身を比べる。

use std::cell::OnceCell;

use diff_align::{pair_with_anchors, Pairing, Step};
use diff_support::{trimmed_lcs_diff, TextProfile};
use serde::Serialize;
use unicode_segmentation::UnicodeSegmentation;

use crate::ir::{BlockIr, Bounds, DocumentIr, PageIr};

/// 出力 JSON の契約バージョン。
pub const DIFF_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// ページ同士を同じページとみなす最小スコア。
    pub page_match_score: f64,
    /// ブロック同士を同じブロックとみなす最小スコア。
    pub block_match_score: f64,
    /// 削除 + 追加を「変更」に畳む最小スコア。
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
    /// 上限を設ける (超えた区間は出現順で対応付ける)。
    pub max_pairwise: usize,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Added,
    Removed,
    Modified,
    Moved,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub kind: SegmentKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
    pub rotation: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageDiff {
    /// 1 始まりのページ番号。
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
    pub status: Status,
    pub match_confidence: Option<f64>,
    pub old_size: Option<PageSize>,
    pub new_size: Option<PageSize>,
    /// 用紙の幅・高さ・表示回転が変わったか。
    pub geometry_changed: bool,
    pub blocks: Vec<BlockChange>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DiffSummary {
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
#[derive(Debug, Clone, Default, Serialize)]
pub struct Diagnostics {
    pub old_pages: usize,
    pub new_pages: usize,
    /// テキストが 1 つも取り出せなかったページ数。
    pub old_pages_without_text: usize,
    pub new_pages_without_text: usize,
}

impl Diagnostics {
    /// 片側でもテキストが全く取れていないか。
    pub fn text_missing(&self) -> bool {
        (self.old_pages > 0 && self.old_pages_without_text == self.old_pages)
            || (self.new_pages > 0 && self.new_pages_without_text == self.new_pages)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentDiff {
    pub schema_version: u32,
    /// 座標系の明示。PDF ユーザ空間 (原点は左下、単位 pt)。
    pub coordinate_system: &'static str,
    pub old_file: String,
    pub new_file: String,
    pub summary: DiffSummary,
    pub diagnostics: Diagnostics,
    pub pages: Vec<PageDiff>,
}

impl DocumentDiff {
    pub fn has_changes(&self) -> bool {
        self.pages
            .iter()
            .any(|page| page.status != Status::Unchanged)
    }
}

/// 2 つの PDF を比較する。
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
    let old_text: Vec<String> = old.pages.iter().map(|page| page.body_text()).collect();
    let new_text: Vec<String> = new.pages.iter().map(|page| page.body_text()).collect();
    // 錨の無い区間は総当たりになる。ページ本文の bigram は組ごとに作り直さず
    // 1 回だけ作る。錨になったページは比べないので、比べるときに初めて作る。
    let old_profiles: Vec<OnceCell<TextProfile>> =
        old_text.iter().map(|_| OnceCell::new()).collect();
    let new_profiles: Vec<OnceCell<TextProfile>> =
        new_text.iter().map(|_| OnceCell::new()).collect();
    let threshold = options.page_match_score;
    let pairings = pair_with_anchors(
        &old_text,
        &new_text,
        threshold,
        options.max_pairwise,
        |i, j| {
            let old_profile = old_profiles[i].get_or_init(|| TextProfile::new(&old_text[i]));
            let new_profile = new_profiles[j].get_or_init(|| TextProfile::new(&new_text[j]));
            old_profile.similarity_at_least(new_profile, threshold)
        },
    );

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
        old_pages: old.pages.len(),
        new_pages: new.pages.len(),
        old_pages_without_text: old.pages.iter().filter(|p| p.blocks.is_empty()).count(),
        new_pages_without_text: new.pages.iter().filter(|p| p.blocks.is_empty()).count(),
    };

    DocumentDiff {
        schema_version: DIFF_SCHEMA_VERSION,
        coordinate_system: "pdf_user_space",
        old_file: old_file.to_string(),
        new_file: new_file.to_string(),
        summary,
        diagnostics,
        pages,
    }
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
        width: page.width,
        height: page.height,
        rotation: page.rotation,
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
    old_page.is_furniture(old_block)
        && new_page.is_furniture(new_block)
        && old_block.text != new_block.text
        && digit_skeleton(&old_block.text) == digit_skeleton(&new_block.text)
}

fn diff_page(old: &PageIr, new: &PageIr, moved: bool, options: &DiffOptions) -> PageDiff {
    let old_text: Vec<&str> = old.blocks.iter().map(|block| block.text.as_str()).collect();
    let new_text: Vec<&str> = new.blocks.iter().map(|block| block.text.as_str()).collect();
    let old_profiles: Vec<TextProfile> =
        old_text.iter().map(|text| TextProfile::new(text)).collect();
    let new_profiles: Vec<TextProfile> =
        new_text.iter().map(|text| TextProfile::new(text)).collect();
    let threshold = options.block_match_score;
    let pairings = pair_with_anchors(
        &old_text,
        &new_text,
        threshold,
        options.max_pairwise,
        |i, j| {
            block_score(
                &old.blocks[i],
                &new.blocks[j],
                &old_profiles[i],
                &new_profiles[j],
                threshold,
            )
        },
    );

    let mut matched: Vec<(usize, usize)> = Vec::new();
    let mut leftover_old: Vec<usize> = Vec::new();
    let mut leftover_new: Vec<usize> = Vec::new();
    for pairing in &pairings {
        match *pairing {
            Pairing::Matched { old: o, new: n, .. } => matched.push((o, n)),
            Pairing::Removed(o) => leftover_old.push(o),
            Pairing::Added(n) => leftover_new.push(n),
        }
    }
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
        change.minor = is_minor_change(old, new, &old.blocks[o], &new.blocks[n]);
        blocks.push(change);
    }
    for o in leftover_old {
        blocks.push(one_sided_block(&old.blocks[o], true));
    }
    for n in leftover_new {
        blocks.push(one_sided_block(&new.blocks[n], false));
    }
    blocks.sort_by_key(|block| {
        (
            block.new_index.unwrap_or(usize::MAX),
            block.old_index.unwrap_or(0),
        )
    });

    let geometry_changed = page_geometry_changed(old, new, options.position_tolerance);
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
        match_confidence: Some(
            TextProfile::new(&old.text()).similarity(&TextProfile::new(&new.text())),
        ),
        old_size: Some(page_size(old)),
        new_size: Some(page_size(new)),
        geometry_changed,
        blocks,
    }
}

fn page_geometry_changed(old: &PageIr, new: &PageIr, tolerance: f64) -> bool {
    numeric_changed(old.width, new.width, tolerance)
        || numeric_changed(old.height, new.height, tolerance)
        || old.rotation % 360 != new.rotation % 360
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
    let candidates: Vec<Vec<usize>> = leftover_old
        .iter()
        .map(|o| {
            let source = &old.blocks[*o].bounds;
            let mut choices: Vec<(usize, f64)> = leftover_new
                .iter()
                .enumerate()
                .map(|(position, n)| (position, overlap_ratio(source, &new.blocks[*n].bounds)))
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
fn overlap_ratio(a: &Bounds, b: &Bounds) -> f64 {
    let width = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0);
    let height = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    let overlap = width * height;
    let smaller = (a.width() * a.height()).min(b.width() * b.height());
    if smaller <= 0.0 {
        return 0.0;
    }
    overlap / smaller
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

fn accumulate(summary: &mut DiffSummary, page: &PageDiff) {
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

    fn page(blocks: Vec<BlockIr>) -> PageIr {
        PageIr {
            index: 1,
            width: 595.0,
            height: 842.0,
            rotation: 0,
            blocks,
        }
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
        let old = page(vec![block(1, 40.0, 55.0, "- 9 -")]);
        let mut new = page(vec![block(1, 40.0, 55.0, "- 10 -")]);
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
