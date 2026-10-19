//! path構築時のCTMとW/W*確定位置を保存し、通常clipを文字・Formへ適用する。
use crate::{
    decode::DecodeBudget,
    form_clip::{Clip, Point},
    metadata_workspace::try_push,
    parser,
    syntax::{Lexer, Token},
    text_draws::{Operand, operand},
};
use std::rc::Rc;

/// path clipの変更点。q/Qは領域を共有し、同じ頂点を複製しない。
#[derive(Default)]
pub(crate) struct Clips {
    /// operator末尾のbyte位置と、その直後のclip。
    entries: Vec<(usize, Option<Rc<Clip>>)>,
    /// 可視性を保証できないclipを一度でも適用したか。
    pub(crate) unsupported: bool,
}

impl Clips {
    /// 描画直前に有効な領域を追加の確保なしで検索する。
    pub(crate) fn get(&self, offset: usize) -> Option<&Clip> {
        let index = self.entries.partition_point(|entry| entry.0 <= offset);
        self.entries.get(index.checked_sub(1)?)?.1.as_deref()
    }
}

/// path終端の結果を、通常描画・非対応clip・有効clipへ分ける。
enum Applied {
    /// 可視領域を変更しないpath operator。
    Unchanged,
    /// 正確な可視性を判定できないpath。
    Unsupported,
    /// ページ空間の正規化済み凸多角形。
    Polygon(Vec<Point>),
}

/// 現在pathを一つの凸領域として保存する。pathはq/Qの退避対象ではない。
#[derive(Default)]
pub(crate) struct Path {
    /// 構築時のCTMを適用したページ空間の頂点。
    points: Vec<Point>,
    /// 曲線・複数subpath・非有限座標などを含むか。
    unsupported: bool,
    /// 明示的に閉じた後の追加lineを区別する。
    closed: bool,
    /// 次のpath終端でclipを適用するか。
    pending: bool,
}

impl Path {
    /// 頂点を累積metadata容量へ予約する。検証CPUは64頂点以内に制限する。
    pub(crate) fn push(&mut self, budget: &DecodeBudget, point: Point) -> Option<()> {
        // nativeは閉路の始点を末尾にも返す。上限ちょうどの閉路でも追加確保しない。
        if self.points.len() == 64 && self.points.first() == Some(&point) {
            return Some(());
        }
        if self.points.len() >= 64 || !point.iter().all(|v| v.is_finite()) {
            self.unsupported = true;
        } else {
            try_push(budget, &mut self.points, point)?;
        }
        Some(())
    }

    /// nativeからも同じ凸性・向きの検証を使う。非対応なら現在の頂点を採用しない。
    pub(crate) fn finish(mut self) -> Option<Vec<Point>> {
        if self.unsupported {
            return None;
        }
        if self.points.len() > 1 && self.points.first() == self.points.last() {
            self.points.pop();
        }
        let mut area = 0.;
        if let Some(&origin) = self.points.first() {
            for edge in self.points.windows(2) {
                area += side(origin, edge[0], edge[1]);
            }
        }
        if area < 0. {
            self.points.reverse();
        }
        // 各辺の半平面に全頂点が入ることを確認し、自己交差する星形も拒否する。
        for (start, end) in self.points.iter().zip(self.points.iter().cycle().skip(1)) {
            if self
                .points
                .iter()
                .any(|point| side(*start, *end, *point) < 0.)
            {
                return None;
            }
        }
        if area == 0. {
            self.points.clear();
        }
        Some(self.points)
    }

    /// path operatorを観測し、描画・nの終端でのみclipを確定する。
    fn observe(
        &mut self,
        budget: &DecodeBudget,
        word: &[u8],
        args: &[f32],
        matrix: [f32; 6],
    ) -> Option<Applied> {
        match word {
            b"m" | b"l" if args.len() == 2 => {
                if (word == b"m" && !self.points.is_empty())
                    || (word == b"l" && (self.points.is_empty() || self.closed))
                {
                    self.unsupported = true;
                }
                self.push(budget, transform([args[0] as f64, args[1] as f64], matrix))?;
            }
            b"re" if args.len() == 4 => {
                if !self.points.is_empty() {
                    self.unsupported = true;
                }
                let [x, y, w, h] = [
                    args[0] as f64,
                    args[1] as f64,
                    args[2] as f64,
                    args[3] as f64,
                ];
                for point in [[x, y], [x + w, y], [x + w, y + h], [x, y + h]] {
                    self.push(budget, transform(point, matrix))?;
                }
                self.closed = true;
            }
            b"h" if args.is_empty() => self.closed = true,
            b"W" | b"W*" if args.is_empty() => self.pending = true,
            b"n" | b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*"
                if args.is_empty() =>
            {
                let previous = std::mem::take(self);
                return Some(
                    if previous.pending && (!previous.points.is_empty() || previous.unsupported) {
                        previous
                            .finish()
                            .map_or(Applied::Unsupported, Applied::Polygon)
                    } else {
                        Applied::Unchanged
                    },
                );
            }
            b"m" | b"l" | b"re" | b"c" | b"v" | b"y" | b"h" => self.unsupported = true,
            _ => {}
        }
        Some(Applied::Unchanged)
    }
}

