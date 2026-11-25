//! 有限のType1 operand stackと描画状態。未知の動的命令は失敗として返す。
use super::{Command, Glyph};
use crate::decode::DecodeBudget;
use crate::type1_outline::standard::Verified;
#[path = "type1_outline_flex.rs"]
mod flex;

/// 一glyph内で共有するstack・相対原点・処理量。
pub(super) struct State<'a> {
    /// 取消と外形保存量を共有する予算。
    budget: &'a DecodeBudget,
    /// Type1の最大operand数に制限したstack。
    stack: [f64; 48],
    /// stackの使用数。
    count: usize,
    /// 現在の相対原点。
    point: [f64; 2],
    /// 一度だけ設定可能な水平advance。
    advance: Option<f64>,
    /// 完了まで外部へ渡さない描画命令。
    commands: Vec<Command>,
    /// 閉じる必要がある輪郭の有無。
    open: bool,
    /// subroutineを含む残り処理回数。
    remaining: usize,
    /// 最上位のendcharまで到達したか。
    ended: bool,
    /// 完全な定義を照合した標準OtherSubrsだけを許可する。
    standard: Option<Verified>,
    /// Flex中の七点。輪郭命令にはまだ追加しない。
    flex: Option<flex::Capture>,
    /// 標準OtherSubrsが返す最大二つの結果。
    returned: [f64; 2],
    /// 今回返す結果の個数。
    return_count: usize,
    /// 次にpopする結果の位置。
    return_cursor: usize,
    /// Flexの二つのpop後にsetcurrentpointを必須とする。
    restore_point: bool,
}

impl<'a> State<'a> {
    /// 動的確保せずに初期状態を作る。
    pub(super) fn new(budget: &'a DecodeBudget) -> Self {
        Self {
            budget,
            stack: [0.; 48],
            count: 0,
            point: [0.; 2],
            advance: None,
            commands: Vec::new(),
            open: false,
            remaining: 100_000,
            ended: false,
            standard: None,
            flex: None,
            returned: [0.; 2],
            return_count: 0,
            return_cursor: 0,
            restore_point: false,
        }
    }

    /// 標準定義の承認値を持つ場合だけ専用の補助演算を有効にする。
    pub(super) fn with_standard(budget: &'a DecodeBudget, verified: Verified) -> Self {
        let mut state = Self::new(budget);
        state.standard = Some(verified);
        state
    }

    /// 保存前に容量を予約し、途中失敗したglyphは公開しない。
    fn emit(&mut self, command: Command) -> Option<()> {
        if self.commands.len() == self.commands.capacity() {
            let capacity = self.commands.capacity().checked_mul(2)?.max(4);
            let mut expanded = self.budget.font_vector(capacity)?;
            expanded.append(&mut self.commands);
            self.commands = expanded;
        }
        self.commands.push(command);
        Some(())
    }

    /// operand stackの有限性と容量を検査する。
    fn push(&mut self, value: f64) -> Option<()> {
        if !value.is_finite() {
            return None;
        }
        *self.stack.get_mut(self.count)? = value;
        self.count += 1;
        Some(())
    }

    /// 必要なoperandを末尾から一つ取り出す。
    fn pop(&mut self) -> Option<f64> {
        self.count = self.count.checked_sub(1)?;
        Some(self.stack[self.count])
    }

    /// 描画命令の引数数を厳密に照合する。
    fn args<const N: usize>(&mut self) -> Option<[f64; N]> {
        if self.count != N {
            return None;
        }
        let mut args = [0.; N];
        args.copy_from_slice(&self.stack[..N]);
        self.count = 0;
        Some(args)
    }

    /// OpenTypeで保存できる座標範囲に制限して相対原点を進める。
    fn shift(&mut self, x: f64, y: f64) -> Option<[f64; 2]> {
        let point = [self.point[0] + x, self.point[1] + y];
        if point.iter().any(|v| !v.is_finite() || v.abs() > 32767.) {
            return None;
        }
        self.point = point;
        Some(point)
    }

    /// closepathでもType1の相対原点を変更しない。
    fn close(&mut self) -> Option<()> {
        if self.flex.is_some() {
            return None;
        }
        if self.open {
            self.emit(Command::Close)?;
            self.open = false;
        }
        Some(())
    }

    /// 新しい輪郭の前に旧輪郭を完了する。
    fn move_to(&mut self, x: f64, y: f64) -> Option<()> {
        self.advance?;
        if self.flex.is_some() {
            return self.flex_move(x, y);
        }
        self.close()?;
        let point = self.shift(x, y)?;
        self.emit(Command::Move(point))?;
        self.open = true;
        Some(())
    }

    /// 始点のある輪郭に直線を追加する。
    fn line_to(&mut self, x: f64, y: f64) -> Option<()> {
        if !self.open || self.flex.is_some() {
            return None;
        }
        let point = self.shift(x, y)?;
        self.emit(Command::Line(point))
    }

    /// 三つの相対座標を絶対座標の三次曲線へ変える。
    fn curve_to(&mut self, values: [f64; 6]) -> Option<()> {
        if !self.open || self.flex.is_some() {
            return None;
        }
        let a = self.shift(values[0], values[1])?;
        let b = self.shift(values[2], values[3])?;
        let c = self.shift(values[4], values[5])?;
        self.emit(Command::Curve([a[0], a[1], b[0], b[1], c[0], c[1]]))
    }

