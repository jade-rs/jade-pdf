//! 縦書きのページ（官報の本紙など）を、読み順の通りの横組みの座標に回して組み、DOCX では
//! 縦書き（`w:textDirection w:val="tbRl"`）の section にする。
//!
//! 縦書きのページは、上下に重ねた段の中に、右から左へ縦の行を並べる。横組みのまま行を
//! 組むと、縦の行の字が別々の行に散らばり読めない。lawrenceanum の kanpo-parser が段に分けて
//! 右の行から読むのと同じ読み順にするため、ページを時計回りに 90 度回した座標で組む。
//! 回した座標では縦の行が上から下へ並ぶ横の行に、上下の段が左から右へ並ぶ段組みになり、
//! 横組みの行・段組み・表の推定をそのまま使える。Word の縦書きの section は回した座標を
//! 縦に組み直す（行は上から下へ、行の並びは右から左へ、段組みは上から下の段になる）ので、
//! 原本と同じ向きで表示される。
//!
//! 縦書きのページは [`ParsedPage::rotation`] に [`MARK`] を足して表す。回した座標の表示寸法は
//! 回転角の 90 度の差で入れ替わるので、`rotation.rem_euclid(180)` による表示寸法の計算は
//! 回した座標の寸法を返す。

use super::*;
use docx_rs::{Pic, RelativeFromHType, RelativeFromVType};

/// 縦書きのページの印（回転角に足す。360 の倍数なので回転角としての意味は変えない）。
/// 印の後ろに段の数 × 360 を足して、段の数も持たせる。
const MARK: i32 = 3600;
/// 縦書きのページとみなす縦の行の字数の下限。
const MIN_VERTICAL_CHARS: usize = 40;
/// 縦の行・横の行とみなす文字片の縦横比。
const ASPECT: f32 = 1.5;
/// 同じ行の文字片をそのままつなぐ間の上限（字の大きさ比）。これ以上の間は全角の空白で埋める。
const JOIN_GAP_EM: f32 = 0.75;
/// 段と段の間とみなす、字の無い帯の幅の下限（pt）。
const MIN_TIER_GAP: f32 = 4.0;
/// 段を区切る罫線（ページの幅いっぱいの横の線、回した座標では縦の線）とみなす長さの割合と太さ。
const SEPARATOR_SPAN: f32 = 0.8;
const MAX_SEPARATOR_THICKNESS: f32 = 6.0;

/// 縦書きとして回したページか。
pub(crate) fn is_vertical(page: &ParsedPage) -> bool {
    page.rotation >= MARK
}

/// 縦書きのページの段の数（縦書きでなければ 1）。
fn tier_count(page: &ParsedPage) -> usize {
    if is_vertical(page) {
        usize::try_from((page.rotation - MARK) / 360)
            .unwrap_or(1)
            .max(1)
    } else {
        1
    }
}

/// 回した座標で 1 段が占める高さ（原本の表示の幅）。
fn band_height(page: &ParsedPage) -> f32 {
    page.height / tier_count(page) as f32
}

/// 回した座標の高さ `y` が属する段の番号（上の段・原本の上の段から 0）。
pub(crate) fn tier_of(page: &ParsedPage, y: f32) -> usize {
    let count = tier_count(page);
    let index = ((page.height - y) / band_height(page)).floor();
    if index.is_finite() && index > 0.0 {
        (index as usize).min(count - 1)
    } else {
        0
    }
}

/// 段 `tier` の上端を、最初の段の上端 `top` から求める（段は 1 段の高さずつ下に積む）。
pub(crate) fn tier_top(page: &ParsedPage, top: f32, tier: usize) -> f32 {
    top - band_height(page) * tier as f32
}

/// 回した座標の高さ `y` を、最初の段の高さに戻す（段ごとの上端をそろえて比べる）。
pub(crate) fn in_first_tier(page: &ParsedPage, y: f32) -> f32 {
    y + band_height(page) * tier_of(page, y) as f32
}

/// 文字片の字数（空白を除く）。
fn char_count(text: &ParsedTextElement) -> usize {
    text.text.chars().filter(|ch| !ch.is_whitespace()).count()
}

/// ページの本文の大半が縦の行か（2 字以上の文字片の縦横比で数える）。
fn page_is_vertical(parsed: &ParsedDocument, page: usize) -> bool {
    let (mut vertical, mut horizontal) = (0usize, 0usize);
    for text in parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !is_rule_font(text))
    {
        let count = char_count(text);
        if count < 2 {
            continue;
        }
        let bounds = normalized_bounds(text.bounds);
        let (width, height) = (bounds.x1 - bounds.x0, bounds.y1 - bounds.y0);
        if height >= ASPECT * width {
            vertical += count;
        } else if width >= ASPECT * height {
            horizontal += count;
        }
    }
    vertical >= MIN_VERTICAL_CHARS && vertical > 2 * horizontal
}

/// 罫線用フォント（MFHSLine など）の文字片か。
fn is_rule_font(text: &ParsedTextElement) -> bool {
    text.font_name
        .as_deref()
        .is_some_and(|font| font.contains("Line"))
}

/// 縦書きのページで、罫線用フォントの字で引いたページの幅いっぱいの点線（段の区切り）を、
/// 字として除かれる前に破線の罫線にする。段の区切りを見つけるのに使う。
pub(super) fn keep_separator_glyphs(parsed: &mut ParsedDocument) {
    let pages = parsed
        .pages
        .iter()
        .filter(|page| !is_vertical(page) && page_is_vertical(parsed, page.page_number))
        .map(|page| (page.page_number, parsed_page_display_size_points(page).0))
        .collect::<Vec<_>>();
    for (page, width) in pages {
        let rules = parsed
            .text_elements
            .iter()
            .filter(|text| text.page == page && is_rule_font(text))
            .map(|text| normalized_bounds(text.bounds))
            .filter(|bounds| {
                bounds.y1 - bounds.y0 <= MAX_SEPARATOR_THICKNESS
                    && bounds.x1 - bounds.x0 >= SEPARATOR_SPAN * width
            })
            .map(|bounds| {
                let y = (bounds.y0 + bounds.y1) / 2.0;
                ParsedGraphicElement {
                    page,
                    bounds: ParsedBounds {
                        x0: bounds.x0,
                        y0: y,
                        x1: bounds.x1,
                        y1: y,
                    },
                    stroke_width: 0.5,
                    dashed: true,
                }
            })
            .collect::<Vec<_>>();
        parsed.graphics.extend(rules);
    }
}

/// 表示座標（左下原点）の矩形を時計回りに 90 度回す（`display_height` は回す前の表示の高さ）。
fn rotate_bounds(bounds: ParsedBounds, display_height: f32) -> ParsedBounds {
    let bounds = normalized_bounds(bounds);
    ParsedBounds {
        x0: display_height - bounds.y1,
        y0: bounds.x0,
        x1: display_height - bounds.y0,
        y1: bounds.x1,
    }
}

