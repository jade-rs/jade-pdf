//! OpenType writerの検査用に、元外形と独立metricを組み合わせて出力する。
use super::*;
use crate::type1_outline::{bounds, cff, cmap, opentype, scale, type2};
use std::path::Path;

/// 独立した所有glyphの外形情報とlayout情報。
#[derive(Deserialize)]
pub(super) struct Owned {
    /// 元のCFF metadata。
    cff: CffMetadata,
    /// 独立して測ったlayout情報。
    info: Metadata,
}

/// writerへ供給する独立したmetadata fixture。productのmetadata抽出とは区別する。
#[derive(Deserialize)]
pub(super) struct Metadata {
    /// GID順の独立glyph metric。
    metrics: Vec<ReferenceMetric>,
    /// component検査用に宣言したUnicode/GID対応。
    mappings: Vec<(u32, u16)>,
    /// 元PDF descriptorのascent。
    ascent: i16,
    /// 元PDF descriptorのdescent。
    descent: i16,
    /// 明示的なline gap。
    line_gap: i16,
    /// 元のweightから得たclass。
    weight: u16,
    /// 元外形が太字か。
    bold: bool,
    /// 元外形が斜体か。
    italic: bool,
    /// 確認済みOFL/所有データに対応する埋め込みbit。
    embedding_flags: u16,
    /// 元の下線位置。
    underline_position: i16,
    /// 元の下線の太さ。
    underline_thickness: i16,
    /// 元のx-height、またはunknownの0。
    x_height: i16,
    /// 元のcap height、またはunknownの0。
    cap_height: i16,
    /// 省略しない元のnoticeとlicense。
    license: String,
    /// 元のlicense URL。
    license_url: String,
}

/// 独立解析器が測った一glyphのmetric。
#[derive(Deserialize)]
struct ReferenceMetric {
    /// 格納する水平advance。
    advance: u16,
    /// 実輪郭の左sidebearing。
    left: i16,
    /// 実輪郭を外向きに丸めた範囲。
    bounds: Option<[i16; 4]>,
}

/// 完成fontを一度だけ保存し、元のgeometryとmetricも比較用manifestに残す。
fn export(
    directory: &Path,
    source: &str,
    glyphs: Vec<(String, Glyph)>,
    metadata: &CffMetadata,
    layout: &Metadata,
) -> serde_json::Value {
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
    let font = cff::Font {
        units_per_em: metadata.units_per_em,
        name: metadata.name.as_bytes(),
        notice: &metadata.notice,
        bbox: metadata.bbox,
        italic_angle: metadata.italic_angle,
        is_fixed_pitch: metadata.is_fixed_pitch,
        glyphs: &entries,
    };
    assert_eq!(glyphs.len(), layout.metrics.len());
    let metrics = glyphs
        .iter()
        .zip(&layout.metrics)
        .map(|((name, glyph), expected)| {
            let actual = bounds::measure(&budget, glyph).unwrap();
            assert_eq!(actual.advance, expected.advance, "{source} {name}");
            assert_eq!(actual.left, expected.left, "{source} {name}");
            assert_eq!(actual.bounds, expected.bounds, "{source} {name}");
            actual
        })
        .collect::<Vec<_>>();
    let mappings = layout
        .mappings
        .iter()
        .map(|&(scalar, glyph)| cmap::Mapping { scalar, glyph })
        .collect::<Vec<_>>();
    let info = opentype::Info {
        metrics: &metrics,
        mappings: &mappings,
        ascent: layout.ascent,
        descent: layout.descent,
        line_gap: layout.line_gap,
        weight: layout.weight,
        bold: layout.bold,
        italic: layout.italic,
        embedding_flags: layout.embedding_flags,
        underline_position: layout.underline_position,
        underline_thickness: layout.underline_thickness,
        x_height: layout.x_height,
        cap_height: layout.cap_height,
        license: &layout.license,
        license_url: &layout.license_url,
    };
    let bytes = opentype::encode(&budget, &font, &info).unwrap();
    let filename = format!("{}.otf", metadata.name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(&filename))
        .unwrap();
    std::io::Write::write_all(&mut file, &bytes).unwrap();
    assert!(budget.check().is_ok());
    let records = glyphs
        .iter()
        .map(|(name, g)| super::type2_export::record(source, name, g))
        .collect::<Vec<_>>();
    serde_json::json!({"path":filename,"source":source,"name":metadata.name,"units_per_em":metadata.units_per_em,"glyphs":records})
}

