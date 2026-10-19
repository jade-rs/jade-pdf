//! stencilの塗り色を描画ごとに保持し、未対応の色・透明状態を推測しない。

/// 非stroke色の現在値と、scに必要なDevice色空間。
#[derive(Clone, Copy)]
pub(super) struct Paint {
    /// 対応色空間の成分数。未対応色空間ではNone。
    components: Option<usize>,
    /// 色成分をRGB8へ写した塗り色。
    rgb: Option<[u8; 3]>,
    /// ExtGStateの非stroke alpha。未知の描画状態ではNone。
    opacity: Option<f64>,
}

/// 対応済みExtGStateによる差分。省略されたalphaは現在値を保持する。
pub(super) struct Parameters {
    /// 明示された非stroke alpha。Noneは既存値の保持。
    pub(super) opacity: Option<f64>,
}

impl Default for Paint {
    /// PDFの既定であるDeviceGrayの黒・不透明を返す。
    fn default() -> Self {
        Self {
            components: Some(1),
            rgb: Some([0; 3]),
            opacity: Some(1.0),
        }
    }
}

impl Paint {
    /// 色と透明状態を分離し、通常画像へ未対応色を伝播しない。
    pub(super) fn snapshot(&self) -> super::ImagePaint {
        super::ImagePaint {
            color: self.rgb,
            opacity: self.opacity,
        }
    }

    /// 色operatorだけを解釈し、stroke色や本文operatorでは状態を変えない。
    pub(super) fn apply(&mut self, word: &[u8], values: &[f32], name: Option<&[u8]>) {
        match word {
            b"g" => {
                self.components = Some(1);
                self.set(values);
            }
            b"rg" => {
                self.components = Some(3);
                self.set(values);
            }
            b"k" => {
                self.components = None;
                self.rgb = None;
            }
            b"cs" => {
                self.components = match name {
                    Some(name)
                        if crate::metadata_names::resource_name_order(name, "DeviceGray")
                            .is_eq() =>
                    {
                        Some(1)
                    }
                    Some(name)
                        if crate::metadata_names::resource_name_order(name, "DeviceRGB")
                            .is_eq() =>
                    {
                        Some(3)
                    }
                    _ => None,
                };
                self.rgb = self.components.map(|_| [0; 3]);
            }
            b"sc" | b"scn" => {
                if name.is_some() {
                    self.rgb = None;
                } else {
                    self.set(values);
                }
            }
            _ => {}
        }
    }

    /// alphaは置換し、省略時は保持する。未知状態はq/Qで戻るまで保持する。
    pub(super) fn apply_state(&mut self, parameters: Option<Parameters>) {
        match parameters {
            Some(parameters) if self.opacity.is_some() => {
                if let Some(opacity) = parameters.opacity {
                    self.opacity = Some(opacity);
                }
            }
            None => self.opacity = None,
            _ => {}
        }
    }

    /// 成分数を検査し、Device色のclip・丸めを一度だけ適用する。
    fn set(&mut self, values: &[f32]) {
        self.rgb = match self.components {
            Some(1) if values.len() == 1 => {
                Some([crate::parser::pdf_color_component_to_u8(values[0]); 3])
            }
            Some(3) if values.len() == 3 => Some(
                [values[0], values[1], values[2]].map(crate::parser::pdf_color_component_to_u8),
            ),
            _ => None,
        };
    }
}
