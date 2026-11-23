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
}

/// 原本PDFから読み始め、独立metadataを入力へ混ぜずに全codeの対応・幅を得る。
#[test]
#[ignore = "reviewed original PDF and resource gold require JADE_SOURCE_PDF and JADE_TYPE1_OUTLINE_GOLD"]
fn original_pdf_resources_match_all_independent_code_widths() {
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
    assert!(budget.check().is_ok());
}
