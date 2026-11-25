//! SDKのglyphを一度だけ観察し、幾何検索の曖昧さをweightの推測に使わない。
use crate::{ParseError, decode::DecodeBudget, metadata_workspace, pdfium_font_tags::Tagged};
use pdfium_render::prelude::*;

/// weight確認用の最小glyph情報。文字本文やSDK objectは複製しない。
struct Glyph {
    /// SDKのloose bounds。左・下・右・上の順で保持する。
    bounds: [f32; 4],
    /// 元resourceと実埋め込みを確認した値。未確認glyphも境界判定へ残す。
    weight: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 正面積で重なる全glyphが同値の場合だけ確認し、未知・混在を除く。
    #[test]
    fn overlapping_weights_must_be_confirmed_and_uniform() {
        let budget = DecodeBudget::default();
        let rect = PdfRect::new_from_values(0., 0., 10., 10.);
        for other in [Some(400), None] {
            let index = Index {
                usable: true,
                glyphs: vec![
                    Glyph {
                        bounds: [1., 1., 3., 3.],
                        weight: Some(700),
                    },
                    Glyph {
                        bounds: [2., 2., 4., 4.],
                        weight: other,
                    },
                ],
            };
            assert_eq!(index.weight(&budget, rect).unwrap(), None);
        }
        let index = Index {
            usable: true,
            glyphs: vec![
                Glyph {
                    bounds: [1., 1., 3., 3.],
                    weight: Some(700),
                },
                Glyph {
                    bounds: [11., 11., 12., 12.],
                    weight: None,
                },
            ],
        };
        assert_eq!(index.weight(&budget, rect).unwrap(), Some(700));
    }

    /// 面積0の未知glyphも矩形内にあれば無視せず、隣の既知glyphで上書きしない。
    #[test]
    fn zero_area_unknown_glyph_blocks_confirmation() {
        let index = Index {
            usable: true,
            glyphs: vec![
                Glyph {
                    bounds: [1., 1., 3., 3.],
                    weight: Some(700),
                },
                Glyph {
                    bounds: [2., 2., 2., 2.],
                    weight: None,
                },
            ],
        };
        assert_eq!(
            index
                .weight(
                    &DecodeBudget::default(),
                    PdfRect::new_from_values(0., 0., 10., 10.)
                )
                .unwrap(),
            None
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        /// ゼロ幅の未知glyphは位置や宣言weightにかかわらず確認済みへ吸収しない。
        #[test]
        fn zero_width_unknown_glyph_never_inherits_weight(weight in 1u16..=1000, x in 1u16..99, y in 1u16..99) {
            let x=f32::from(x);let y=f32::from(y);
            let index=Index {usable:true,glyphs:vec![Glyph{bounds:[0.,0.,100.,100.],weight:Some(weight)},Glyph{bounds:[x,y,x,y+1.],weight:None}]};
            prop_assert_eq!(index.weight(&DecodeBudget::default(),PdfRect::new_from_values(0.,0.,100.,100.)).unwrap(),None);
        }
    }
}

/// ページ内の全非空白glyph。矩形ごとのSDK呼出しを増やさない。
pub(super) struct Index {
    /// 共有metadata予算で予約した観察値。
    glyphs: Vec<Glyph>,
    /// 幾何を確認できないglyphがあればページの範囲推定を使わない。
    usable: bool,
}

impl Index {
    /// 確認済み候補がある場合だけ、非空白glyphのboundsとsource weightを一度読む。
    pub(super) fn new(
        budget: &DecodeBudget,
        characters: &PdfPageTextChars<'_>,
        tags: &Tagged,
    ) -> Result<Self, ParseError> {
        metadata_workspace::ensure_active(budget)?;
        let mut result = Self {
            glyphs: Vec::new(),
            usable: true,
        };
        if !tags.has_weights() {
            return Ok(result);
        }
        result.glyphs = metadata_workspace::vector(budget, characters.len())?;
        for character in characters.iter() {
            metadata_workspace::ensure_active(budget)?;
            if char::from_u32(character.unicode_value()).is_some_and(char::is_whitespace) {
                continue;
            }
            let Ok(rect) = character.loose_bounds() else {
                result.usable = false;
                break;
            };
            let bounds = [
                rect.left().value,
                rect.bottom().value,
                rect.right().value,
                rect.top().value,
            ];
            if !bounds.iter().all(|v| v.is_finite()) {
                result.usable = false;
                break;
            }
            result.glyphs.push(Glyph {
                bounds,
                weight: tags.weight(&character.font_name()),
            });
        }
        Ok(result)
    }

    /// 正面積で重なる全glyphが同じ確認値を持つ矩形だけを採用する。
    pub(super) fn weight(
        &self,
        budget: &DecodeBudget,
        rect: PdfRect,
    ) -> Result<Option<u16>, ParseError> {
        metadata_workspace::ensure_active(budget)?;
        if !self.usable {
            return Ok(None);
        }
        let bounds = [
            rect.left().value,
            rect.bottom().value,
            rect.right().value,
            rect.top().value,
        ];
        if !bounds.iter().all(|v| v.is_finite()) {
            return Ok(None);
        }
        let mut weight = None;
        for (index, glyph) in self.glyphs.iter().enumerate() {
            if index % 256 == 0 {
                metadata_workspace::ensure_active(budget)?;
            }
            let b = glyph.bounds;
            let positive = bounds[0].max(b[0]) < bounds[2].min(b[2])
                && bounds[1].max(b[1]) < bounds[3].min(b[3]);
            let degenerate = (b[0] == b[2] || b[1] == b[3])
                && bounds[0].max(b[0]) <= bounds[2].min(b[2])
                && bounds[1].max(b[1]) <= bounds[3].min(b[3]);
            if positive || degenerate {
                let Some(value) = glyph.weight else {
                    return Ok(None);
                };
                if weight.is_some_and(|previous| previous != value) {
                    return Ok(None);
                }
                weight = Some(value);
            }
        }
        metadata_workspace::ensure_active(budget)?;
        Ok(weight)
    }
}