/// 回した座標の、段（原本の上下に重ねた段。回した座標では左右に並ぶ）の左右の範囲。
///
/// 文字・罫線・塗り・画像が覆う横の範囲を合わせ、[`MIN_TIER_GAP`] 以上空いた所で分ける。
/// 文字の無い範囲（罫線だけなど）は段にしない。ページの幅の大半にわたる罫線・塗り（囲み）と、
/// 段の間の罫線のような細い線は段をつなげてしまうので数えない。
fn detect_tiers(parsed: &ParsedDocument, page: usize, width: f32, height: f32) -> Vec<(f32, f32)> {
    let mut spans = Vec::new();
    for text in parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && char_count(text) > 0)
    {
        spans.push((text.bounds.x0, text.bounds.x1, true));
    }
    let wide = 0.6 * width;
    for bounds in parsed
        .graphics
        .iter()
        .filter(|graphic| graphic.page == page)
        .map(|graphic| graphic.bounds)
        .chain(
            parsed
                .fills
                .iter()
                .filter(|fill| fill.page == page)
                .map(|fill| fill.bounds),
        )
        .chain(
            parsed
                .image_elements
                .iter()
                .filter(|image| image.page == page)
                .map(|image| image.bounds),
        )
    {
        // 段の間の罫線（回した座標では幅の無い縦の線）は段を区切る線なので、段をつながない。
        let span = bounds.x1 - bounds.x0;
        if (MIN_TIER_GAP..wide).contains(&span) {
            spans.push((bounds.x0, bounds.x1, false));
        }
    }
    spans.retain(|(x0, x1, _)| x0.is_finite() && x1.is_finite());
    let mut separators = separators(parsed, page, height);
    if !separators.is_empty() {
        // 段を区切る罫線があるページでは、罫線で区切られた帯ごとに 1 つの段にする
        // （目次の段の下端のページ番号も同じ段。余白の印が罫線をまたいでも段はつながない）。
        separators.sort_by(f32::total_cmp);
        let mut cells: Vec<Option<(f32, f32)>> = vec![None; separators.len() + 1];
        for (x0, x1, has_text) in spans {
            let center = (x0 + x1) / 2.0;
            let cell = separators.partition_point(|&rule| rule < center);
            let low = if cell == 0 {
                f32::MIN
            } else {
                separators[cell - 1]
            };
            let high = separators.get(cell).copied().unwrap_or(f32::MAX);
            let (x0, x1) = (x0.max(low), x1.min(high));
            let slot = &mut cells[cell];
            *slot = match (*slot, has_text) {
                (Some((a, b)), _) => Some((a.min(x0), b.max(x1))),
                (None, true) => Some((x0, x1)),
                (None, false) => None,
            };
        }
        return cells.into_iter().flatten().collect();
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32, bool)> = Vec::new();
    for (x0, x1, has_text) in spans {
        match merged.last_mut() {
            Some(last) if x0 <= last.1 + MIN_TIER_GAP => {
                last.1 = last.1.max(x1);
                last.2 |= has_text;
            }
            _ => merged.push((x0, x1, has_text)),
        }
    }
    merged
        .into_iter()
        .filter(|(_, _, has_text)| *has_text)
        .map(|(x0, x1, _)| (x0, x1))
        .collect()
}

/// 段を区切る罫線（ページの幅いっぱいの細い横線。罫線用フォントの字で引いた線も含む）の、
/// 回した座標での位置。
fn separators(parsed: &ParsedDocument, page: usize, height: f32) -> Vec<f32> {
    let is_separator = |bounds: ParsedBounds| {
        bounds.x1 - bounds.x0 <= MAX_SEPARATOR_THICKNESS
            && bounds.y1 - bounds.y0 >= SEPARATOR_SPAN * height
    };
    parsed
        .graphics
        .iter()
        .filter(|graphic| graphic.page == page)
        .map(|graphic| graphic.bounds)
        .chain(
            parsed
                .text_elements
                .iter()
                .filter(|text| text.page == page)
                .map(|text| text.bounds),
        )
        .filter(|bounds| is_separator(*bounds))
        .map(|bounds| (bounds.x0 + bounds.x1) / 2.0)
        .collect()
}

/// 括弧・約物だけの文字片か。
fn is_marks(text: &str) -> bool {
    let body = text.trim();
    !body.is_empty()
        && body.chars().all(|ch| {
            matches!(
                ch,
                '（' | '）' | '(' | ')' | '「' | '」' | '〔' | '〕' | '［' | '］' | '、' | '。'
            )
        })
}

/// 縦書きのページの中の横組みの文字列（柱・題字の下の発行者など）を、1 行の縦の行にする。
///
/// 回した座標では横組みの文字列は縦に長い文字片になり、行の組み立てでは字の並ぶ長さ分の
/// 送りを空けてしまう。横組みの文字列に接する括弧を同じ文字列に入れ、字の並ぶ長さを
/// 行の長さに、元の文字列の中央を行の位置にする（Word の縦書きでは縦に並ぶ）。
fn collapse_horizontal_runs(parsed: &mut ParsedDocument, page: usize, tiers: &[(f32, f32)]) {
    let is_horizontal = |text: &ParsedTextElement| {
        let height = text.bounds.y1 - text.bounds.y0;
        let width = text.bounds.x1 - text.bounds.x0;
        char_count(text) >= 2 && height >= ASPECT * width
    };
    let mut remove = BTreeSet::new();
    let mut collapsed = Vec::new();
    for index in 0..parsed.text_elements.len() {
        if parsed.text_elements[index].page != page
            || remove.contains(&index)
            || !is_horizontal(&parsed.text_elements[index])
        {
            continue;
        }
        // 前後（回した座標の下・上）に接する括弧・横組みの文字片を取り込む。
        loop {
            let base = parsed.text_elements[index].clone();
            let size = base.font_size.max(1.0);
            let neighbor = (0..parsed.text_elements.len()).find(|&other| {
                let candidate = &parsed.text_elements[other];
                let overlap = candidate.bounds.x1.min(base.bounds.x1)
                    - candidate.bounds.x0.max(base.bounds.x0);
                other != index
                    && candidate.page == page
                    && !remove.contains(&other)
                    && (is_marks(&candidate.text) || is_horizontal(candidate))
                    && overlap >= 0.5 * (candidate.bounds.x1 - candidate.bounds.x0).min(size)
                    && (candidate.bounds.y0 - base.bounds.y1)
                        .max(base.bounds.y0 - candidate.bounds.y1)
                        < 0.5 * size
            });
            let Some(other) = neighbor else {
                break;
            };
            let candidate = parsed.text_elements[other].clone();
            let base = &mut parsed.text_elements[index];
            // 原本の左から右（回した座標の下から上）の順につなぐ。
            base.text = if candidate.bounds.y0 < base.bounds.y0 {
                format!("{}{}", candidate.text.trim(), base.text.trim())
            } else {
                format!("{}{}", base.text.trim(), candidate.text.trim())
            };
            base.bounds = ParsedBounds {
                x0: base.bounds.x0.min(candidate.bounds.x0),
                y0: base.bounds.y0.min(candidate.bounds.y0),
                x1: base.bounds.x1.max(candidate.bounds.x1),
                y1: base.bounds.y1.max(candidate.bounds.y1),
            };
            remove.insert(other);
        }
        let text = &mut parsed.text_elements[index];
        let size = text.font_size.max(1.0);
        let center = (text.bounds.y0 + text.bounds.y1) / 2.0;
        let tier = nearest_tier(tiers, text.bounds.x0, text.bounds.x1);
        let length = set_width(text);
        text.bounds.y0 = center - size / 2.0;
        text.bounds.y1 = center + size / 2.0;
        collapsed.push((index, tier, length));
    }
    // 原本で上下に重ねた横組みの行（題字の下の発行者の 2 行など）は、回した座標では同じ
    // 行に並ぶ。原本の上の行から順に続けて組み、段の終わりを越えるなら全体を段の中へ戻す
    // （越えると行が折り返し、その後の行がすべて送られる）。
    collapsed.sort_by(|a, b| {
        parsed.text_elements[a.0]
            .bounds
            .x0
            .total_cmp(&parsed.text_elements[b.0].bounds.x0)
    });
    let mut placed = vec![false; collapsed.len()];
    for first in 0..collapsed.len() {
        if placed[first] {
            continue;
        }
        let (index, tier, _) = collapsed[first];
        let row = parsed.text_elements[index].bounds;
        let group = (first..collapsed.len())
            .filter(|&other| {
                let bounds = parsed.text_elements[collapsed[other].0].bounds;
                let center = (bounds.y0 + bounds.y1) / 2.0;
                !placed[other] && collapsed[other].1 == tier && center >= row.y0 && center <= row.y1
            })
            .collect::<Vec<_>>();
        let (tier_start, tier_end) = tiers
            .get(tier)
            .copied()
            .unwrap_or((f32::NEG_INFINITY, f32::INFINITY));
        let total = group.iter().map(|&member| collapsed[member].2).sum::<f32>();
        let mut cursor = row.x0.min(tier_end - total).max(tier_start);
        for member in group {
            placed[member] = true;
            let (index, _, length) = collapsed[member];
            let bounds = &mut parsed.text_elements[index].bounds;
            bounds.x0 = cursor;
            bounds.x1 = cursor + length;
            cursor += length;
        }
    }
    let mut index = 0;
    parsed.text_elements.retain(|_| {
        let keep = !remove.contains(&index);
        index += 1;
        keep
    });
}

