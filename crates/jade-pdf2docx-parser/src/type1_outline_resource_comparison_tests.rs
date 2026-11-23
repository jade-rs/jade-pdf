//! 原本PDFの有効辞書・元FontFileから、全codeの対応と幅を独立goldへ照合する。
use super::*;
use crate::{metadata, parser, resource_resolver::ResourceResolver};

/// 独立解析器による、一つの原本PDF font resourceの期待値。
#[derive(Deserialize)]
pub(super) struct Metadata {
    /// 原本のfont辞書のobject番号。
    object_number: u32,
    /// 原本のfont辞書の有効世代。
    generation: u16,
    /// 256code順の原本対応。
    slots: Vec<ExpectedSlot>,
    /// UnicodeのNoneも意図的に宣言した独立goldか。
    #[serde(default)]
    unicode_reference_complete: bool,
}

/// GIDの番号ではなく、元のglyph名で対応の正しさを比較する。
#[derive(Deserialize)]
struct ExpectedSlot {
    /// 元Encodingが指定するglyph名。
    name: String,
    /// 元CharStrings内に対応するglyphが実在するか。
    present: bool,
    /// 元Widths、またはMissingWidthの幅。
    width: f64,
    /// 元Widths内の明示的な値か。
    explicit_width: bool,
    /// 原本ToUnicode、または確認済み名前のAGLから得た未正規化の列。
    #[serde(default)]
    scalars: Option<Vec<u32>>,
    /// 原本ToUnicodeの明示対応か。
    #[serde(default)]
    from_tounicode: bool,
}

/// 原本PDFから読み始め、独立metadataを入力へ混ぜずに全codeの対応・幅を得る。
#[test]
#[ignore = "reviewed original PDF and resource gold require JADE_SOURCE_PDF and JADE_TYPE1_OUTLINE_GOLD"]
fn original_pdf_resources_match_all_independent_code_widths() {
    compare(false, false);
}

/// 原本PDFのToUnicodeを実際に読み、文字列の正規化なしで全codeへ結び付ける。
#[test]
#[ignore = "reviewed original PDF and Unicode gold require JADE_SOURCE_PDF and JADE_TYPE1_OUTLINE_GOLD"]
fn original_pdf_resources_match_all_independent_unicode_sequences() {
    compare(true, false);
}

/// 全原本glyphと各codeの幅を残す計画を、原本PDFと未変更の独立goldへ照合する。
#[test]
#[ignore = "reviewed PDF/Unicode gold and new JADE_TYPE1_PLAN_REPORT required"]
fn original_pdf_plans_preserve_every_code_width_and_sequence() {
    compare(true, true);
}

