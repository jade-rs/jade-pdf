//! PDF の文字描画命令を順に評価し、文字行列と行原点を分けて保持する。
use crate::{
    decode::DecodeBudget,
    page_attributes, parser,
    syntax::{Lexer, Token},
};

/// text object 間で引き継ぐ文字と塗りの状態。
pub(crate) struct State {
    /// 選択中の Font resource 名。
    pub(crate) font: Option<String>,
    /// Form開始時の選択Fontを使い、局所TfのResourcesで再解決しない印。
    pub(crate) inherited_font: bool,
    /// 文字空間での Font size。
    pub(crate) size: f32,
    /// Device color の RGB hex 近似。
    pub(crate) color: Option<String>,
    /// 次の行への移動量。
    leading: f32,
    /// glyph ごとの追加 advance。
    pub(crate) character_spacing: f32,
    /// 一 byte の code 32 に追加する advance。
    pub(crate) word_spacing: f32,
    /// 文字横軸と advance の倍率。
    pub(crate) horizontal_scale: f32,
    /// 文字縦軸に沿う baseline の移動量。
    pub(crate) rise: f32,
    /// q/QおよびForm呼び出しで保持する文字描画モード。
    pub(crate) rendering: crate::text_rendering::Mode,
    /// fill/strokeの透明度を独立して保持する。
    pub(crate) alpha: crate::text_alpha::State,
}

impl State {
    /// ページの文字状態の初期値を作る。
    pub(crate) fn initial(rendering: crate::text_rendering::Mode) -> Self {
        Self {
            font: None,
            inherited_font: false,
            size: 10.,
            color: None,
            leading: 0.,
            character_spacing: 0.,
            word_spacing: 0.,
            horizontal_scale: 1.,
            rise: 0.,
            rendering,
            alpha: Default::default(),
        }
    }

    /// q保存用のFont名と色もコピー前に共有metadata容量へ予約する。
    pub(crate) fn saved(&self, budget: &DecodeBudget) -> Option<Self> {
        let copy = |text: &Option<String>| -> Option<Option<String>> {
            match text {
                Some(text) => {
                    let mut bytes = budget.metadata_vector::<u8>(text.len())?;
                    bytes.extend_from_slice(text.as_bytes());
                    Some(Some(String::from_utf8(bytes).ok()?))
                }
                None => Some(None),
            }
        };
        Some(Self {
            font: copy(&self.font)?,
            inherited_font: self.inherited_font,
            color: copy(&self.color)?,
            size: self.size,
            leading: self.leading,
            character_spacing: self.character_spacing,
            word_spacing: self.word_spacing,
            horizontal_scale: self.horizontal_scale,
            rise: self.rise,
            rendering: self.rendering,
            alpha: self.alpha,
        })
    }