/// 字送りを測る範囲（回した座標の行の位置の上下、pt）。
const PITCH_NEIGHBORHOOD_PT: f32 = 30.0;

/// 行ごとの字送り（縦組みは表のマスなどで字を詰めて組むことがあり、字の大きさより狭い）。
pub(crate) struct Pitches(Vec<(ParsedBounds, f32, f32)>);

impl Pitches {
    /// 両端に約物の無い全角の字の行から字送りを測る（範囲・字の大きさ・字送り）。
    fn measure(parsed: &ParsedDocument, page: usize) -> Self {
        let mut samples = Vec::new();
        for text in parsed.text_elements.iter().filter(|text| text.page == page) {
            let body = text.text.trim();
            let count = body.chars().count();
            let size = text.font_size;
            if count < 4
                || !(size.is_finite() && size > 0.0)
                || !body.chars().all(|ch| {
                    // 仮名・漢字・全角英数（括弧・約物は外接矩形が字の枠より狭いので除く）
                    u32::from(ch) >= 0x3041
                        && !CLOSING_MARKS.contains(&ch)
                        && !matches!(
                            ch,
                            '（' | '「' | '『' | '〔' | '［' | '【' | '〈' | '《' | '｛' | '・'
                        )
                })
            {
                continue;
            }
            let extent = text.bounds.x1 - text.bounds.x0;
            // 字の外接矩形は字の枠より少し狭い。最後の字の分を除いて字の間の数で割る。
            let pitch = (extent - 0.95 * size) / (count - 1) as f32;
            if pitch.is_finite() && (0.8 * size..=1.05 * size).contains(&pitch) {
                samples.push((text.bounds, size, pitch));
            }
        }
        Self(samples)
    }

    /// 文字片 `text` の字送り。近くの行（同じ横の範囲で上下 [`PITCH_NEIGHBORHOOD_PT`] 以内）の
    /// 同じ大きさの字送りの中央値。測れなければ字の大きさ。
    fn near(&self, text: &ParsedTextElement) -> f32 {
        let size = text.font_size.max(1.0);
        let center = (text.bounds.y0 + text.bounds.y1) / 2.0;
        let mut values = self
            .0
            .iter()
            .filter(|(bounds, sample, _)| {
                (sample - size).abs() < 0.25
                    && ((bounds.y0 + bounds.y1) / 2.0 - center).abs() <= PITCH_NEIGHBORHOOD_PT
                    && bounds.x1 > text.bounds.x0
                    && bounds.x0 < text.bounds.x1
            })
            .map(|(_, _, pitch)| *pitch)
            .collect::<Vec<_>>();
        values.sort_by(f32::total_cmp);
        values.get(values.len() / 2).copied().unwrap_or(size)
    }
}