/// 元PDF幅と独立した単位選択の期待値。
#[derive(Deserialize)]
pub(super) struct ScaleProfile {
    /// GID順の元PDF幅。未指定glyphは元programの幅を使う。
    widths: Vec<Option<f64>>,
    /// 独立した有理数演算で選んだem単位数。
    expected_units: u16,
    /// 元の独立geometryから計算した値の上限。
    limits: ReferenceLimits,
}

/// 元の1000単位系で独立して求めた値の上限。
#[derive(Deserialize)]
struct ReferenceLimits {
    /// 絶対座標の上限。
    coordinate: f64,
    /// 連続点の差分上限。
    delta: f64,
    /// header等のmetric上限。
    metric: f64,
    /// 元programの幅上限。
    advance: f64,
}

/// PDF幅と外形をproduct helperで変換し、独立metricへ一致するOpenTypeを渡す。
#[test]
#[ignore = "reviewed original PDF-width gold and new JADE_OTF_EXPORT_DIR required"]
fn export_pdf_width_scaled_fonts_for_independent_reader() {
    let input = std::env::var("JADE_TYPE1_OUTLINE_GOLD").unwrap();
    let output = std::env::var("JADE_OTF_EXPORT_DIR").unwrap();
    let directory = Path::new(&output);
    let bytes = std::fs::read(input).unwrap();
    assert!(bytes.len() <= 8 * 1024 * 1024);
    let gold: Gold = serde_json::from_slice(&bytes).unwrap();
    std::fs::create_dir(directory).unwrap();
    let budget = DecodeBudget::default();
    let mut fonts = Vec::new();
    for font in gold.fonts {
        let original = compare_original(&font);
        let profile = font.pdf_scale.as_ref().unwrap();
        assert_eq!(original.len(), profile.widths.len());
        let widths = profile.widths.iter().flatten().copied().collect::<Vec<_>>();
        let limits = scale::Limits {
            coordinate: profile.limits.coordinate,
            delta: profile.limits.delta,
            metric: profile.limits.metric,
            advance: profile.limits.advance,
        };
        let units = scale::choose_units(&budget, &widths, &limits).unwrap();
        assert_eq!(units, profile.expected_units, "{}", font.source_name);
        assert_eq!(units, font.cff.as_ref().unwrap().units_per_em);
        let glyphs = original
            .into_iter()
            .zip(&profile.widths)
            .map(|((name, glyph), &width)| {
                (name, scale::prepare(&budget, &glyph, width, units).unwrap())
            })
            .collect();
        fonts.push(export(
            directory,
            &font.source_name,
            glyphs,
            font.cff.as_ref().unwrap(),
            font.otf.as_ref().unwrap(),
        ));
    }
    assert!(budget.check().is_ok());
    let hash = hmac_sha256::Hash::hash(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
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

/// 元FontFileから復号した全外形を、独立metadataでOpenTypeとして検査へ渡す。
#[test]
#[ignore = "reviewed original FontFile/metric gold and new JADE_OTF_EXPORT_DIR required"]
fn export_complete_opentype_fonts_for_independent_reader() {
    let input = std::env::var("JADE_TYPE1_OUTLINE_GOLD").unwrap();
    let output = std::env::var("JADE_OTF_EXPORT_DIR").unwrap();
    let directory = Path::new(&output);
    let bytes = std::fs::read(input).unwrap();
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
            font.otf.as_ref().unwrap(),
        ));
    }
    let owned = gold.owned_otf.unwrap();
    let mut glyphs = vec![(
        ".notdef".to_string(),
        Glyph {
            advance: 0.,
            commands: vec![],
        },
    )];
    glyphs.extend(super::type2_export::owned_cases());
    fonts.push(export(directory, "owned", glyphs, &owned.cff, &owned.info));
    let hash = hmac_sha256::Hash::hash(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
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
