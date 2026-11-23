//! 出力の取り違えと予約名の再利用を、元幅・文字列・metadataの独立な変更で検査する。
use super::*;
use crate::{
    ParseLimits,
    type1_outline::{bounds, file, plan, resource::Slot, type2, unicode::Mapping},
};
use proptest::prelude::*;

/// 所有の二層programから外形を読み、変更前の入力をclosureへ渡す。
fn with_input(f: impl FnOnce(&[u8], &mut Plan, &cff::Font<'_>, &opentype::Info<'_>)) {
    let budget = DecodeBudget::default();
    let (program, first, second, _) = crate::type1_embedding_tests::program();
    let original = file::read(&budget, &program, first, second).unwrap();
    let mut mappings = (0..256)
        .map(|_| Mapping {
            source: Slot {
                glyph: None,
                width: 0.,
                explicit_width: false,
            },
            scalars: None,
            from_tounicode: false,
        })
        .collect::<Vec<_>>();
    mappings[65] = Mapping {
        source: Slot {
            glyph: Some(1),
            width: 600.,
            explicit_width: true,
        },
        scalars: Some(vec![65]),
        from_tounicode: true,
    };
    let mut plan = plan::build(&budget, &original, mappings).unwrap();
    let programs = original
        .glyphs
        .iter()
        .map(|g| type2::encode(&budget, &g.outline).unwrap())
        .collect::<Vec<_>>();
    let glyphs = original
        .glyphs
        .iter()
        .zip(&programs)
        .map(|(g, program)| cff::Entry {
            name: &g.name,
            program,
        })
        .collect::<Vec<_>>();
    let metrics = original
        .glyphs
        .iter()
        .map(|g| bounds::measure(&budget, &g.outline).unwrap())
        .collect::<Vec<_>>();
    let font = cff::Font {
        units_per_em: 1000,
        name: b"OriginalReservedFamily",
        notice: b"Copyright Jade owned; owned font permission",
        bbox: [0, 0, 600, 700],
        italic_angle: 0.,
        is_fixed_pitch: false,
        glyphs: &glyphs,
    };
    let cmap = [super::super::cmap::Mapping {
        scalar: 65,
        glyph: 1,
    }];
    let info = opentype::Info {
        metrics: &metrics,
        mappings: &cmap,
        ascent: 700,
        descent: -200,
        line_gap: 0,
        weight: 400,
        bold: false,
        italic: false,
        embedding_flags: 0,
        underline_position: -100,
        underline_thickness: 50,
        x_height: 450,
        cap_height: 700,
        license: "Copyright Jade owned; owned font permission",
        license_url: "",
    };
    f(&program, &mut plan, &font, &info);
}

/// 最大63byteのPostScript名へ全256bitを保ち、同じ入力は同じ実fontになる。
#[test]
fn complete_identity_renames_cff_and_name_tables_deterministically() {
    with_input(|program, plan, font, info| {
        let source = Source { program, plan };
        let budget = DecodeBudget::default();
        let first = opentype::encode_derived(&budget, &source, font, info, &[]).unwrap();
        let second = opentype::encode_derived(&budget, &source, font, info, &[]).unwrap();
        assert_eq!(first.family, second.family);
        assert_eq!(first.bytes, second.bytes);
        assert!(first.family.starts_with("JadeT1-"));
        assert_eq!(first.family.len(), 59);
        assert!(
            first.family[7..]
                .bytes()
                .all(|b| b.is_ascii_uppercase() || matches!(b, b'2'..=b'7'))
        );
        assert!(
            !first
                .bytes
                .windows(b"OriginalReservedFamily".len())
                .any(|w| w == b"OriginalReservedFamily")
        );
        assert!(
            first
                .bytes
                .windows(first.family.len())
                .any(|w| w == first.family.as_bytes())
        );
        let utf16 = first
            .family
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        assert!(first.bytes.windows(utf16.len()).any(|w| w == utf16));
        let renamed = cff::Font {
            name: b"OtherReservedName",
            ..*font
        };
        assert_eq!(
            first.family,
            derive(&budget, &source, &renamed, info, &[]).unwrap()
        );
    });
}

/// 同じ元programでも、空・未知・由来とcode順を含むPDF計画の差を隠さない。
#[test]
fn every_source_plan_difference_changes_the_effective_family() {
    with_input(|program, plan, font, info| {
        let budget = DecodeBudget::default();
        let original = derive(&budget, &Source { program, plan }, font, info, &[]).unwrap();
        plan.codes[65].mapping.source.width = 599.;
        assert_ne!(
            original,
            derive(&budget, &Source { program, plan }, font, info, &[]).unwrap()
        );
        plan.codes[65].mapping.source.width = 600.;
        plan.codes[65].mapping.source.explicit_width = false;
        assert_ne!(
            original,
            derive(&budget, &Source { program, plan }, font, info, &[]).unwrap()
        );
        plan.codes[65].mapping.source.explicit_width = true;
        plan.codes[65].mapping.from_tounicode = false;
        assert_ne!(
            original,
            derive(&budget, &Source { program, plan }, font, info, &[]).unwrap()
        );
        plan.codes[65].mapping.from_tounicode = true;
        let mut names = std::collections::BTreeSet::new();
        names.insert(original.clone());
        for text in [Some(vec![]), None, Some(vec![65, 65]), Some(vec![0x1f600])] {
            plan.codes[65].mapping.scalars = text;
            assert!(
                names.insert(derive(&budget, &Source { program, plan }, font, info, &[]).unwrap())
            );
        }
        plan.codes[65].mapping.scalars = Some(vec![65]);
        plan.codes.swap(65, 66);
        assert_ne!(
            original,
            derive(&budget, &Source { program, plan }, font, info, &[]).unwrap()
        );
        plan.codes.swap(65, 66);
        plan.glyphs[1].pdf_width = Some(600.);
        assert_ne!(
            original,
            derive(&budget, &Source { program, plan }, font, info, &[]).unwrap()
        );
    });
}

