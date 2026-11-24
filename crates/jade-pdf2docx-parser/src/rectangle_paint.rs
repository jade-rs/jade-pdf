//! 長方形の罫線として保存できる、不透明なDevice黒と実線だけを確認する。
use crate::{metadata, text_draws::Operand};

/// q/Qで保持するstroke色・alpha・dashの確認状態。
#[derive(Clone, Copy)]
pub(super) struct Paint {
    /// Device色空間の成分数。未対応の空間ではNone。
    components: Option<usize>,
    /// Wordの黒罫線へ対応できる色か。
    black: bool,
    /// 通常合成のstroke alpha。不明な状態を後の部分gsで回復しない。
    opacity: Option<f64>,
    /// dash配列が空であることを確認できたか。
    solid: bool,
}

impl Default for Paint {
    /// PDF既定のDeviceGray黒、不透明、実線を返す。
    fn default() -> Self {
        Self {
            components: Some(1),
            black: true,
            opacity: Some(1.),
            solid: true,
        }
    }
}

impl Paint {
    /// 未解決gsの効果は保持し、確認済みCAだけを現在値へ反映する。
    pub(super) fn state(&mut self, parameters: Option<Option<f64>>) {
        match parameters {
            Some(Some(alpha)) if self.opacity.is_some() => self.opacity = Some(alpha),
            None => self.opacity = None,
            _ => {}
        }
    }

    /// strokeだけの色とdashを解釈し、fill色には影響されない。
    pub(super) fn apply(&mut self, word: &[u8], args: &[Operand<'_>]) {
        match word {
            b"G" | b"RG" | b"K" => {
                self.components = Some(match word {
                    b"G" => 1,
                    b"RG" => 3,
                    _ => 4,
                });
                self.color(args);
            }
            b"CS" => {
                self.components = match args {
                    [Operand::Name(name)]
                        if crate::metadata_names::resource_name_order(name, "DeviceGray")
                            .is_eq() =>
                    {
                        Some(1)
                    }
                    [Operand::Name(name)]
                        if crate::metadata_names::resource_name_order(name, "DeviceRGB")
                            .is_eq() =>
                    {
                        Some(3)
                    }
                    [Operand::Name(name)]
                        if crate::metadata_names::resource_name_order(name, "DeviceCMYK")
                            .is_eq() =>
                    {
                        Some(4)
                    }
                    _ => None,
                };
                self.black = false;
            }
            b"SC" | b"SCN" => self.color(args),
            b"d" => {
                self.solid = match args {
                    [Operand::Array(raw), Operand::Number(phase)] if phase.is_finite() => {
                        metadata::array_entries(raw, usize::MAX)
                            .is_some_and(|mut entries| entries.next().is_none())
                    }
                    _ => false,
                };
            }
            _ => {}
        }
    }

    /// PDFの成分clip後にDevice黒となる場合だけ採用する。
    fn color(&mut self, args: &[Operand<'_>]) {
        let mut values = [0.; 4];
        self.black = Some(args.len()) == self.components
            && args.len() <= 4
            && args.iter().zip(&mut values).all(|(arg, value)| {
                if let Operand::Number(number) = arg
                    && number.is_finite()
                {
                    *value = number.clamp(0., 1.);
                    true
                } else {
                    false
                }
            })
            && match self.components {
                Some(1) => values[0] == 0.,
                Some(3) => values[..3] == [0.; 3],
                Some(4) => values[3] == 1. || values[..3] == [1.; 3],
                _ => false,
            };
    }

    /// 通常の黒い実線を、色や透明度を推測せずに使えるかを返す。
    pub(super) fn supported(self) -> bool {
        self.black && self.solid && self.opacity == Some(1.)
    }
}