/// 回した座標の同じ行で、同じ書体・大きさのまま字間を空けずに続く文字片を 1 つにする。
///
/// 縦組みの約物（句点・括弧など）は字の枠の隅に小さく置かれ、外接矩形の間が空いて見える。
/// そのままでは文字片の間に空白を補ってしまう。縦組みの字は全角の送りで並び、空きは全角の
/// 空白の字で組むので、半角より狭い間は字の間の空きではない。
fn join_line_fragments(
    parsed: &mut ParsedDocument,
    page: usize,
    tiers: &[(f32, f32)],
    pitches: &Pitches,
) {
    for text in parsed
        .text_elements
        .iter_mut()
        .filter(|text| text.page == page)
    {
        let tier = tiers
            .get(nearest_tier(tiers, text.bounds.x0, text.bounds.x1))
            .copied()
            .unwrap_or((f32::NEG_INFINITY, f32::INFINITY));
        let pitch = pitches.near(text);
        restore_em_extent(text, tier, pitch);
    }
    let mut indices = (0..parsed.text_elements.len())
        .filter(|&index| parsed.text_elements[index].page == page)
        .collect::<Vec<_>>();
    indices.sort_by(|&a, &b| {
        let (a, b) = (
            &parsed.text_elements[a].bounds,
            &parsed.text_elements[b].bounds,
        );
        b.y1.total_cmp(&a.y1).then(a.x0.total_cmp(&b.x0))
    });
    let mut remove = BTreeSet::new();
    for position in 0..indices.len() {
        let current = indices[position];
        if remove.contains(&current) {
            continue;
        }
        loop {
            let base = &parsed.text_elements[current];
            let size = base.font_size.max(1.0);
            // 次に続く字（行の幅の中に中心がある字のうち、最も近いもの）。約物の外接矩形は
            // 字の枠の隅に寄るので、行の幅の中に収まるかで同じ行とみなす。
            let follows = |other: usize| {
                let candidate = &parsed.text_elements[other];
                let center = (candidate.bounds.y0 + candidate.bounds.y1) / 2.0;
                other != current
                    && !remove.contains(&other)
                    && (center - base.bounds.y0).min(base.bounds.y1 - center) >= -0.1 * size
                    && candidate.bounds.y1 - candidate.bounds.y0
                        <= base.bounds.y1 - base.bounds.y0 + 0.3 * size
                    // 約物の枠は前の字の枠に重なることがある
                    && candidate.bounds.x0 >= base.bounds.x1 - 0.5 * size
                    && candidate.bounds.x1 > base.bounds.x1
                    && nearest_tier(tiers, candidate.bounds.x0, candidate.bounds.x1)
                        == nearest_tier(tiers, base.bounds.x0, base.bounds.x1)
            };
            let next = indices
                .iter()
                .copied()
                .filter(|&other| follows(other))
                .min_by(|&a, &b| {
                    parsed.text_elements[a]
                        .bounds
                        .x0
                        .total_cmp(&parsed.text_elements[b].bounds.x0)
                })
                .filter(|&other| {
                    let candidate = &parsed.text_elements[other];
                    candidate.font_name == base.font_name
                        && (candidate.font_size - base.font_size).abs() <= 0.25
                        && candidate.fill_color == base.fill_color
                        && candidate.bounds.x0 - base.bounds.x1 < JOIN_GAP_EM * pitches.near(base)
                });
            let Some(next) = next else {
                break;
            };
            let (text, bounds) = {
                let next = &parsed.text_elements[next];
                (next.text.clone(), next.bounds)
            };
            let base = &mut parsed.text_elements[current];
            base.text = format!("{}{}", base.text.trim_end(), text.trim_start());
            base.bounds = ParsedBounds {
                x0: base.bounds.x0.min(bounds.x0),
                y0: base.bounds.y0.min(bounds.y0),
                x1: base.bounds.x1.max(bounds.x1),
                y1: base.bounds.y1.max(bounds.y1),
            };
            remove.insert(next);
        }
    }
    let mut index = 0;
    parsed.text_elements.retain(|_| {
        let keep = !remove.contains(&index);
        index += 1;
        keep
    });
}

/// 文字片の間 `(x0, x1)` の行の高さ `(y0, y1)` の範囲に、罫線・塗り・地紋があるか。
///
/// 表のマスの境目の罫線や、見出しの囲みの地紋の切れ目は、表のマスを分ける根拠になる。
/// その間を空白で埋めると文字がマスをまたぐので、何か描かれている間は埋めない。
fn graphics_between(
    parsed: &ParsedDocument,
    page: usize,
    (x0, x1): (f32, f32),
    (y0, y1): (f32, f32),
) -> bool {
    parsed
        .graphics
        .iter()
        .filter(|graphic| graphic.page == page)
        .map(|graphic| normalized_bounds(graphic.bounds))
        .chain(
            parsed
                .fills
                .iter()
                .filter(|fill| fill.page == page)
                .map(|fill| normalized_bounds(fill.bounds)),
        )
        .any(|bounds| bounds.x1 > x0 && bounds.x0 < x1 && bounds.y1 >= y0 && bounds.y0 <= y1)
}

/// 同じ行の離れた文字片（日付と署名、目次の項目とページ番号など）の間を、前の文字片に
/// 全角の空白を足して埋める。
///
/// 横組みの行の組み立ては、大きく離れた文字片を別の行にする。縦書きでは別の行は 1 行分の
/// 送りを増やし、段の行が収まらなくなる。縦組みは全角の送りで並ぶので、空きを全角の空白の
/// 字数にすれば、同じ行のまま原本の位置に置ける。罫線・地紋のある間（表のマスの境目）は
/// 埋めない。
fn pad_row_gaps(parsed: &mut ParsedDocument, page: usize, tiers: &[(f32, f32)], pitches: &Pitches) {
    let mut indices = (0..parsed.text_elements.len())
        .filter(|&index| {
            let text = &parsed.text_elements[index];
            text.page == page && char_count(text) > 0
        })
        .collect::<Vec<_>>();
    indices.sort_by(|&a, &b| {
        parsed.text_elements[a]
            .bounds
            .x0
            .total_cmp(&parsed.text_elements[b].bounds.x0)
    });
    let mut pads = Vec::new();
    // 組んだときの文字片の始まり。原本の半角の括弧・中黒も全角で組まれるので、行の前の
    // 文字片からの送りで追い、空白の数はそこから次の文字片の位置までで決める。
    let mut set_start = vec![None::<f32>; parsed.text_elements.len()];
    for &current in &indices {
        let base = &parsed.text_elements[current];
        let set_end = set_start[current].unwrap_or(base.bounds.x0) + set_width(base);
        let size = base.font_size.max(1.0);
        let tier = nearest_tier(tiers, base.bounds.x0, base.bounds.x1);
        // 同じ行で次に続く文字片（終わりが先にある文字片のうち、始まりが最も近いもの）。
        // 中黒などの字形の外接矩形は低いので、どちらかの中心がもう一方の行に入れば同じ行。
        let in_row = |inner: &ParsedBounds, outer: &ParsedBounds| {
            let center = (inner.y0 + inner.y1) / 2.0;
            (center - outer.y0).min(outer.y1 - center) >= -0.1 * size
        };
        let next = indices
            .iter()
            .copied()
            .filter(|&other| {
                let candidate = &parsed.text_elements[other];
                other != current
                    && candidate.bounds.x1 > base.bounds.x1
                    && candidate.bounds.x0 >= base.bounds.x0
                    && (in_row(&candidate.bounds, &base.bounds)
                        || in_row(&base.bounds, &candidate.bounds))
            })
            .min_by(|&a, &b| {
                parsed.text_elements[a]
                    .bounds
                    .x0
                    .total_cmp(&parsed.text_elements[b].bounds.x0)
            });
        let Some(next) = next else {
            continue;
        };
        let candidate = &parsed.text_elements[next];
        let gap = candidate.bounds.x0 - base.bounds.x1;
        // 中黒・句読点の外接矩形は字の枠の中央・隅にあり、空白の無い所でも半角ほど空く。
        let pitch = pitches.near(base);
        // 空白の数は組んだ位置から原本の位置まで。行末まで届く字（目次のページ番号など）は
        // 半角の漢数字も全角で組まれるので、段の終わりを越えない数に減らす。
        let limit = tiers
            .get(tier)
            .map_or(f32::MAX, |&(_, end)| end.max(candidate.bounds.x1))
            + 0.25 * size;
        // 始め括弧の字形は字の枠の後ろ半分にあり、字の枠は外接矩形より半字前から始まる。
        let opening = candidate
            .text
            .trim_start()
            .starts_with(['「', '（', '〔', '『', '【', '［', '｛', '〈', '《']);
        let target = candidate.bounds.x0
            - if opening {
                0.5 * candidate.font_size
            } else {
                0.0
            };
        let mut count = ((target - set_end) / size).round().max(0.0);
        while count > 0.0 && set_end + count * size + set_width(candidate) > limit {
            count -= 1.0;
        }
        let next_start = if gap < JOIN_GAP_EM * pitch {
            set_end
        } else {
            set_end + count.max(0.0) * size
        };
        // 全角で組んだ字は原本より前へは出ない（前の行の文字片を拾っても原本の位置より前に
        // しない）。
        let next_start = set_start[next]
            .map_or(next_start, |start| start.max(next_start))
            .max(candidate.bounds.x0);
        set_start[next] = Some(next_start);
        if gap < JOIN_GAP_EM * pitch
            || count < 1.0
            || (candidate.font_size - base.font_size).abs() > 0.3 * size
            || nearest_tier(tiers, candidate.bounds.x0, candidate.bounds.x1) != tier
            || graphics_between(
                parsed,
                page,
                (base.bounds.x1, candidate.bounds.x0),
                (base.bounds.y0, base.bounds.y1),
            )
        {
            continue;
        }
        pads.push((current, count as usize, candidate.bounds.x0));
    }
    for (index, count, end) in pads {
        let text = &mut parsed.text_elements[index];
        text.text = format!("{}{}", text.text.trim_end(), "\u{3000}".repeat(count));
        text.bounds.x1 = end;
    }
}