/// ページ座標へ変換し、f32の積でのoverflowを避ける。
pub(crate) fn transform([x, y]: Point, matrix: [f32; 6]) -> Point {
    let [a, b, c, d, e, f] = matrix.map(f64::from);
    [a * x + c * y + e, b * x + d * y + f]
}

/// 三点の外積で半平面を判定する。
fn side(a: Point, b: Point, p: Point) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Rc本体とcounterを確保前に累積metadata容量へ予約する。
fn share(budget: &DecodeBudget, clip: Clip) -> Option<Rc<Clip>> {
    budget.charge_metadata_workspace_bytes(
        (std::mem::size_of::<Clip>() + 2 * std::mem::size_of::<usize>()) as u64,
    )?;
    Some(Rc::new(clip))
}

/// CTMとclipをq/Qで退避し、Contents連結後の本文を走査する。
pub(crate) fn scan(budget: &DecodeBudget, content: &[u8], initial: [f32; 6]) -> Clips {
    if !content.contains(&b'W') || budget.is_stopped() || !has_clip(budget, content) {
        return Clips::default();
    }
    let mut result = Clips::default();
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    let mut matrix = initial;
    let mut clip: Option<Rc<Clip>> = None;
    let mut saved = Vec::new();
    let mut path = Path::default();
    let mut args = [Operand::Other; 6];
    let mut count = 0;
    while let Some((token, span)) = lexer.next_spanned() {
        if budget.is_stopped() {
            return Clips::default();
        }
        let Token::Word(word) = token else {
            let value = operand(&mut lexer, token, span.start);
            if count < args.len() {
                args[count] = value;
                count += 1;
            } else {
                count = 0;
            }
            continue;
        };
        if let Some(value) = crate::page_attributes::number_value(word) {
            if count < args.len() {
                args[count] = Operand::Number(value);
                count += 1;
            } else {
                count = 0;
            }
            continue;
        }
        let mut numbers = [0.; 6];
        let numeric = args[..count].iter().zip(&mut numbers).all(|(arg, n)| {
            if let Operand::Number(value) = arg {
                *n = *value;
                true
            } else {
                false
            }
        });
        let mut changed = false;
        match word {
            b"q" if count == 0 => {
                if budget.check_form_graphics_depth(saved.len() + 1).is_none()
                    || try_push(budget, &mut saved, (matrix, clip.clone())).is_none()
                {
                    return Clips::default();
                }
            }
            b"Q" if count == 0 => {
                if let Some(previous) = saved.pop() {
                    (matrix, clip) = previous;
                    changed = true;
                }
            }
            b"cm" if count == 6 && numeric => matrix = parser::concat_matrix(matrix, numbers),
            b"BI" if count == 0 => break,
            _ => {
                let Some(applied) = path.observe(
                    budget,
                    word,
                    if numeric {
                        &numbers[..count]
                    } else {
                        &[f32::NAN]
                    },
                    matrix,
                ) else {
                    return Clips::default();
                };
                match applied {
                    Applied::Polygon(polygon) => {
                        let next = match &clip {
                            Some(clip) => clip.intersect_polygon(budget, polygon),
                            None => Some(Clip::polygon(polygon)),
                        };
                        let Some(next) = next.and_then(|clip| share(budget, clip)) else {
                            return Clips::default();
                        };
                        clip = Some(next);
                        changed = true;
                    }
                    Applied::Unsupported => result.unsupported = true,
                    Applied::Unchanged => {}
                }
            }
        }
        if changed && try_push(budget, &mut result.entries, (span.end, clip.clone())).is_none() {
            return Clips::default();
        }
        count = 0;
    }
    result
}

/// 偽operatorを読み飛ばし、実際のclipがある本文だけにtimeline容量を使う。
fn has_clip(budget: &DecodeBudget, content: &[u8]) -> bool {
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    while let Some((token, span)) = lexer.next_spanned() {
        if budget.is_stopped() {
            return false;
        }
        match token {
            Token::Word(b"W" | b"W*") => return true,
            Token::Word(b"BI") => return false,
            Token::Word(_) => {}
            token => {
                operand(&mut lexer, token, span.start);
            }
        }
    }
    false
}

#[cfg(test)]
#[path = "content_clip_tests.rs"]
mod tests;
