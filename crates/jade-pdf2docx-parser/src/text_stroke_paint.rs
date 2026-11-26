//! 文字の通常weightと独立して、同じDevice色の塗りと輪郭線を追跡する。
/// 既定置換がないことは、公開結果へ渡す前に所属ページで別途検証する。
#[derive(Clone, Copy, PartialEq)]
struct Color {
    /// DeviceGray/RGB/CMYKの成分数。
    count: usize,
    /// clip済みの有限成分。使わない末尾はゼロ。
    values: [f32; 4],
}

/// q/Qとともに保存する、文字描画と共通の色・線幅。
#[derive(Clone, Copy)]
pub(crate) struct State {
    /// 非stroke色。不明な色空間・成分ではNone。
    fill: Option<Color>,
    /// stroke色。不明な色空間・成分ではNone。
    stroke: Option<Color>,
    /// ユーザー空間の線幅。
    width: f32,
}

impl Default for State {
    /// PDF既定のDeviceGray黒と線幅1を保持する。
    fn default() -> Self {
        let black = Some(Color {
            count: 1,
            values: [0.; 4],
        });
        Self {
            fill: black,
            stroke: black,
            width: 1.0,
        }
    }
}

/// 有限の正しい成分数だけを、Device色の定義域へclipする。
fn color(args: Option<&[f32]>, count: usize) -> Option<Color> {
    let args = args.filter(|v| v.len() == count && v.iter().all(|n| n.is_finite()))?;
    let mut values = [0.; 4];
    for (out, input) in values.iter_mut().zip(args) {
        *out = input.clamp(0., 1.);
    }
    Some(Color { count, values })
}

impl State {
    /// stroke/nonstrokeを独立に更新し、未知の色命令から同色を推測しない。
    pub(crate) fn update(&mut self, word: &[u8], args: Option<&[f32]>) {
        match word {
            b"g" => self.fill = color(args, 1),
            b"G" => self.stroke = color(args, 1),
            b"rg" => self.fill = color(args, 3),
            b"RG" => self.stroke = color(args, 3),
            b"k" => self.fill = color(args, 4),
            b"K" => self.stroke = color(args, 4),
            b"cs" | b"sc" | b"scn" => self.fill = None,
            b"CS" | b"SC" | b"SCN" => self.stroke = None,
            b"w" => {
                self.width = match args {
                    Some([v]) if v.is_finite() && *v > 0. => *v,
                    _ => f32::NAN,
                }
            }
            _ => {}
        }
    }

    /// 一様なCTMでの正の実効線幅だけを返し、fontのweight値は変更しない。
    pub(crate) fn same_color_width(self, ctm: [f32; 6]) -> Option<f32> {
        let sx = ctm[0].hypot(ctm[1]);
        let sy = ctm[2].hypot(ctm[3]);
        let dot = ctm[0] * ctm[2] + ctm[1] * ctm[3];
        let valid = self.fill.is_some()
            && self.fill == self.stroke
            && sx.is_finite()
            && sy.is_finite()
            && sx > 0.
            && sy > 0.
            && (sx - sy).abs() <= sx.max(sy) * 0.0001
            && dot.abs() <= sx * sy * 0.0001;
        let width = self.width * sy;
        (valid && width.is_finite() && width > 0.).then_some(width)
    }
}
