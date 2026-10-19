//! 文字描画モードの有限値とpaintの有無を共有する。

/// PDFのTrで選ぶ0から7の文字描画モード。
#[derive(Clone, Copy, Default)]
pub(crate) struct Mode {
    /// 既定値0はfillによる文字描画。
    value: u8,
}

impl Mode {
    /// 通常FontではTrのfill/stroke種別に対応するalphaが正の場合だけpaintする。
    /// Type3はglyph内部で状態を変更できるため従来のunsupported診断へ委ねる。
    pub(crate) fn paints_with_alpha(
        self,
        type3: bool,
        pdf20: bool,
        alpha: crate::text_alpha::State,
    ) -> bool {
        self.paints_for(type3, pdf20)
            && if type3 {
                true
            } else {
                match self.value {
                    0 | 4 => alpha.fill > 0.,
                    1 | 5 => alpha.stroke > 0.,
                    2 | 6 => alpha.fill > 0. || alpha.stroke > 0.,
                    _ => false,
                }
            }
    }
    /// 範囲内の整数operandだけを描画モードとして受け入れる。
    pub(crate) fn from_operand(value: f32) -> Option<Self> {
        (value.is_finite() && (0. ..=7.).contains(&value) && value.fract() == 0.)
            .then_some(Self { value: value as u8 })
    }

    /// paintを行わない不可視モード3・clipだけのモード7を除外する。
    pub(crate) fn paints(self) -> bool {
        self.paints_for(false, false)
    }

    /// Type3のモード7はPDF 1.xで描画され、PDF 2.0以降では不可視となる。
    pub(crate) fn paints_for(self, type3: bool, pdf20: bool) -> bool {
        self.value != 3 && (self.value != 7 || type3 && !pdf20)
    }

    /// 通常Fontのclipと、実効バージョンで描画されるType3だけを診断する。
    pub(crate) fn uses_glyph_shape_for(self, type3: bool, pdf20: bool) -> bool {
        self.value != 3 && !(type3 && pdf20 && self.value == 7)
    }
}
