//! 固定の行閾値から離れた小文字行を、検証済みの隣接本文へまとめて戻す。
use super::{ParsedTextElement, inline_baseline, native_baseline};

/// 密集した候補行は曖昧と扱い、glyphごとの全行走査を避ける。
const MAX_CANDIDATE_ROWS: usize = 16;

/// 元行の基準座標と、最大fontに近い通常文字の水平索引。
struct Row<'a> {
    /// 初期の4pt行分割に使った降順の座標。
    seed_y: f32,
    /// 行の最大font。無効な値を含む場合は0として候補から外す。
    size: f32,
    /// 水平方向に整列した通常文字。未知baselineも局所照合の拒否に使う。
    anchors: Vec<&'a ParsedTextElement>,
    /// 長い先行boxを細い後続glyphが隠しても交差を検出する累積右端。
    right_prefix: Vec<f32>,
}

impl<'a> Row<'a> {
    /// 全glyphを複製せず、通常文字だけの索引を一度構築する。
    fn new(texts: &[&'a ParsedTextElement]) -> Self {
        let valid = texts.iter().all(|t| {
            t.font_size.is_finite()
                && t.font_size > 0.
                && t.bounds.x0.is_finite()
                && t.bounds.x1.is_finite()
                && t.bounds.x1 >= t.bounds.x0
        });
        let size = if valid {
            texts.iter().map(|t| t.font_size).fold(0., f32::max)
        } else {
            0.
        };
        let mut anchors = texts
            .iter()
            .copied()
            .filter(|t| size > 0. && t.font_size >= size * 0.95)
            .collect::<Vec<_>>();
        anchors.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
        let mut right = f32::NEG_INFINITY;
        let right_prefix = anchors
            .iter()
            .map(|text| {
                right = right.max(text.bounds.x1);
                right
            })
            .collect();
        Self {
            seed_y: native_baseline::line_y(texts[0]),
            size,
            anchors,
            right_prefix,
        }
    }

    /// 本文と重なる小さい別行を拒否し、近接する通常文字の基線だけを照合する。
    fn accepts(&self, text: &ParsedTextElement) -> bool {
        let Some(y) = text.baseline_y.filter(|y| y.is_finite()) else {
            return false;
        };
        if self.size <= 0.
            || !(0.5..=0.85).contains(&(text.font_size / self.size))
            || text.bounds.x1 <= text.bounds.x0
        {
            return false;
        }
        let next = self
            .anchors
            .partition_point(|t| t.bounds.x0 < text.bounds.x0);
        let left = next.checked_sub(1).map(|i| self.anchors[i]);
        let right = self.anchors.get(next).copied();
        if next
            .checked_sub(1)
            .is_some_and(|i| self.right_prefix[i] > text.bounds.x0 + 0.01)
            || right.is_some_and(|t| t.bounds.x0 < text.bounds.x1 - 0.01)
        {
            return false;
        }
        let left = left.filter(|t| text.bounds.x0 - t.bounds.x1 <= self.size * 2.);
        let right = right.filter(|t| t.bounds.x0 - text.bounds.x1 <= self.size * 2.);
        let Some((_, baseline)) = inline_baseline::neighbouring_reference(self.size, left, right)
        else {
            return false;
        };
        let delta = (y - baseline).abs();
        (0.25..=self.size * 0.6).contains(&delta) && delta <= 15840.
    }
}

/// 小文字だけの行を一意な本文行へ全体として移し、曖昧・重複・入れ子の移動を避ける。
pub(super) fn recover<'a>(rows: &mut Vec<Vec<&'a ParsedTextElement>>) {
    if rows.is_empty() || rows.iter().any(Vec::is_empty) {
        return;
    }
    let metadata = rows.iter().map(|r| Row::new(r)).collect::<Vec<_>>();
    if metadata.iter().any(|r| !r.seed_y.is_finite())
        || metadata.windows(2).any(|r| r[0].seed_y < r[1].seed_y)
    {
        return;
    }
    let mut moves = vec![None; rows.len()];
    for (index, texts) in rows.iter().enumerate() {
        let source = &metadata[index];
        if source.size <= 0. || texts.iter().any(|t| t.font_size < source.size * 0.95) {
            continue;
        }
        let radius = 4. + (source.size * 1.2).min(15840.);
        let start = metadata.partition_point(|r| r.seed_y > source.seed_y + radius);
        let end = metadata.partition_point(|r| r.seed_y >= source.seed_y - radius);
        if end - start > MAX_CANDIDATE_ROWS {
            continue;
        }
        let mut target = None;
        for (candidate, row) in metadata.iter().enumerate().take(end).skip(start) {
            if candidate != index && texts.iter().all(|t| row.accepts(t)) {
                if target.is_some() {
                    target = None;
                    break;
                }
                target = Some(candidate);
            }
        }
        moves[index] = target;
    }
    let mut incoming = vec![false; rows.len()];
    for target in moves.iter().flatten() {
        incoming[*target] = true;
    }
    for (source, target) in moves.iter().enumerate() {
        if let Some(target) = target
            && moves[*target].is_none()
            && !incoming[source]
        {
            let texts = std::mem::take(&mut rows[source]);
            rows[*target].extend(texts);
        }
    }
    rows.retain(|row| !row.is_empty());
}