/// 文字片を組んだときの長さ（全角の字は 1 字、ほかの字は半角とみなす）。
fn set_width(text: &ParsedTextElement) -> f32 {
    let ems = text
        .text
        .trim()
        .chars()
        .map(|ch| if u32::from(ch) >= 0x2E80 { 1.0 } else { 0.5 })
        .sum::<f32>();
    ems * text.font_size.max(1.0)
}

/// 全角の字の間の半角空白を除く（縦組みの約物の外接矩形の間から補われた空白）。
fn without_spaces_between_wide_chars(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let wide = |ch: Option<&char>| ch.is_some_and(|ch| u32::from(*ch) >= 0x3000);
    chars
        .iter()
        .enumerate()
        .filter(|(index, ch)| {
            !(**ch == ' '
                && index
                    .checked_sub(1)
                    .is_some_and(|previous| wide(chars.get(previous)))
                && wide(chars[index + 1..].iter().find(|next| **next != ' ')))
        })
        .map(|(_, ch)| *ch)
        .collect()
}

/// 縦組みで字の枠の始まりの側に寄る約物（句読点・終わり括弧）。
const CLOSING_MARKS: [char; 12] = [
    '、', '。', '）', '」', '』', '〕', '］', '】', '〉', '》', '｝', '，',
];

/// 全角の字だけの行の範囲を、字数分の全角の送りに戻す。
///
/// 縦組みの括弧・句読点の外接矩形は字の枠の半分ほどしかなく、行頭の始め括弧は行の始まりを
/// 遅く、行末の約物は行の終わりを早く見せる。字下げや行の長さを字の枠でそろえるため、
/// 始め括弧で始まる行は始まりを、1 字だけの字は中央を、それ以外は終わりを、字数分の全角の
/// 送りの位置にする。
fn restore_em_extent(text: &mut ParsedTextElement, (tier_start, tier_end): (f32, f32), pitch: f32) {
    // 原本の文字列の端の半角空白（字間から補われたもの）は、行の中の空きを全角の空白で
    // 埋めるので落とす。
    let trimmed = without_spaces_between_wide_chars(text.text.trim());
    text.text = trimmed;
    let body = text.text.clone();
    let count = body.chars().count();
    let size = text.font_size;
    if count == 0
        || !(size.is_finite() && size > 0.0)
        || !body.chars().all(|ch| u32::from(ch) >= 0x3000)
    {
        return;
    }
    let advance = pitch * count as f32;
    let extent = text.bounds.x1 - text.bounds.x0;
    // 字送りの詰まった行（字の大きさより狭い送り）は戻さない。
    if extent >= advance - 0.25 * pitch || extent < advance - pitch {
        return;
    }
    if body.starts_with(['（', '「', '『', '〔', '［', '【', '〈', '《', '｛']) {
        text.bounds.x0 = (text.bounds.x1 - advance).max(tier_start);
    } else if count == 1 && !body.starts_with(CLOSING_MARKS) {
        // 「一」「・」など 1 字だけの字は字の枠の中央にある。
        let center = (text.bounds.x0 + text.bounds.x1) / 2.0;
        text.bounds.x0 = (center - size / 2.0).max(tier_start);
        text.bounds.x1 = (center + size / 2.0).min(tier_end);
    } else {
        text.bounds.x1 = (text.bounds.x0 + advance).min(tier_end);
    }
}

/// 横の範囲 `(x0, x1)` の中央が属する段（範囲の外なら最も近い段）。
fn nearest_tier(tiers: &[(f32, f32)], x0: f32, x1: f32) -> usize {
    let center = (x0 + x1) / 2.0;
    tiers
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let distance = |tier: &(f32, f32)| (tier.0 - center).max(center - tier.1).max(0.0);
            distance(a).total_cmp(&distance(b))
        })
        .map_or(0, |(index, _)| index)
}

/// 縦書きのページを時計回りに 90 度回し、段を上から下へ積んだ座標にして、ページに縦書きの
/// 印と段の数を付ける。
///
/// 回した座標では原本の上下の段が左右に並ぶ。段ごとに 1 段の高さずつ下へずらして積むと、
/// 行の組み立て・表の推定は段をまたがず、上から下への読み順が段の順になる。横の位置は
/// 原本のまま（段の左端が Word の段の左端に当たる）。
pub(super) fn rotate(parsed: &mut ParsedDocument) {
    let pages = parsed
        .pages
        .iter()
        .filter(|page| !is_vertical(page) && page_is_vertical(parsed, page.page_number))
        .map(|page| (page.page_number, parsed_page_display_size_points(page)))
        .collect::<Vec<_>>();
    for (page, (display_width, display_height)) in pages {
        for text in parsed
            .text_elements
            .iter_mut()
            .filter(|text| text.page == page)
        {
            text.bounds = rotate_bounds(text.bounds, display_height);
            text.baseline_y = None;
        }
        for graphic in parsed
            .graphics
            .iter_mut()
            .filter(|graphic| graphic.page == page)
        {
            graphic.bounds = rotate_bounds(graphic.bounds, display_height);
        }
        for fill in parsed.fills.iter_mut().filter(|fill| fill.page == page) {
            fill.bounds = rotate_bounds(fill.bounds, display_height);
        }
        for image in parsed
            .image_elements
            .iter_mut()
            .filter(|image| image.page == page)
        {
            image.bounds = rotate_bounds(image.bounds, display_height);
        }
        // 段は字形の外接矩形で分け（字の枠に戻すと段の間の空きを埋めてしまう）、
        // 行の文字片はその段の中だけでつなぐ。
        let tiers = detect_tiers(parsed, page, display_height, display_width);
        let pitches = Pitches::measure(parsed, page);
        collapse_horizontal_runs(parsed, page, &tiers);
        join_line_fragments(parsed, page, &tiers, &pitches);
        pad_row_gaps(parsed, page, &tiers, &pitches);
        let count = tiers.len().max(1);
        let shift = |bounds: &mut ParsedBounds| {
            let tier = nearest_tier(&tiers, bounds.x0, bounds.x1);
            let dy = display_width * (count - 1 - tier.min(count - 1)) as f32;
            bounds.y0 += dy;
            bounds.y1 += dy;
        };
        for text in parsed
            .text_elements
            .iter_mut()
            .filter(|text| text.page == page)
        {
            shift(&mut text.bounds);
        }
        for graphic in parsed
            .graphics
            .iter_mut()
            .filter(|graphic| graphic.page == page)
        {
            shift(&mut graphic.bounds);
        }
        for fill in parsed.fills.iter_mut().filter(|fill| fill.page == page) {
            shift(&mut fill.bounds);
        }
        for image in parsed
            .image_elements
            .iter_mut()
            .filter(|image| image.page == page)
        {
            shift(&mut image.bounds);
        }
        if let Some(parsed_page) = parsed
            .pages
            .iter_mut()
            .find(|parsed_page| parsed_page.page_number == page)
        {
            parsed_page.width = display_height;
            parsed_page.height = display_width * count as f32;
            parsed_page.rotation = MARK + 360 * i32::try_from(count).unwrap_or(1);
        }
    }
}

