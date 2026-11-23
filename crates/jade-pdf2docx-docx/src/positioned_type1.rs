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
    runs: &[StructuredTextRun],
    fonts: &[ParsedEmbeddedFont],
) -> Option<Vec<Placement>> {
    let first = runs.first()?.style.source_bounds?;
    if runs.first()?.text.trim().is_empty() || runs.last()?.text.trim().is_empty() {
        return None;
    }
    let mut output = Vec::with_capacity(runs.len());
    let mut actual = 0f64;
    for (index, run) in runs.iter().enumerate() {
        let bounds = run.style.source_bounds?;
        if ![
            bounds.x0,
            bounds.x1,
            bounds.y0,
            bounds.y1,
            run.style.font_size,
        ]
        .iter()
        .all(|v| v.is_finite())
            || bounds.x1 <= bounds.x0
            || bounds.y0 != first.y0
            || bounds.y1 <= bounds.y0
        {
            return None;
        }
        let logical_space = run.text == " " && run.style.font_name.is_none();
        let (stretch, width) = if logical_space {
            (100, 1.)
        } else {
            let name = run.style.font_name.as_deref()?;
            if !run.style.font_embedded || !name.starts_with("JadeT1-") {
                return None;
            }
            let font = fonts.iter().find(|f| f.font_name == name)?;
            let half_points = pdf_font_size_to_half_points(run.style.font_size)?;
            let normal = glyph_metrics::advance(&font.bytes, &run.text)? * half_points as f64 / 2.;
            let percent = (f64::from(bounds.x1 - bounds.x0) / normal * 100.).round();
            if !normal.is_finite() || !(1. ..=600.).contains(&percent) {
                return None;
            }
            (percent as i32, normal * percent / 100.)
        };
        let target = if let Some(next) = runs.get(index + 1) {
            let next = next.style.source_bounds?;
            if next.x0 <= bounds.x0 {
                return None;
            }
            next.x0
        } else {
            bounds.x1
        };
        let spacing = ((f64::from(target - first.x0) - actual - width) * 20.).round();
        if !spacing.is_finite() || !(-31680. ..=31680.).contains(&spacing) {
            return None;
        }
        actual += width + spacing / 20.;
        output.push(Placement {
            stretch,
            spacing: spacing as i32,
            logical_space,
        });
    }
    Some(output)
}

/// 行全体の計測が成功してから、空白font選択と字間を一度だけ反映する。
pub(super) fn prepare(line: &mut StructuredTextLine, fonts: &[ParsedEmbeddedFont], family: &str) {
    let Some(plan) = placements(&line.runs, fonts) else {
        return;
    };
    for (run, placement) in line.runs.iter_mut().zip(plan) {
        if placement.logical_space {
            run.style.font_name = Some(family.to_owned());
            run.style.font_size = 1.;
            run.style.font_embedded = true;
            run.style.embedded_face = Some(embedded_face::Face::default());
        }
        run.style.placement = Some(placement);
    }
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

    /// 小さい負のTJはglyphを縮めず、次の原点への負字間として出す。
    #[test]
    fn negative_kerning_does_not_squash_glyph_shapes() {
        let font = owned();
        let runs = [
            run("f", 20., 10., Some(&font.font_name)),
            run("f", 29.5, 10., Some(&font.font_name)),
        ];
        let plan = placements(&runs, &[font]).unwrap();
        assert_eq!(
            plan[0],
            Placement {
                stretch: 100,
                spacing: -10,
                logical_space: false
            }
        );
        assert_eq!(plan[1].stretch, 100);
    }

    /// 実際のstructured変換とZIP梱包を通り、論理spaceが内部fontを選んで埋め込まれる。
    #[test]
    fn structured_package_embeds_the_selected_owned_space_font() {
        use std::io::{Cursor, Read};
        let font = owned();
        let runs = [
            run("f", 20., 10., Some(&font.font_name)),
            run(" ", 30., 3.25, None),
            run("f", 33.25, 10., Some(&font.font_name)),
        ];
        let mut parsed = jade_pdf2docx_parser::parse_pdf_bytes(
            "owned-positioned.pdf",
            b"%PDF-1.4\n1 0 obj << /Type /Page /MediaBox [0 0 200 200] >> endobj\n",
        )
        .unwrap();
        parsed.text_elements = runs
            .into_iter()
            .map(|r| ParsedTextElement {
                page: 0,
                baseline_y: Some(80.),
                bounds: r.style.source_bounds.unwrap(),
                font_size: 10.,
                font_name: r.style.font_name,
                fill_color: None,
                text: r.text,
            })
            .collect();
        parsed.embedded_fonts = vec![font];
        parsed.text_elements.push(ParsedTextElement {
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds {
                x0: 20.,
                x1: 70.,
                y0: 40.,
                y1: 50.,
            },
            font_size: 10.,
            font_name: Some("JadeLayoutSpace-0".into()),
            fill_color: None,
            text: "reserved source font".into(),
        });
        let docx = build_structured_docx_from_parsed(&parsed, &None, &[]).unwrap();
        let bytes = pack_docx_to_bytes(docx, &parsed.embedded_fonts).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut document = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut document)
            .unwrap();
        assert!(document.contains("JadeLayoutSpace-1"), "{document}");
        assert!(document.contains("JadeLayoutSpace-0"), "{document}");
        assert!(document.contains(r#"<w:spacing w:val="45""#), "{document}");
        let mut table = String::new();
        archive
            .by_name("word/fontTable.xml")
            .unwrap()
            .read_to_string(&mut table)
            .unwrap();
        assert!(table.contains("JadeLayoutSpace-1"), "{table}");
        assert!(!table.contains("JadeLayoutSpace-0"), "{table}");
        let mut settings = String::new();
        archive
            .by_name("word/settings.xml")
            .unwrap()
            .read_to_string(&mut settings)
            .unwrap();
        assert!(
            !settings.contains("balanceSingleByteDoubleByteWidth"),
            "{settings}"
        );
        assert!(settings.contains("useFELayout"));
        assert_eq!(
            archive
                .file_names()
                .filter(|n| n.ends_with(".odttf"))
                .count(),
            2
        );
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