    /// 文字状態を変える命令を共有評価し、有効なTfだけ選択元の更新を通知する。
    pub(crate) fn update(
        &mut self,
        budget: &DecodeBudget,
        word: &[u8],
        args: &[Operand<'_>],
        in_text: bool,
    ) -> bool {
        let mut values = [0.; 6];
        let numbers = (args.len() <= 6
            && args.iter().zip(&mut values).all(|(arg, value)| {
                if let Operand::Number(number) = arg {
                    *value = *number;
                    true
                } else {
                    false
                }
            }))
        .then_some(&values[..args.len().min(6)]);
        match (word, args, numbers) {
            (b"Tf", [Operand::Name(name), Operand::Number(size)], _) => {
                self.font = crate::metadata_names::resource_name(budget, name);
                self.inherited_font = false;
                self.size = *size;
                return true;
            }
            (b"TL", _, Some([leading])) => self.leading = *leading,
            (b"TD", _, Some([_, y])) if in_text => self.leading = -*y,
            (b"Tc", _, Some([spacing])) => self.character_spacing = *spacing,
            (b"Tw", _, Some([spacing])) => self.word_spacing = *spacing,
            (b"Tz", _, Some([scale])) => self.horizontal_scale = *scale / 100.,
            (b"Ts", _, Some([rise])) => self.rise = *rise,
            (b"Tr", _, Some([value])) => {
                if let Some(mode) = crate::text_rendering::Mode::from_operand(*value) {
                    self.rendering = mode;
                }
            }
            (
                b"\"",
                [
                    Operand::Number(word),
                    Operand::Number(character),
                    Operand::String(_),
                ],
                _,
            ) if in_text => {
                self.word_spacing = *word;
                self.character_spacing = *character;
            }
            (b"rg", _, Some([r, g, b])) => {
                self.color = color(
                    budget,
                    [
                        parser::pdf_color_component_to_u8(*r),
                        parser::pdf_color_component_to_u8(*g),
                        parser::pdf_color_component_to_u8(*b),
                    ],
                );
            }
            (b"g", _, Some([g])) => {
                self.color = color(budget, [parser::pdf_color_component_to_u8(*g); 3]);
            }
            (b"k", _, Some([c, m, y, k])) => {
                self.color = color(budget, parser::pdf_cmyk_to_rgb(*c, *m, *y, *k));
            }
            _ => {}
        }
        false
    }
}

/// RGB hexの6byteだけを予約し、一時format文字列を作らず保存する。
fn color(budget: &DecodeBudget, rgb: [u8; 3]) -> Option<String> {
    let mut output = budget.metadata_vector::<u8>(6)?;
    for byte in rgb {
        output.push(b"0123456789ABCDEF"[(byte >> 4) as usize]);
        output.push(b"0123456789ABCDEF"[(byte & 15) as usize]);
    }
    String::from_utf8(output).ok()
}

/// 一つの命令の operand。container 内の偽命令は実行しない。
#[derive(Clone, Copy)]
pub(crate) enum Operand<'a> {
    /// 有限の PDF 数値。
    Number(f32),
    /// 未展開 resource 名。
    Name(&'a [u8]),
    /// literal/hex 文字列の元 byte 列。
    String(&'a [u8]),
    /// TJ 配列の元 byte 列。
    Array(&'a [u8]),
    /// その他の operand。
    Other,
}

/// 行列の文字空間の軸に沿って原点を移動する。
fn translate(matrix: [f32; 6], x: f32, y: f32) -> [f32; 6] {
    parser::concat_matrix(matrix, [1., 0., 0., 1., x, y])
}

/// 同じ文字軸・baseline 上で、推定 advance の半 em 以内に続く描画を判定する。
pub(crate) fn adjacent(previous: [f32; 6], width: f32, current: [f32; 6], size: f32) -> bool {
    if previous[..4] != current[..4] {
        return false;
    }
    let axis_squared = previous[0] * previous[0] + previous[1] * previous[1];
    if !axis_squared.is_finite() || axis_squared <= f32::EPSILON {
        return false;
    }
    let end = translate(previous, width, 0.);
    let dx = current[4] - end[4];
    let dy = current[5] - end[5];
    let gap = (dx * previous[0] + dy * previous[1]) / axis_squared;
    let baseline = (dx * previous[1] - dy * previous[0]) / axis_squared.sqrt();
    baseline.abs() <= 0.0001 && gap.abs() <= size.abs() * 0.5
}

/// 命令とcontainerを区別し、未完文字列・配列の後続描画へ復帰する。
pub(crate) fn operand<'a>(lexer: &mut Lexer<'a>, token: Token<'a>, start: usize) -> Operand<'a> {
    match token {
        Token::Name(name) => Operand::Name(name),
        Token::Word(word) => {
            page_attributes::number_value(word).map_or(Operand::Other, Operand::Number)
        }
        Token::Other if matches!(lexer.bytes[start], b'(' | b'<') => {
            let raw = &lexer.bytes[start..lexer.cursor];
            if crate::pdf_binary_string::visit_string(raw, |_| Some(())).is_none() {
                lexer.cursor = start + 1;
                Operand::Other
            } else {
                Operand::String(raw)
            }
        }
        Token::Open(dict) => {
            let mut depth = 1usize;
            let mut recovery = None;
            while let Some((token, span)) = lexer.next_spanned() {
                match token {
                    Token::Open(_) => depth += 1,
                    Token::Close(_) => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Token::Word(b"BT" | b"ET") if depth == 1 && recovery.is_none() => {
                        recovery = Some(span.start);
                    }
                    _ => {}
                }
            }
            if depth != 0 {
                // 完結したcontainer内の偽命令は実行せず、未完配列だけを境界へ戻す。
                if !dict && let Some(cursor) = recovery {
                    lexer.cursor = cursor;
                }
                Operand::Other
            } else if dict {
                Operand::Other
            } else {
                Operand::Array(&lexer.bytes[start..lexer.cursor])
            }
        }
        _ => Operand::Other,
    }
}

/// 描画直前の状態を渡し、返された局所幅だけ文字原点を進める。
fn show(
    raw: &[u8],
    state: &State,
    begin: usize,
    sequence: usize,
    matrix: &mut [f32; 6],
    draw: &mut impl FnMut(usize, usize, &[u8], &State, [f32; 6]) -> f32,
) {
    let width = draw(begin, sequence, raw, state, *matrix);
    *matrix = translate(*matrix, width, 0.);
}

/// 各 Tj/TJ/引用符の文字列を現在の状態で描画し、Td/Tm/改行を順に適用する。
#[cfg(test)]
pub(crate) fn visit(
    budget: &DecodeBudget,
    bytes: &[u8],
    draw: impl FnMut(usize, usize, &[u8], &State, [f32; 6]) -> f32,
) {
    visit_with_mode(budget, bytes, Default::default(), draw);
}

/// Form呼び出し元の描画モードを初期状態へ引き継いで命令を評価する。
#[cfg(test)]
pub(crate) fn visit_with_mode(
    budget: &DecodeBudget,
    bytes: &[u8],
    rendering: crate::text_rendering::Mode,
    draw: impl FnMut(usize, usize, &[u8], &State, [f32; 6]) -> f32,
) {
    visit_with_state(
        budget,
        bytes,
        State::initial(rendering),
        |_| Default::default(),
        draw,
    );
}

/// 選択Fontと文字状態を引き継ぎ、局所命令とq/Qを評価する。
pub(crate) fn visit_with_state(
    budget: &DecodeBudget,
    bytes: &[u8],
    mut state: State,
    mut alpha: impl FnMut(&[u8]) -> crate::text_alpha::Parameters,
    mut draw: impl FnMut(usize, usize, &[u8], &State, [f32; 6]) -> f32,
) {
    if budget.is_stopped() {
        return;
    }
    let identity = [1., 0., 0., 1., 0., 0.];
    let mut matrix = identity;
    let mut line = identity;
    let mut begin = None;
    let mut saved = Vec::new();
    let mut args = [Operand::Other; 6];
    let mut arg_len = 0;
    let mut lexer = Lexer { bytes, cursor: 0 };
    while let Some((token, span)) = lexer.next_spanned() {
        if budget.is_stopped() {
            return;
        }
        let Token::Word(word) = token else {
            let value = operand(&mut lexer, token, span.start);
            if arg_len < 6 {
                args[arg_len] = value;
                arg_len += 1;
            } else {
                arg_len = 0;
            }
            continue;
        };
        if let Some(number) = page_attributes::number_value(word) {
            if arg_len < 6 {
                args[arg_len] = Operand::Number(number);
                arg_len += 1;
            } else {
                arg_len = 0;
            }
            continue;
        }
        let mut values = [0.; 6];
        let numbers = args[..arg_len]
            .iter()
            .zip(&mut values)
            .all(|(value, number)| {
                if let Operand::Number(n) = value {
                    *number = *n;
                    true
                } else {
                    false
                }
            })
            .then_some(&values[..arg_len]);
        state.alpha.update(word, &args[..arg_len], &mut alpha);
        state.update(budget, word, &args[..arg_len], begin.is_some());
        match (word, &args[..arg_len], numbers) {
            (b"q", [], _) => {
                if budget.check_form_graphics_depth(saved.len() + 1).is_none() {
                    return;
                }
                let Some(previous) = state.saved(budget) else {
                    return;
                };
                if crate::metadata_workspace::try_push(
                    budget,
                    &mut saved,
                    (previous, begin.map(|_| (matrix, line))),
                )
                .is_none()
                {
                    return;
                }
            }
            (b"Q", [], _) => {
                if let Some((previous, matrices)) = saved.pop() {
                    state = previous;
                    if begin.is_some()
                        && let Some(previous) = matrices
                    {
                        (matrix, line) = previous;
                    }
                }
            }
            (b"BI", [], _) => break,
            (b"BT", [], _) => {
                begin = Some(span.start);
                matrix = identity;
                line = identity;
            }
            (b"ET", [], _) => begin = None,
            (b"Tm", _, Some([a, b, c, d, e, f])) if begin.is_some() => {
                matrix = [*a, *b, *c, *d, *e, *f];
                line = matrix;
            }
            (b"Td" | b"TD", _, Some([x, y])) if begin.is_some() => {
                line = translate(line, *x, *y);
                matrix = line;
            }
            (b"T*", [], _) if begin.is_some() => {
                line = translate(line, 0., -state.leading);
                matrix = line;
            }
            (b"Tj" | b"'", [Operand::String(raw)], _) if begin.is_some() => {
                if word == b"'" {
                    line = translate(line, 0., -state.leading);
                    matrix = line;
                }
                show(
                    raw,
                    &state,
                    begin.unwrap(),
                    span.start,
                    &mut matrix,
                    &mut draw,
                );
            }
            (b"\"", [Operand::Number(_), Operand::Number(_), Operand::String(raw)], _)
                if begin.is_some() =>
            {
                line = translate(line, 0., -state.leading);
                matrix = line;
                show(
                    raw,
                    &state,
                    begin.unwrap(),
                    span.start,
                    &mut matrix,
                    &mut draw,
                );
            }
            (b"TJ", [Operand::Array(array)], _) if begin.is_some() => {
                let sequence = span.start;
                let mut items = Lexer {
                    bytes: &array[1..array.len().saturating_sub(1)],
                    cursor: 0,
                };
                while let Some((token, span)) = items.next_spanned() {
                    if budget.is_stopped() {
                        return;
                    }
                    match operand(&mut items, token, span.start) {
                        Operand::String(raw) => show(
                            raw,
                            &state,
                            begin.unwrap(),
                            sequence,
                            &mut matrix,
                            &mut draw,
                        ),
                        Operand::Number(value) => {
                            matrix = translate(
                                matrix,
                                -value * state.size / 1000. * state.horizontal_scale,
                                0.,
                            )
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        arg_len = 0;
    }
}