/// 原本を使う共通経路で、任意でUnicodeの明示性・空文字・列全体も検査する。
fn compare(check_unicode: bool, check_plan: bool) {
    let gold_bytes = std::fs::read(std::env::var("JADE_TYPE1_OUTLINE_GOLD").unwrap()).unwrap();
    assert!(gold_bytes.len() <= 8 * 1024 * 1024);
    let gold: Gold = serde_json::from_slice(&gold_bytes).unwrap();
    let pdf = std::fs::read(std::env::var("JADE_SOURCE_PDF").unwrap()).unwrap();
    let hash = hmac_sha256::Hash::hash(&pdf)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(Some(hash), gold.source_pdf_sha256);
    let budget = DecodeBudget::default();
    let source = parser::parser_source_with_compressed_objects(&budget, &pdf);
    let objects = crate::object_index::build(&budget, &source).unwrap();
    let mut resolver = ResourceResolver::new(&objects, &budget);
    let offsets = parser::stream_object_offsets(&budget, &pdf);
    let mut count = 0;
    let mut explicit = 0;
    let (mut mapped, mut unicode_explicit, mut multi) = (0, 0, 0);
    let mut plans = Vec::new();
    for font in gold.fonts {
        let reference = font.pdf_resource.as_ref().unwrap();
        let &(generation, dictionary) = objects.get(&reference.object_number).unwrap();
        assert_eq!(generation, reference.generation);
        let (descriptor, _) = resolver
            .resolve(metadata::value(dictionary, b"FontDescriptor").unwrap())
            .unwrap();
        let (stream, number) = resolver
            .resolve(metadata::value(descriptor, b"FontFile").unwrap())
            .unwrap();
        let length1 = crate::syntax::integer(
            resolver
                .resolve(metadata::value(stream, b"Length1").unwrap())
                .unwrap()
                .0,
        )
        .unwrap();
        let length2 = crate::syntax::integer(
            resolver
                .resolve(metadata::value(stream, b"Length2").unwrap())
                .unwrap()
                .0,
        )
        .unwrap();
        let payload =
            parser::embedded_font_payload(&budget, &pdf, number.unwrap(), &offsets).unwrap();
        assert_eq!(payload, font.fontfile);
        let decoded =
            crate::type1_outline::file::read(&budget, &payload, length1, length2).unwrap();
        let slots =
            crate::type1_outline::resource::read(&budget, &decoded, dictionary, &mut resolver)
                .unwrap();
        assert_eq!(slots.len(), reference.slots.len());
        assert_eq!(slots.len(), 256);
        if check_unicode {
            assert!(reference.unicode_reference_complete);
            let original = match metadata::value(dictionary, b"ToUnicode") {
                Some(raw) => {
                    let (_, number) = resolver.resolve(raw).unwrap();
                    parser::font_cmap_for_object_number(&budget, &pdf, number.unwrap(), &offsets)
                        .unwrap()
                }
                None => crate::text::ToUnicodeCMap::default(),
            };
            let unicode =
                crate::type1_outline::unicode::map(&budget, &decoded, &slots, &original).unwrap();
            for (code, (actual, expected)) in unicode.iter().zip(&reference.slots).enumerate() {
                assert_eq!(
                    actual.scalars, expected.scalars,
                    "{} code{code}",
                    font.source_name
                );
                assert_eq!(
                    actual.from_tounicode, expected.from_tounicode,
                    "{} code{code}",
                    font.source_name
                );
                assert_eq!(
                    actual.source.width, expected.width,
                    "{} code{code}",
                    font.source_name
                );
                if actual.scalars.is_some() {
                    mapped += 1;
                }
                if actual.from_tounicode {
                    unicode_explicit += 1;
                }
                if actual.scalars.as_ref().is_some_and(|text| text.len() > 1) {
                    multi += 1;
                }
            }
            if check_plan {
                let plan = crate::type1_outline::plan::build(&budget, &decoded, unicode).unwrap();
                for (gid, original) in decoded.glyphs.iter().enumerate() {
                    assert_eq!(usize::from(plan.glyphs[gid].original), gid);
                    assert!(plan.glyphs[gid].pdf_width.is_none());
                    assert!(
                        font.glyphs
                            .iter()
                            .any(|g| g.name.as_bytes() == original.name)
                    );
                }
                let mut rows = Vec::new();
                let mut unicode_glyphs = std::collections::BTreeMap::new();
                let mut conflicts = Vec::new();
                for (code, (actual, expected)) in
                    plan.codes.iter().zip(&reference.slots).enumerate()
                {
                    assert_eq!(actual.mapping.scalars, expected.scalars);
                    assert_eq!(actual.mapping.from_tounicode, expected.from_tounicode);
                    assert_eq!(actual.mapping.source.width, expected.width);
                    assert_eq!(
                        actual.mapping.source.explicit_width,
                        expected.explicit_width
                    );
                    let shape = actual.glyph.map(|gid| {
                        let binding = &plan.glyphs[usize::from(gid)];
                        let original = &decoded.glyphs[usize::from(binding.original)];
                        assert_eq!(original.name, expected.name.as_bytes());
                        let independent = font
                            .glyphs
                            .iter()
                            .find(|g| g.name == expected.name)
                            .unwrap();
                        let width = binding.pdf_width.unwrap_or(independent.advance);
                        assert!(
                            (width - expected.width).abs() <= 1e-7,
                            "{} code{code} {width} != {}",
                            font.source_name,
                            expected.width
                        );
                        (gid, binding.original, width)
                    });
                    assert_eq!(
                        shape.is_some(),
                        expected.present && expected.name != ".notdef"
                    );
                    if let (Some((gid, _, _)), Some(text)) = (shape, &actual.mapping.scalars)
                        && !text.is_empty()
                        && let Some(prior) = unicode_glyphs.insert(text.clone(), gid)
                        && prior != gid
                    {
                        conflicts.push(serde_json::json!({"code":code,"scalars":text,"first_glyph":prior,"second_glyph":gid}));
                    }
                    rows.push(serde_json::json!({"code":code,"shape":shape,"name":expected.name,"scalars":actual.mapping.scalars,"from_tounicode":actual.mapping.from_tounicode}));
                }
                println!(
                    "{}: {} original glyphs, {} planned glyphs; all256 code widths/sequences retained; {} Unicode/GID conflicts",
                    font.source_name,
                    decoded.glyphs.len(),
                    plan.glyphs.len(),
                    conflicts.len()
                );
                plans.push(serde_json::json!({"source":font.source_name,"original_glyphs":decoded.glyphs.len(),"planned_glyphs":plan.glyphs.len(),"unicode_glyph_conflicts":conflicts,"codes":rows}));
            }
        }
        for (code, (actual, expected)) in slots.iter().zip(&reference.slots).enumerate() {
            let name = actual
                .glyph
                .map(|gid| decoded.glyphs[usize::from(gid)].name.as_slice());
            assert_eq!(
                name,
                expected.present.then_some(expected.name.as_bytes()),
                "{} code{code}",
                font.source_name
            );
            assert_eq!(
                actual.width, expected.width,
                "{} code{code}",
                font.source_name
            );
            assert_eq!(
                actual.explicit_width, expected.explicit_width,
                "{} code{code}",
                font.source_name
            );
            count += 1;
            if expected.name != ".notdef" && expected.explicit_width {
                explicit += 1;
            }
        }
        println!(
            "{}: all256 original PDF code bindings and f64 widths match",
            font.source_name
        );
    }
    assert_eq!(count, 2816);
    assert_eq!(explicit, 340);
    if check_unicode {
        assert_eq!(mapped, 1023);
        assert_eq!(unicode_explicit, 1020);
        assert_eq!(multi, 20);
        println!(
            "all2816code Unicode states match;1020original ToUnicode,3AGL,20multi-scalar sequences"
        );
    }
    assert!(budget.check().is_ok());
    if check_plan {
        let path = std::env::var("JADE_TYPE1_PLAN_REPORT").unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        serde_json::to_writer_pretty(&mut file, &serde_json::json!({"fonts":plans,"scope":"Original PDF component plans only; font embedding, GSUB, source-cluster routing and rendered appearance remain unqualified","production_ready":false})).unwrap();
    }
}