/// 縦書きのページの段の左右の範囲（回した座標、pt）。段に積んだ高さから求める。
pub(crate) fn tiers(parsed: &ParsedDocument, page: &ParsedPage) -> Vec<(f32, f32)> {
    let mut tiers = vec![(f32::INFINITY, f32::NEG_INFINITY); tier_count(page)];
    let mut include = |bounds: ParsedBounds| {
        let bounds = normalized_bounds(bounds);
        if !(bounds.x0.is_finite() && bounds.x1.is_finite()) {
            return;
        }
        let tier = &mut tiers[tier_of(page, (bounds.y0 + bounds.y1) / 2.0)];
        tier.0 = tier.0.min(bounds.x0);
        tier.1 = tier.1.max(bounds.x1);
    };
    let wide = 0.6 * page.width;
    for text in parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page.page_number && char_count(text) > 0)
    {
        include(text.bounds);
    }
    for bounds in parsed
        .graphics
        .iter()
        .filter(|graphic| graphic.page == page.page_number)
        .map(|graphic| graphic.bounds)
        .chain(
            parsed
                .fills
                .iter()
                .filter(|fill| fill.page == page.page_number)
                .map(|fill| fill.bounds),
        )
        .filter(|bounds| (bounds.x1 - bounds.x0).abs() < wide)
    {
        include(bounds);
    }
    tiers
}

/// 回した座標の用紙寸法・余白（twips）を、原本の向きの縦書きの section の寸法・余白にする。
///
/// 回した座標の左は原本の上、上は原本の右、右は原本の下、下は原本の左に当たる。積んだ段の
/// 高さは 1 段分に戻す。
pub(crate) fn physical_layout(
    page: &ParsedPage,
    (width, height): (u32, u32),
    margin: PageMargin,
) -> ((u32, u32), PageMargin) {
    let count = u32::try_from(tier_count(page)).unwrap_or(1).max(1);
    let physical = PageMargin::new()
        .top(margin.left)
        .right(margin.top)
        .bottom(margin.right)
        .left(margin.bottom)
        .header(margin.header)
        .footer(margin.footer)
        .gutter(margin.gutter);
    ((height / count, width), physical)
}

/// 縦書きのページの画像を、原本の向きの用紙の位置に浮かせた図にする（行の送りを使わない）。
///
/// 回した座標の画像の範囲を、段に積む前の原本の位置（左上原点）に戻す。
pub(crate) fn floating_picture(mut image: ParsedImageElement, page: &ParsedPage) -> Option<Pic> {
    let bounds = normalized_bounds(image.bounds);
    let band = band_height(page);
    let tier = tier_of(page, (bounds.y0 + bounds.y1) / 2.0);
    let shift = band * (tier_count(page) - 1 - tier) as f32;
    // 回した座標の y は原本の x、x は原本の上端からの距離。
    let (left, top) = (bounds.y0 - shift, bounds.x0);
    let (width, height) = (bounds.y1 - bounds.y0, bounds.x1 - bounds.x0);
    let emu = |points: f32| {
        let value = (f64::from(points) * 12_700.0).round();
        (value.is_finite() && value >= 0.0 && value <= f64::from(i32::MAX)).then_some(value as i32)
    };
    let (width_emu, height_emu) = (emu(width)?, emu(height)?);
    let bytes = std::mem::take(&mut image.png_bytes);
    Some(
        Pic::new_with_dimensions(bytes, 1, 1)
            .size(
                u32::try_from(width_emu).ok()?.max(1),
                u32::try_from(height_emu).ok()?.max(1),
            )
            .floating()
            .offset_x(emu(left)?)
            .offset_y(emu(top)?)
            .relative_from_h(RelativeFromHType::Page)
            .relative_from_v(RelativeFromVType::Page)
            .relative_height(FLOATING_PICTURE_MARK)
            .overlapping(),
    )
}

/// 縦書きのページの浮かせた図の印（`relativeHeight`）。梱包時に本文の背面へ回す。
const FLOATING_PICTURE_MARK: u32 = 190_517;

/// 縦書きの section の段間の上限（twips）。
const MAX_COLUMN_SPACE_TWIPS: i64 = 40;

/// 縦書きの section の書字方向。
pub(crate) const TEXT_DIRECTION: &str = "tbRl";

