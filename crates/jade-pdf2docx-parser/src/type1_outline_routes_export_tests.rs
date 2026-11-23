//! 所有FontFileからの実出力を独立シェーパーへ渡し、同一文字列の外形と幅を区別する。
use super::*;
use crate::type1_outline::{
    bounds, cff, charstring, file, gsub, opentype, plan, resource::Slot, type2, unicode::Mapping,
};

/// 所有の元program・幅・Unicodeから、三つの実fontを生成する。
fn owned_fonts() -> Vec<(String, Vec<u8>)> {
    let budget = DecodeBudget::default();
    let (source, first, second, _) = crate::type1_cluster_embedding_tests::font_program();
    let original = file::read(&budget, &source, first, second).unwrap();
    assert_eq!(
        original
            .glyphs
            .iter()
            .map(|g| g.name.as_slice())
            .collect::<Vec<_>>(),
        [b".notdef".as_slice(), b"f", b"ff"]
    );
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
    for (code, glyph, width, text) in [
        (65, 1, 300., vec![102]),
        (66, 2, 500., vec![102, 102]),
        (67, 1, 450., vec![102]),
    ] {
        mappings[code] = Mapping {
            source: Slot {
                glyph: Some(glyph),
                width,
                explicit_width: true,
            },
            scalars: Some(text),
            from_tounicode: true,
        };
    }
    let plan = plan::build(&budget, &original, mappings).unwrap();
    let routes = build(&budget, &plan).unwrap();
    assert_eq!(
        (routes.codes[65], routes.codes[66], routes.codes[67]),
        (Some(0), Some(1), Some(2))
    );
    assert_eq!(routes.faces.len(), 3);
    let outlines = plan
        .glyphs
        .iter()
        .map(|entry| {
            let original = &original.glyphs[usize::from(entry.original)];
            charstring::Glyph {
                advance: entry.pdf_width.unwrap_or(original.outline.advance),
                commands: original.outline.commands.clone(),
            }
        })
        .collect::<Vec<_>>();
    let programs = outlines
        .iter()
        .map(|g| type2::encode(&budget, g).unwrap())
        .collect::<Vec<_>>();
    let metrics = outlines
        .iter()
        .map(|g| bounds::measure(&budget, g).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        metrics.iter().map(|m| m.advance).collect::<Vec<_>>(),
        [600, 300, 500, 450]
    );
    let names = [b".notdef".as_slice(), b"f", b"ff", b"JadeWidth3"];
    let glyphs = names
        .iter()
        .zip(&programs)
        .map(|(&name, program)| cff::Entry { name, program })
        .collect::<Vec<_>>();
    let mut output = Vec::new();
    for (index, face) in routes.faces.iter().enumerate() {
        let name = format!("JadeOwnedClusterRoute{index}");
        let font = cff::Font {
            units_per_em: 1000,
            name: name.as_bytes(),
            notice: b"Copyright Jade owned; owned test outlines may be embedded and modified.",
            bbox: [0, 0, 500, 700],
            italic_angle: 0.,
            is_fixed_pitch: false,
            glyphs: &glyphs,
        };
        let info = opentype::Info {
            metrics: &metrics,
            mappings: &face.mappings,
            ascent: 700,
            descent: 0,
            line_gap: 0,
            weight: 400,
            bold: false,
            italic: false,
            embedding_flags: 0,
            underline_position: -100,
            underline_thickness: 50,
            x_height: 0,
            cap_height: 700,
            license: "Copyright Jade owned; owned test outlines may be embedded and modified.",
            license_url: "",
        };
        let rules = face.ligature.map(|glyph| gsub::Rule {
            components: &face.components,
            glyph,
        });
        let bytes =
            opentype::encode_with_ligatures(&budget, &font, &info, rules.as_slice()).unwrap();
        output.push((name, bytes));
    }
    assert!(budget.check().is_ok());
    output
}

/// 通常font・元合字・幅違いfontが、同じ所有原本外形を含む実OTTOになる。
#[test]
fn original_fontfile_routes_produce_three_complete_fonts() {
    let fonts = owned_fonts();
    assert_eq!(fonts.len(), 3);
    for (_, font) in fonts {
        assert_eq!(font.get(..4), Some(b"OTTO".as_slice()));
    }
}

/// 一度だけ保存し、原本code由来のfont選択を独立HarfBuzzへ検査させる。
#[test]
#[ignore = "new JADE_OWNED_CLUSTER_EXPORT_DIR required for independent shaping"]
fn export_owned_source_cluster_fonts() {
    let directory =
        std::path::PathBuf::from(std::env::var_os("JADE_OWNED_CLUSTER_EXPORT_DIR").unwrap());
    std::fs::create_dir(&directory).unwrap();
    for (name, bytes) in owned_fonts() {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("{name}.otf")))
            .unwrap();
        std::io::Write::write_all(&mut file, &bytes).unwrap();
    }
}
