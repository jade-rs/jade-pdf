//! 承認済みの標準OtherSubrsだけを、PostScriptなしで型付きの二曲線へ変える。
use super::{Command, State};

/// Flexの参照点一つと、二曲線の六つの絶対座標を保持する固定長状態。
pub(super) struct Capture {
    /// 参照点・制御点・終点の順の七点。
    points: [[f64; 2]; 7],
    /// OtherSubrs2で確定した点数。
    count: usize,
    /// 直前のmoveでまだ確定していない点があるか。
    pending: bool,
}

impl State<'_> {
    /// Flex中のmoveは新しい輪郭を作らず、次の標準点確定だけに渡す。
    pub(super) fn flex_move(&mut self, x: f64, y: f64) -> Option<()> {
        if self.flex.as_ref()?.pending {
            return None;
        }
        self.shift(x, y)?;
        self.flex.as_mut()?.pending = true;
        Some(())
    }

    /// 標準の引数数と呼出順序を厳密に照合し、任意のPostScriptを呼ばない。
    pub(super) fn other_subr(&mut self) -> Option<()> {
        let verified = self.standard?;
        let index = self.pop()?;
        let count = self.pop()?;
        self.return_count = 0;
        self.return_cursor = 0;
        match (index, count) {
            (1., 0.) if verified.allows_flex() => {
                self.args::<0>()?;
                if self.flex.is_some() || !self.open {
                    return None;
                }
                self.flex = Some(Capture {
                    points: [[0.; 2]; 7],
                    count: 0,
                    pending: false,
                });
            }
            (2., 0.) if verified.allows_flex() => {
                self.args::<0>()?;
                let flex = self.flex.as_mut()?;
                if !flex.pending {
                    return None;
                }
                *flex.points.get_mut(flex.count)? = self.point;
                flex.count += 1;
                flex.pending = false;
            }
            (0., 3.) if verified.allows_flex() => {
                let [height, x, y] = self.args()?;
                let flex = self.flex.take()?;
                if height < 0.
                    || flex.count != 7
                    || flex.pending
                    || (self.point[0] - x).abs() > 1e-7
                    || (self.point[1] - y).abs() > 1e-7
                {
                    return None;
                }
                let p = flex.points;
                self.emit(Command::Curve([
                    p[1][0], p[1][1], p[2][0], p[2][1], p[3][0], p[3][1],
                ]))?;
                self.emit(Command::Curve([
                    p[4][0], p[4][1], p[5][0], p[5][1], p[6][0], p[6][1],
                ]))?;
                self.returned = [x, y];
                self.return_count = 2;
                self.restore_point = true;
            }
            (3., 1.) if self.flex.is_none() => {
                let [hint] = self.args()?;
                self.returned[0] = hint;
                self.return_count = 1;
            }
            _ => return None,
        }
        Some(())
    }

    /// callothersubr直後の明示的な結果だけを、順序どおりstackへ戻す。
    pub(super) fn pop_result(&mut self) -> Option<()> {
        if self.return_cursor >= self.return_count {
            return None;
        }
        let value = self.returned[self.return_cursor];
        self.return_cursor += 1;
        self.push(value)
    }

    /// Flexの終点に対応した二つのpopが完了した場合だけ現在点を確定する。
    pub(super) fn restore_currentpoint(&mut self) -> Option<()> {
        if !self.restore_point || self.return_count != 2 || self.return_cursor != 2 {
            return None;
        }
        let [x, y] = self.args()?;
        if [x, y] != self.returned {
            return None;
        }
        self.point = [x, y];
        self.restore_point = false;
        Some(())
    }
}