/// 縦書きの section の `w:cols` を、段の範囲（回した座標、pt）に合わせた幅に書き換える。
///
/// `tiers` は本文 XML に現れる縦書きの section の順。段の幅は section の上余白（原本の向き）
/// から測り、最後の段は下余白までの残りを使う。
pub(crate) fn apply_columns(document: &mut Vec<u8>, tiers: &[Vec<(f32, f32)>]) {
    if tiers.is_empty() {
        return;
    }
    // 浮かせた図は本文の背面に置き、本文を回り込ませない（縦書きでは回り込みの範囲が
    // 回した向きで計られ、本文の行を途中で切ることがある）。
    let marked = format!(
        "allowOverlap=\"0\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"0\" relativeHeight=\"{FLOATING_PICTURE_MARK}\""
    );
    if let Ok(text) = std::str::from_utf8(document)
        && text.contains(&marked)
    {
        let behind = marked
            .replace("allowOverlap=\"0\"", "allowOverlap=\"1\"")
            .replace("behindDoc=\"0\"", "behindDoc=\"1\"");
        *document = text.replace(&marked, &behind).into_bytes();
    }
    let Ok(text) = std::str::from_utf8(document) else {
        return;
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut index = 0usize;
    while let Some(start) = rest.find("<w:sectPr") {
        let Some(length) = rest[start..].find("</w:sectPr>") else {
            break;
        };
        let end = start + length + "</w:sectPr>".len();
        let section = &rest[start..end];
        out.push_str(&rest[..start]);
        if section.contains(&format!("w:textDirection w:val=\"{TEXT_DIRECTION}\"")) {
            match tiers
                .get(index)
                .and_then(|tiers| section_with_columns(section, tiers))
            {
                Some(rewritten) => out.push_str(&rewritten),
                None => out.push_str(section),
            }
            index += 1;
        } else {
            out.push_str(section);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    *document = out.into_bytes();
}

/// 要素 `tag` の属性 `name` の数値。
fn attribute(section: &str, tag: &str, name: &str) -> Option<i64> {
    let start = section.find(&format!("<{tag} "))?;
    let element = &section[start..start + section[start..].find('>')?];
    let key = format!(" {name}=\"");
    let value = &element[element.find(&key)? + key.len()..];
    value[..value.find('"')?].parse().ok()
}

/// section の `w:cols` を段の幅に書き換える（段が 1 つ、または寸法が合わなければ `None`）。
fn section_with_columns(section: &str, tiers: &[(f32, f32)]) -> Option<String> {
    if tiers.len() < 2 || tiers.iter().any(|(x0, x1)| !(x0.is_finite() && x1 > x0)) {
        return None;
    }
    let height = attribute(section, "w:pgSz", "w:h")?;
    let top = attribute(section, "w:pgMar", "w:top")?;
    let bottom = attribute(section, "w:pgMar", "w:bottom")?;
    let available = height - top - bottom;
    let twips = |points: f32| (f64::from(points) * 20.0).round() as i64;
    // 各段の始まり（上余白から。最初の段は上余白の位置）。
    let starts = tiers
        .iter()
        .enumerate()
        .map(|(index, tier)| if index == 0 { 0 } else { twips(tier.0) - top })
        .collect::<Vec<_>>();
    if starts.windows(2).any(|pair| pair[1] <= pair[0]) || available <= starts[starts.len() - 1] {
        return None;
    }
    let mut columns = Vec::new();
    for (index, tier) in tiers.iter().enumerate() {
        let Some(next) = starts.get(index + 1) else {
            columns.push((available - starts[index], 0));
            break;
        };
        // 字の外接矩形は字送りより狭く、段の幅を字面の範囲にすると行末の字が折り返す。
        // 段間の大半を段の幅に回し、段間は [`MAX_COLUMN_SPACE_TWIPS`] までにする。
        let end = (twips(tier.1) - top).clamp(starts[index] + 1, *next);
        let space = (next - end).min(MAX_COLUMN_SPACE_TWIPS);
        columns.push((next - space - starts[index], space));
    }
    let cols_start = section.find("<w:cols")?;
    let cols_end = cols_start
        + if let Some(close) = section[cols_start..].find("</w:cols>") {
            close + "</w:cols>".len()
        } else {
            section[cols_start..].find("/>")? + 2
        };
    let mut cols = format!(
        "<w:cols w:num=\"{}\" w:space=\"{}\" w:equalWidth=\"0\">",
        columns.len(),
        columns[0].1
    );
    for (width, space) in &columns {
        cols.push_str(&format!("<w:col w:w=\"{width}\" w:space=\"{space}\"/>"));
    }
    cols.push_str("</w:cols>");
    Some(format!(
        "{}{}{}",
        &section[..cols_start],
        cols,
        &section[cols_end..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用の文字片。
    fn text(body: &str, x0: f32, y0: f32, x1: f32, y1: f32) -> ParsedTextElement {
        ParsedTextElement {
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds { x0, y0, x1, y1 },
            font_size: 8.0,
            font_name: None,
            fill_color: None,
            text: body.to_string(),
        }
    }

    /// テスト用の解析結果（A4 縦 1 ページ）。
    fn document(text_elements: Vec<ParsedTextElement>) -> ParsedDocument {
        ParsedDocument {
            quality: QualityReport::default(),
            path: std::path::PathBuf::new(),
            byte_len: 0,
            header: "%PDF-1.4".into(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 4 },
            page_count: 1,
            pages: vec![ParsedPage {
                page_number: 0,
                width: 595.0,
                height: 842.0,
                rotation: 0,
            }],
            graphics: Vec::new(),
            fills: Vec::new(),
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
            text_elements,
        }
    }

    /// 縦の行のページは、右の行が上の横の行になるように回し、縦書きの印を付ける。
    #[test]
    fn vertical_columns_become_lines_read_from_the_right() {
        let body = "農業の担い手に対する経営安定のための交付金の交付に関する法律";
        let mut parsed = document(vec![
            text(body, 452.0, 400.0, 460.0, 780.0),
            text(body, 440.0, 400.0, 448.0, 780.0),
        ]);
        rotate(&mut parsed);
        assert!(is_vertical(&parsed.pages[0]));
        assert_eq!(
            parsed_page_display_size_points(&parsed.pages[0]),
            (842.0, 595.0)
        );
        let right = normalized_bounds(parsed.text_elements[0].bounds);
        let left = normalized_bounds(parsed.text_elements[1].bounds);
        // 原本の上端から 62pt の位置で始まる横の行になる。
        assert_eq!((right.x0, right.x1), (62.0, 442.0));
        // 右の縦の行は上の横の行になる。
        assert!(right.y0 > left.y1);
    }

    /// 上下の段は、上の段を先に読むよう上から下へ積み、段ごとの範囲を返す。
    #[test]
    fn tiers_are_stacked_in_reading_order() {
        let body = "農業の担い手に対する経営安定のための交付金の交付に関する法律";
        let mut parsed = document(vec![
            // 下の段の右端の行
            text(body, 540.0, 60.0, 548.0, 400.0),
            // 上の段の右端と左端の行
            text(body, 540.0, 440.0, 548.0, 780.0),
            text(body, 60.0, 440.0, 68.0, 780.0),
        ]);
        rotate(&mut parsed);
        let page = &parsed.pages[0];
        assert_eq!(tier_count(page), 2);
        assert_eq!(parsed_page_display_size_points(page), (842.0, 1190.0));
        let tops = parsed
            .text_elements
            .iter()
            .map(|text| tier_of(page, normalized_bounds(text.bounds).y1))
            .collect::<Vec<_>>();
        assert_eq!(tops, vec![1, 0, 0]);
        assert_eq!(tiers(&parsed, page), vec![(62.0, 402.0), (442.0, 782.0)]);
        // 下の段の右端の行は、上の段の右端の行と同じ高さから 1 段分下。
        let lower = normalized_bounds(parsed.text_elements[0].bounds);
        let upper = normalized_bounds(parsed.text_elements[1].bounds);
        assert_eq!(upper.y1 - lower.y1, 595.0);
    }

    /// 段の範囲に合わせて縦書きの section の段幅を書き換える。
    #[test]
    fn columns_follow_the_tiers() {
        let section = "<w:sectPr><w:pgSz w:w=\"11900\" w:h=\"16840\" /><w:pgMar w:top=\"1240\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" /><w:cols w:space=\"425\" w:num=\"2\" /><w:textDirection w:val=\"tbRl\" /></w:sectPr>";
        let mut document = section.as_bytes().to_vec();
        apply_columns(&mut document, &[vec![(62.0, 402.0), (442.0, 782.0)]]);
        let document = String::from_utf8(document).unwrap();
        assert!(document.contains("<w:cols w:num=\"2\" w:space=\"40\" w:equalWidth=\"0\"><w:col w:w=\"7560\" w:space=\"40\"/><w:col w:w=\"8000\" w:space=\"0\"/></w:cols>"), "{document}");
    }

    /// 回した座標の文字片（行の高さ 8pt）。
    fn piece(body: &str, x0: f32, x1: f32, y0: f32, y1: f32) -> ParsedTextElement {
        text(body, x0, y0, x1, y1)
    }

    /// 字の枠の隅にある約物の間は空けずにつなぎ、離れた日付と署名の間は全角の空白で埋める。
    #[test]
    fn row_fragments_join_and_gaps_become_ideographic_spaces() {
        let mut parsed = document(vec![
            piece("平均である〇", 0.0, 46.5, 100.0, 108.0),
            piece("・", 50.5, 54.2, 102.0, 105.7),
            piece("五九", 55.8, 71.6, 100.0, 108.0),
            piece("令和八年", 0.0, 32.0, 120.0, 128.0),
            piece("大臣", 72.0, 88.0, 120.0, 128.0),
        ]);
        let tiers = vec![(0.0, 200.0)];
        let pitches = Pitches::measure(&parsed, 0);
        join_line_fragments(&mut parsed, 0, &tiers, &pitches);
        pad_row_gaps(&mut parsed, 0, &tiers, &pitches);
        let texts = parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec![
                "平均である〇・五九",
                "令和八年\u{3000}\u{3000}\u{3000}\u{3000}\u{3000}",
                "大臣"
            ]
        );
    }

    /// 段を区切る罫線があるページでは、段の下端のページ番号の帯は罫線までの段に入る。
    #[test]
    fn a_separator_rule_keeps_page_numbers_with_their_tier() {
        let mut parsed = document(vec![
            piece("目次の項目", 0.0, 180.0, 100.0, 108.0),
            piece("一〇", 190.0, 198.0, 100.0, 108.0),
            piece("本文の行", 206.0, 400.0, 100.0, 108.0),
        ]);
        assert_eq!(
            detect_tiers(&parsed, 0, 842.0, 500.0),
            vec![(0.0, 180.0), (190.0, 198.0), (206.0, 400.0)]
        );
        parsed.graphics.push(ParsedGraphicElement {
            page: 0,
            bounds: ParsedBounds {
                x0: 202.0,
                y0: 10.0,
                x1: 202.0,
                y1: 490.0,
            },
            stroke_width: 0.5,
            dashed: true,
        });
        assert_eq!(
            detect_tiers(&parsed, 0, 842.0, 500.0),
            vec![(0.0, 198.0), (206.0, 400.0)]
        );
    }

    /// 原本の半角の括弧・漢数字も全角で組まれるので、行末のページ番号の前の空白は、
    /// 組んだ行が段の終わりを越えない数にする。
    #[test]
    fn line_end_numbers_are_padded_to_fit_the_tier() {
        let mut parsed = document(vec![
            piece("（中部六二）", 0.0, 44.0, 100.0, 108.0),
            piece("一〇", 92.0, 100.0, 100.0, 108.0),
        ]);
        let tiers = vec![(0.0, 100.0)];
        let pitches = Pitches::measure(&parsed, 0);
        pad_row_gaps(&mut parsed, 0, &tiers, &pitches);
        assert_eq!(
            parsed.text_elements[0].text,
            "（中部六二）\u{3000}\u{3000}\u{3000}\u{3000}"
        );
    }

    /// 原本で上下に重ねた横組みの 2 行は、上の行から続けて組み、段の終わりを越えない。
    #[test]
    fn stacked_horizontal_runs_follow_each_other_inside_the_tier() {
        let mut parsed = document(vec![
            piece("発行内閣府", 193.0, 202.0, 470.0, 515.0),
            piece("（原稿作成国立印刷局）", 205.0, 212.0, 455.0, 530.0),
        ]);
        collapse_horizontal_runs(&mut parsed, 0, &[(61.0, 238.0)]);
        let bounds = parsed
            .text_elements
            .iter()
            .map(|text| (text.bounds.x0, text.bounds.x1))
            .collect::<Vec<_>>();
        assert_eq!(bounds, vec![(110.0, 150.0), (150.0, 238.0)]);
    }

    /// 約物を詰めて組んだ全角の文字片は原本の長さに収まる字間にし、詰めていない文字片は
    /// そのまま組む。
    #[test]
    fn compressed_punctuation_gets_a_negative_character_spacing() {
        // 20 字を 19 字分の長さに組んだ行
        let compressed = piece(
            "あるのは「の令和八年度」と、「当該交付前",
            0.0,
            152.0,
            0.0,
            8.0,
        );
        let placement = compressed_punctuation_placement(&compressed).expect("condensed");
        assert_eq!(placement.spacing, -7);
        let plain = piece(
            "生した時において、当該災害に際し災害救",
            0.0,
            151.0,
            0.0,
            8.0,
        );
        assert!(compressed_punctuation_placement(&plain).is_none());
    }

    /// 表のマスの境目の罫線をまたいで空白で埋めない。
    #[test]
    fn gaps_across_a_cell_rule_stay_apart() {
        let mut parsed = document(vec![
            piece("改正後", 0.0, 24.0, 100.0, 108.0),
            piece("改正前", 72.0, 96.0, 100.0, 108.0),
        ]);
        parsed.graphics.push(ParsedGraphicElement {
            page: 0,
            bounds: ParsedBounds {
                x0: 50.0,
                y0: 90.0,
                x1: 50.0,
                y1: 120.0,
            },
            stroke_width: 0.5,
            dashed: false,
        });
        let tiers = vec![(0.0, 200.0)];
        let pitches = Pitches::measure(&parsed, 0);
        pad_row_gaps(&mut parsed, 0, &tiers, &pitches);
        assert_eq!(parsed.text_elements[0].text, "改正後");
    }

    /// 横組みのページは回さない。
    #[test]
    fn horizontal_pages_stay_as_they_are() {
        let body = "農業の担い手に対する経営安定のための交付金の交付に関する法律";
        let mut parsed = document(vec![
            text(body, 60.0, 700.0, 440.0, 708.0),
            text(body, 60.0, 688.0, 440.0, 696.0),
        ]);
        rotate(&mut parsed);
        assert!(!is_vertical(&parsed.pages[0]));
        assert_eq!(parsed.text_elements[0].bounds.x0, 60.0);
    }

    /// 回した座標の余白は、原本の向きの余白に戻す。
    #[test]
    fn physical_layout_maps_margins_back_to_the_portrait_page() {
        let page = ParsedPage {
            page_number: 0,
            width: 842.0,
            height: 1190.0,
            rotation: MARK + 720,
        };
        let margin = PageMargin::new().top(100).left(200).right(0).bottom(0);
        let ((width, height), physical) = physical_layout(&page, (16840, 23800), margin);
        assert_eq!((width, height), (11900, 16840));
        assert_eq!(
            (physical.top, physical.right, physical.bottom, physical.left),
            (200, 100, 0, 0)
        );
    }
}