    /// 水平advanceとsidebearingは輪郭に先立って一度だけ定義する。
    fn width(&mut self, x: f64, y: f64, width: f64) -> Option<()> {
        if self.advance.is_some() || !self.commands.is_empty() || !(0. ..=65535.).contains(&width) {
            return None;
        }
        self.shift(x, y)?;
        self.advance = Some(width);
        Some(())
    }

    /// 数値byteをType1の整数表現として読み、Type2固定小数点とは区別する。
    fn number(byte: u8, bytes: &[u8], cursor: &mut usize) -> Option<f64> {
        let value = match byte {
            32..=246 => i32::from(byte) - 139,
            247..=254 => {
                let next = i32::from(*bytes.get(*cursor)?);
                *cursor += 1;
                if byte <= 250 {
                    (i32::from(byte) - 247) * 256 + next + 108
                } else {
                    -(i32::from(byte) - 251) * 256 - next - 108
                }
            }
            255 => {
                let end = cursor.checked_add(4)?;
                let value = i32::from_be_bytes(bytes.get(*cursor..end)?.try_into().ok()?);
                *cursor = end;
                value
            }
            _ => return None,
        };
        Some(f64::from(value))
    }

    /// subroutineでもstackと処理上限を共有し、終端の対応を厳密に検査する。
    pub(super) fn run(&mut self, bytes: &[u8], subrs: &[Vec<u8>], depth: usize) -> Option<()> {
        if depth > 10 {
            return None;
        }
        let mut cursor = 0;
        while let Some(&byte) = bytes.get(cursor) {
            if self.budget.is_stopped() {
                return None;
            }
            self.remaining = self.remaining.checked_sub(1)?;
            cursor += 1;
            if self.return_cursor < self.return_count {
                if byte != 12 || bytes.get(cursor) != Some(&17) {
                    return None;
                }
            } else if self.restore_point && (byte != 12 || bytes.get(cursor) != Some(&33)) {
                return None;
            }
            if byte >= 32 {
                self.push(Self::number(byte, bytes, &mut cursor)?)?;
                continue;
            }
            match byte {
                1 | 3 => {
                    self.args::<2>()?;
                }
                4 => {
                    let [y] = self.args()?;
                    self.move_to(0., y)?;
                }
                5 => {
                    let [x, y] = self.args()?;
                    self.line_to(x, y)?;
                }
                6 => {
                    let [x] = self.args()?;
                    self.line_to(x, 0.)?;
                }
                7 => {
                    let [y] = self.args()?;
                    self.line_to(0., y)?;
                }
                8 => {
                    let values = self.args()?;
                    self.curve_to(values)?;
                }
                9 => {
                    self.args::<0>()?;
                    self.close()?;
                }
                10 => {
                    let index = self.pop()?;
                    if index < 0. || index.fract() != 0. || index >= subrs.len() as f64 {
                        return None;
                    }
                    self.run(subrs.get(index as usize)?, subrs, depth + 1)?;
                }
                11 => return (depth > 0 && cursor == bytes.len()).then_some(()),
                12 => {
                    let escape = *bytes.get(cursor)?;
                    cursor += 1;
                    self.escape(escape)?;
                }
                13 => {
                    let [x, width] = self.args()?;
                    self.width(x, 0., width)?;
                }
                14 => {
                    if depth != 0 || cursor != bytes.len() {
                        return None;
                    }
                    self.args::<0>()?;
                    self.advance?;
                    self.close()?;
                    self.ended = true;
                    return Some(());
                }
                21 => {
                    let [x, y] = self.args()?;
                    self.move_to(x, y)?;
                }
                22 => {
                    let [x] = self.args()?;
                    self.move_to(x, 0.)?;
                }
                30 => {
                    let [y1, x2, y2, x3] = self.args()?;
                    self.curve_to([0., y1, x2, y2, x3, 0.])?;
                }
                31 => {
                    let [x1, x2, y2, y3] = self.args()?;
                    self.curve_to([x1, 0., x2, y2, 0., y3])?;
                }
                _ => return None,
            }
        }
        None
    }

    /// 静的な補助命令だけを許可し、任意PostScriptの呼出しは解釈しない。
    fn escape(&mut self, byte: u8) -> Option<()> {
        match byte {
            0 => {
                self.args::<0>()?;
            }
            1 | 2 => {
                self.args::<6>()?;
            }
            7 => {
                let [x, y, width, vertical] = self.args()?;
                if vertical != 0. {
                    return None;
                }
                self.width(x, y, width)?;
            }
            12 => {
                let divisor = self.pop()?;
                let numerator = self.pop()?;
                if divisor == 0. {
                    return None;
                }
                self.push(numerator / divisor)?;
            }
            16 => self.other_subr()?,
            17 => self.pop_result()?,
            33 => self.restore_currentpoint()?,
            _ => return None,
        }
        Some(())
    }

    /// 明示的な完了があったglyphだけを渡す。
    pub(super) fn finish(self) -> Option<Glyph> {
        if !self.ended || self.budget.is_stopped() {
            return None;
        }
        Some(Glyph {
            advance: self.advance?,
            commands: self.commands,
        })
    }
}
