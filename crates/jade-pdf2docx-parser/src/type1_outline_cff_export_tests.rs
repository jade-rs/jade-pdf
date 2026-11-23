//! 元FontFileの外形と許諾表示を、独立解析するためのCFFとして出力する。
use super::*;
use crate::type1_outline::{cff, type2};
use std::path::Path;

/// glyphを元の名前で格納し、検査用の元座標とmetadataも一緒に保存する。
fn export(
    directory: &Path,
    source: &str,
    glyphs: Vec<(String, Glyph)>,
    metadata: &CffMetadata,
) -> serde_json::Value {
    assert_eq!(glyphs[0].0, ".notdef");
    let budget = DecodeBudget::default();
    let mut programs = budget.font_vector(glyphs.len()).unwrap();
    for (_, glyph) in &glyphs {
        programs.push(type2::encode(&budget, glyph).unwrap());
    }
    let mut entries = budget.font_vector(glyphs.len()).unwrap();
    for ((name, _), program) in glyphs.iter().zip(&programs) {
        entries.push(cff::Entry {
            name: name.as_bytes(),
            program,
        });
    }
    let bytes = cff::encode(
        &budget,
        &cff::Font {
            name: metadata.name.as_bytes(),
            notice: &metadata.notice,
            bbox: metadata.bbox,
            italic_angle: metadata.italic_angle,
            is_fixed_pitch: metadata.is_fixed_pitch,
            glyphs: &entries,
        },
    )
    .unwrap();
    let filename = format!("{}.cff", metadata.name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(&filename))
        .unwrap();
    std::io::Write::write_all(&mut file, &bytes).unwrap();
    let records = glyphs
        .iter()
        .map(|(name, glyph)| super::type2_export::record(source, name, glyph))
        .collect::<Vec<_>>();
    assert!(budget.check().is_ok());
    serde_json::json!({"path":filename,"source":source,"name":metadata.name,"notice":metadata.notice,"bbox":metadata.bbox,"italic_angle":metadata.italic_angle,"is_fixed_pitch":metadata.is_fixed_pitch,"glyphs":records})
}

/// 完全に検証した元の351外形と所有glyphを、型付きwriterだけでCFFにする。
#[test]
#[ignore = "reviewed original FontFile metadata gold and new JADE_CFF_EXPORT_DIR required"]
fn export_cff_containers_for_independent_reader() {
    let path = std::env::var("JADE_TYPE1_OUTLINE_GOLD").unwrap();
    let output = std::env::var("JADE_CFF_EXPORT_DIR").unwrap();
    let directory = Path::new(&output);
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() <= 8 * 1024 * 1024);
    let gold: Gold = serde_json::from_slice(&bytes).unwrap();
    std::fs::create_dir(directory).unwrap();
    let mut fonts = Vec::new();
    for font in gold.fonts {
        let glyphs = compare_original(&font);
        fonts.push(export(
            directory,
            &font.source_name,
            glyphs,
            font.cff.as_ref().unwrap(),
        ));
    }
    assert_eq!(fonts.len(), 11);
    let mut glyphs = vec![(
        ".notdef".to_string(),
        Glyph {
            advance: 0.,
            commands: vec![],
        },
    )];
    glyphs.extend(super::type2_export::owned_cases());
    let metadata = CffMetadata {
        name: "JadeOwnedFractions".to_string(),
        notice: b"Jade owned qualification outlines; reproducible in memory".to_vec(),
        bbox: [0, 0, 1000, 1000],
        italic_angle: -9.46,
        is_fixed_pitch: false,
    };
    fonts.push(export(directory, "owned", glyphs, &metadata));
    let hash = hmac_sha256::Hash::hash(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("manifest.json"))
        .unwrap();
    serde_json::to_writer(
        file,
        &serde_json::json!({"source_gold_sha256":hash,"fonts":fonts}),
    )
    .unwrap();
}
