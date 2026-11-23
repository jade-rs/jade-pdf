//! 正立した元glyph列を、測定済みadvanceと累積誤差を抑えるtwip字間へ変換する。
use super::*;

/// 一つの元glyphまたは論理空白へ付与する表示指定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Placement {
    /// 元のglyph形状に近い整数percentの水平倍率。
    pub(super) stretch: i32,
    /// 次の原点を保つ追加字間（twips）。
    pub(super) spacing: i32,
    /// source fontではなく所有空白fontを使うか。
    pub(super) logical_space: bool,
}

/// 行全体を確認できた場合だけ、元glyphの原点列から配置指定を返す。
pub(super) fn placements(
    _runs: &[StructuredTextRun],
    _fonts: &[ParsedEmbeddedFont],
) -> Option<Vec<Placement>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 所有fontのcmapだけをfへ変更し、位置演算用に既知の1000単位advanceを使う。
    fn owned() -> ParsedEmbeddedFont {
        let name = format!("JadeT1-{}", "A".repeat(52));
        let mut bytes = layout_space::font(&name);
        let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
        let offset = face
            .raw_face()
            .table_records
            .into_iter()
            .find(|r| r.tag == ttf_parser::Tag::from_bytes(b"cmap"))
            .unwrap()
            .offset as usize;
        for (at, value) in [(26, 102u16), (32, 102), (36, (-101i16) as u16)] {
            bytes[offset + at..offset + at + 2].copy_from_slice(&value.to_be_bytes());
        }
        ParsedEmbeddedFont {
            font_name: name,
            bytes,
        }
    }

    /// source boundsを保持した一つの正立glyphを作る。
    fn run(text: &str, x: f32, width: f32, name: Option<&str>) -> StructuredTextRun {
        StructuredTextRun {
            text: text.into(),
            style: StructuredTextStyle {
                font_name: name.map(str::to_string),
                font_embedded: name.is_some(),
                font_size: 10.,
                source_bounds: Some(ParsedBounds {
                    x0: x,
                    x1: x + width,
                    y0: 80.,
                    y1: 90.,
                }),
                ..Default::default()
            },
        }
    }

    /// TJ空白だけに専用fontを使い、元codeへU+0020を割り当てず実際のgapを保持する。
    #[test]
    fn logical_gap_uses_owned_space_advance() {
        let font = owned();
        let runs = [
            run("f", 20., 10., Some(&font.font_name)),
            run(" ", 30., 3.25, None),
            run("f", 33.25, 10., Some(&font.font_name)),
        ];
        assert_eq!(
            placements(&runs, &[font]).unwrap(),
            [
                Placement {
                    stretch: 100,
                    spacing: 0,
                    logical_space: false
                },
                Placement {
                    stretch: 100,
                    spacing: 45,
                    logical_space: true
                },
                Placement {
                    stretch: 100,
                    spacing: 0,
                    logical_space: false
                },
            ]
        );
    }

    /// 未確認glyph・原点・fontが一つでもあれば行全体の従来表示を保つ。
    #[test]
    fn incomplete_line_never_receives_partial_positioning() {
        let font = owned();
        let first = run("f", 20., 10., Some(&font.font_name));
        let mut invalids = vec![
            run("A", 30., 10., Some(&font.font_name)),
            run("ff", 30., 10., Some(&font.font_name)),
            run("f", 30., 10., None),
        ];
        let mut absent = first.clone();
        absent.style.source_bounds = None;
        invalids.push(absent);
        for invalid in invalids {
            assert!(placements(&[first.clone(), invalid], std::slice::from_ref(&font)).is_none());
        }
    }

    proptest! {
        /// 各glyphの丸め誤差が積み重ならず、すべての次原点が半twip以内に収まる。
        #[test]
        fn absolute_origins_bound_rounding_error(gaps in prop::collection::vec(-1.5f32..2.,1..40)) {
            let font=owned();let mut runs=Vec::new();let mut x=20.;
            for gap in gaps { runs.push(run("f",x,10.,Some(&font.font_name)));x+=10.+gap; }
            runs.push(run("f",x,10.,Some(&font.font_name)));
            let plan=placements(&runs,&[font]).unwrap();let mut actual=20f64;
            for (r,p) in runs.iter().zip(plan) {
                prop_assert!((actual-f64::from(r.style.source_bounds.unwrap().x0)).abs()<=0.0251);
                actual+=10.*f64::from(p.stretch)/100.+f64::from(p.spacing)/20.;
            }
        }
    }
}
