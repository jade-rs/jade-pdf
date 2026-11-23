//! 合字表を実CFF fontへ接続し、同じ所有fontを独立シェーパーへ渡す。
use super::*;
use crate::type1_outline::{
    charstring::{Command, Glyph},
    gsub::Rule,
    type2,
};

/// f/iと三つの合字に別GID・幅を持たせた、完全に所有の描画用fontを作る。
fn owned_font(with_ligatures: bool) -> Vec<u8> {
    let budget = DecodeBudget::default();
    let widths = [600, 300, 200, 500, 450, 700];
    let names = [b".notdef".as_slice(), b"f", b"i", b"ff", b"fi", b"ffi"];
    let programs = widths
        .iter()
        .enumerate()
        .map(|(gid, &width)| {
            type2::encode(
                &budget,
                &Glyph {
                    advance: f64::from(width),
                    commands: if gid == 0 {
                        vec![]
                    } else {
                        vec![
                            Command::Move([0., 0.]),
                            Command::Line([300., 700.]),
                            Command::Line([600., 0.]),
                            Command::Close,
                        ]
                    },
                },
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let glyphs = names
        .iter()
        .zip(&programs)
        .map(|(&name, program)| cff::Entry { name, program })
        .collect::<Vec<_>>();
    let font = cff::Font {
        units_per_em: 1000,
        name: b"JadeOwnedLigature",
        notice: b"Copyright Jade owned; original geometry and permission retained",
        bbox: [0, 0, 600, 700],
        italic_angle: 0.,
        is_fixed_pitch: false,
        glyphs: &glyphs,
    };
    let metrics = widths
        .iter()
        .enumerate()
        .map(|(gid, &advance)| Metric {
            advance,
            left: 0,
            bounds: (gid != 0).then_some([0, 0, 600, 700]),
        })
        .collect::<Vec<_>>();
    let mappings = [
        cmap::Mapping {
            scalar: 0x66,
            glyph: 1,
        },
        cmap::Mapping {
            scalar: 0x69,
            glyph: 2,
        },
        cmap::Mapping {
            scalar: 0x1f600,
            glyph: 1,
        },
    ];
    let info = Info {
        metrics: &metrics,
        mappings: &mappings,
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
        license: "Jade owned original test outlines; permission retained",
        license_url: "https://example.invalid/owned",
    };
    let rules = [
        Rule {
            components: &[1, 1],
            glyph: 3,
        },
        Rule {
            components: &[1, 2],
            glyph: 4,
        },
        Rule {
            components: &[1, 1, 2],
            glyph: 5,
        },
    ];
    if with_ligatures {
        encode_with_ligatures(&budget, &font, &info, &rules).unwrap()
    } else {
        encode(&budget, &font, &info).unwrap()
    }
}

/// sfnt directoryの実offsetから指定tableを探す。
fn table<'a>(font: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    let count = u16::from_be_bytes(font[4..6].try_into().unwrap());
    (0..usize::from(count)).find_map(|index| {
        let row = 12 + 16 * index;
        if &font[row..row + 4] != tag {
            return None;
        }
        let start = u32::from_be_bytes(font[row + 8..row + 12].try_into().unwrap()) as usize;
        let size = u32::from_be_bytes(font[row + 12..row + 16].try_into().unwrap()) as usize;
        Some(&font[start..start + size])
    })
}

/// 従来の九tableは保ち、GSUBも独立checksumを持つ十番目のtableとして含める。
#[test]
fn complete_font_contains_required_ligatures_without_changing_cff_or_metrics() {
    let plain = owned_font(false);
    let combined = owned_font(true);
    assert_eq!(u16::from_be_bytes(combined[4..6].try_into().unwrap()), 10);
    assert!(table(&combined, b"GSUB").is_some());
    for tag in [
        b"CFF ", b"OS/2", b"cmap", b"hhea", b"hmtx", b"maxp", b"name", b"post",
    ] {
        assert_eq!(table(&combined, tag), table(&plain, tag));
    }
    let checksum = combined.chunks(4).fold(0u32, |sum, part| {
        let mut bytes = [0; 4];
        bytes[..part.len()].copy_from_slice(part);
        sum.wrapping_add(u32::from_be_bytes(bytes))
    });
    assert_eq!(checksum, 0xb1b0_afba);
}

/// 所有fontを一度だけ退避し、外部fontTools/HarfBuzzへ同じ実行結果を渡す。
#[test]
#[ignore = "new JADE_OWNED_GSUB_EXPORT_DIR required for independent shaping"]
fn export_owned_ligature_fonts_for_independent_shaping() {
    let directory =
        std::path::PathBuf::from(std::env::var_os("JADE_OWNED_GSUB_EXPORT_DIR").unwrap());
    std::fs::create_dir(&directory).unwrap();
    for (name, enabled) in [
        ("JadeOwnedLigature.otf", true),
        ("JadeOwnedUnshapedControl.otf", false),
    ] {
        let bytes = owned_font(enabled);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))
            .unwrap();
        std::io::Write::write_all(&mut file, &bytes).unwrap();
    }
}
