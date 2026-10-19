//! 文字の局所矩形を合成行列で変換し、Form の可視領域へ切り抜く。
use crate::{ParsedBounds, decode::DecodeBudget, form_clip::Clip, parser};

/// 一つの文字描画に適用する座標変換と呼び出し領域。
#[derive(Clone, Copy)]
pub(crate) struct TextGeometry<'a> {
    /// 本文または文字空間からページ空間への合成行列。
    pub(crate) matrix: [f32; 6],
    /// Form 呼び出し時に固定した領域。ページ本文は未指定。
    clip: Option<&'a Clip>,
    /// 通常pathの描画時点のclip。
    path_clip: Option<&'a Clip>,
}

impl<'a> TextGeometry<'a> {
    /// 全体矩形を含まないclipがあり、glyph単位の切り抜きが必要かを返す。
    pub(crate) fn needs_glyph_clip(&self, left: f32, right: f32, height: f32) -> bool {
        self.clip
            .into_iter()
            .chain(self.path_clip)
            .any(|clip| !clip.contains_rect([left, 0., right, height], self.matrix))
    }

    /// 各文字描画の位置で有効な行列と Form の領域を保持する。
    pub(crate) fn new(matrix: [f32; 6], clip: Option<&'a Clip>) -> Self {
        Self {
            matrix,
            clip,
            path_clip: None,
        }
    }

    /// 描画時点の通常path clipを追加する。
    pub(crate) fn clipped(mut self, clip: Option<&'a Clip>) -> Self {
        self.path_clip = clip;
        self
    }

    /// 文字行列を本文の CTM と合成する。
    pub(crate) fn transformed(self, text_matrix: [f32; 6]) -> Self {
        Self {
            matrix: parser::concat_matrix(self.matrix, text_matrix),
            clip: self.clip,
            path_clip: self.path_clip,
        }
    }

    /// 合成後の文字縦軸の倍率を返す。
    pub(crate) fn vertical_scale(&self) -> f32 {
        self.matrix[2].hypot(self.matrix[3])
    }

    /// 局所矩形の四隅を変換し、交差後に外接 bounds を求める。
    pub(crate) fn bounds(
        &self,
        budget: &DecodeBudget,
        width: f32,
        height: f32,
    ) -> Option<ParsedBounds> {
        if budget.is_stopped() {
            return None;
        }
        match self.clip.or(self.path_clip) {
            Some(clip) => clip.transformed_bounds_with(
                budget,
                [0., 0., width, height],
                self.matrix,
                self.clip.and(self.path_clip),
            ),
            None => {
                // 通常ページの罫線は局所・上原点の旧座標系を含むため、
                // 軸平行の縦反転文字だけは従来の正方向高さに合わせる。
                // Form はページ空間の多角形 clip を使い、この補正を適用しない。
                let height = if self.matrix[1] == 0. && self.matrix[2] == 0. && self.matrix[3] < 0.
                {
                    -height
                } else {
                    height
                };
                Some(parser::transform_rect_bounds(
                    self.matrix,
                    0.,
                    0.,
                    width,
                    height,
                ))
            }
        }
    }
}