/// 同じ原本でも、権利全文・style・metric・文字対応・GSUBなど全出力情報を区別する。
#[test]
fn changed_output_metadata_never_reuses_the_previous_family() {
    with_input(|program, plan, font, info| {
        let budget = DecodeBudget::default();
        let source = Source { program, plan };
        let original = derive(&budget, &source, font, info, &[]).unwrap();
        for changed in [
            opentype::Info {
                ascent: 701,
                ..*info
            },
            opentype::Info {
                descent: -201,
                ..*info
            },
            opentype::Info {
                line_gap: 1,
                ..*info
            },
            opentype::Info {
                weight: 401,
                ..*info
            },
            opentype::Info {
                bold: true,
                ..*info
            },
            opentype::Info {
                italic: true,
                ..*info
            },
            opentype::Info {
                embedding_flags: 8,
                ..*info
            },
            opentype::Info {
                underline_position: -99,
                ..*info
            },
            opentype::Info {
                underline_thickness: 51,
                ..*info
            },
            opentype::Info {
                x_height: 451,
                ..*info
            },
            opentype::Info {
                cap_height: 701,
                ..*info
            },
            opentype::Info {
                license: "Other owned permission",
                ..*info
            },
            opentype::Info {
                license_url: "https://example.invalid/owned",
                ..*info
            },
        ] {
            assert_ne!(
                original,
                derive(&budget, &source, font, &changed, &[]).unwrap()
            );
        }
        for changed in [
            cff::Font {
                units_per_em: 1001,
                ..*font
            },
            cff::Font {
                notice: b"Other owned notice",
                ..*font
            },
            cff::Font {
                bbox: [0, 0, 601, 700],
                ..*font
            },
            cff::Font {
                italic_angle: -12.,
                ..*font
            },
            cff::Font {
                is_fixed_pitch: true,
                ..*font
            },
        ] {
            assert_ne!(
                original,
                derive(&budget, &source, &changed, info, &[]).unwrap()
            );
        }
        let mappings = [super::super::cmap::Mapping {
            scalar: 66,
            glyph: 1,
        }];
        assert_ne!(
            original,
            derive(
                &budget,
                &source,
                font,
                &opentype::Info {
                    mappings: &mappings,
                    ..*info
                },
                &[]
            )
            .unwrap()
        );
        let metrics = [
            opentype::Metric {
                advance: 600,
                left: 0,
                bounds: None,
            },
            opentype::Metric {
                advance: 599,
                left: 0,
                bounds: Some([0, 0, 600, 700]),
            },
        ];
        assert_ne!(
            original,
            derive(
                &budget,
                &source,
                font,
                &opentype::Info {
                    metrics: &metrics,
                    ..*info
                },
                &[]
            )
            .unwrap()
        );
        let rules = [gsub::Rule {
            components: &[1, 1],
            glyph: 1,
        }];
        assert_ne!(
            original,
            derive(&budget, &source, font, info, &rules).unwrap()
        );
    });
}

/// 名の予約に一byte不足した場合や中止時には、名前もfontも公開しない。
#[test]
fn identity_storage_is_exact_and_stopped_generation_is_atomic() {
    with_input(|program, plan, font, info| {
        let source = Source { program, plan };
        for limit in [59, 58] {
            let budget = DecodeBudget::new(&ParseLimits {
                max_total_embedded_font_bytes: limit,
                ..Default::default()
            });
            assert_eq!(
                derive(&budget, &source, font, info, &[]).is_some(),
                limit == 59
            );
        }
        let stopped = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: 0,
            ..Default::default()
        });
        assert!(stopped.font_vector::<u8>(1).is_none());
        assert!(opentype::encode_derived(&stopped, &source, font, info, &[]).is_none());
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 元programの任意の異なるsuffixを、同じfont名へまとめない。
    #[test]
    fn arbitrary_distinct_source_programs_keep_distinct_families(suffix in prop::collection::vec(any::<u8>(),0..256),change in any::<u8>()) {
        with_input(|program,plan,font,info| {
            let mut first=program.to_vec();first.extend_from_slice(&suffix);first.push(change);
            let mut second=first.clone();*second.last_mut().unwrap()^=1;
            let budget=DecodeBudget::default();
            assert_ne!(derive(&budget,&Source{program:&first,plan},font,info,&[]).unwrap(),derive(&budget,&Source{program:&second,plan},font,info,&[]).unwrap());
        });
    }
}
