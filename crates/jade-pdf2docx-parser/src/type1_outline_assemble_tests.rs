//! 期待metadataを供給せず、所有FontFileと元PDF辞書だけから実fontを生成する。
use super::{DecodeBudget, ResourceResolver, ToUnicodeCMap};
use crate::type1_outline::{Input, Prepared, prepare};
use crate::{ParseLimits, page_attributes::Objects, text::parse_tounicode_body};

/// 所有PDFの実descriptorを再現し、共通予算で一resource全体を生成する。
fn owned(budget: &DecodeBudget, input: Input<'_>, dictionary: &[u8]) -> Option<Prepared> {
    let objects = Objects::default();
    let mut resolver = ResourceResolver::new(&objects, budget);
    prepare(
        budget,
        input,
        dictionary,
        b"<< /FontName /Owned /Ascent 700 /Descent 0 /CapHeight 700 /Flags 32 >>",
        &mut resolver,
    )
}

/// 最小の所有Type1を、字形・元幅・派生名のある一つのfontへ生成する。
#[test]
fn original_fontfile_and_pdf_resource_generate_a_complete_portable_font() {
    let (program, length1, length2, _) = crate::type1_embedding_tests::program();
    let cmap = ToUnicodeCMap::default();
    let result = owned(
        &DecodeBudget::default(),
        Input {
            program: &program,
            length1,
            length2,
            cmap: &cmap,
        },
        b"<< /Subtype /Type1 /FirstChar 65 /LastChar 65 /Widths [600] >>",
    )
    .unwrap();
    assert_eq!(result.fonts.len(), 1);
    assert!(result.fonts[0].font_name.starts_with("JadeT1-"));
    assert_eq!(result.fonts[0].bytes.get(..4), Some(b"OTTO".as_slice()));
    assert_eq!(result.family(65), Some(result.fonts[0].font_name.as_str()));
    assert!(result.family(64).is_none());
}

/// 同じ原文ffでも、一字合字・二字通常文字・幅違いを異なる実fontへ渡す。
#[test]
fn source_cluster_and_width_conflicts_produce_distinct_complete_fonts() {
    let (program, length1, length2, _) = crate::type1_cluster_embedding_tests::font_program();
    let cmap=parse_tounicode_body(&DecodeBudget::default(),b"1 begincodespacerange\n<00> <FF>\nendcodespacerange\n3 beginbfchar\n<41> <0066>\n<42> <00660066>\n<43> <0066>\nendbfchar");
    let result=owned(&DecodeBudget::default(),Input{program:&program,length1,length2,cmap:&cmap},b"<< /Subtype /Type1 /FirstChar 65 /LastChar 67 /Widths [300 500 450] /Encoding << /Differences [67 /f] >> >>").unwrap();
    assert_eq!(result.fonts.len(), 3);
    assert_ne!(result.family(65), result.family(66));
    assert_ne!(result.family(65), result.family(67));
    assert_ne!(result.family(66), result.family(67));
    for font in &result.fonts {
        assert_eq!(font.bytes.get(..4), Some(b"OTTO".as_slice()));
    }
    assert!(result.family(255).is_none());
}

/// 制限・拒否された元権利・壊れた宣言を、使える先頭fontだけへ縮めて公開しない。
#[test]
fn prohibited_broken_or_stopped_sources_do_not_publish_partial_fonts() {
    let (mut program, length1, length2, _) = crate::type1_embedding_tests::program();
    let cmap = ToUnicodeCMap::default();
    let dictionary = b"<< /Subtype /Type1 /FirstChar 65 /LastChar 65 /Widths [600] >>";
    let stopped = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 1,
        ..Default::default()
    });
    assert!(
        owned(
            &stopped,
            Input {
                program: &program,
                length1,
                length2,
                cmap: &cmap
            },
            dictionary
        )
        .is_none()
    );
    let position = program
        .windows(b"/FSType 0".len())
        .position(|p| p == b"/FSType 0")
        .unwrap()
        + b"/FSType ".len();
    program[position] = b'2';
    assert!(
        owned(
            &DecodeBudget::default(),
            Input {
                program: &program,
                length1,
                length2,
                cmap: &cmap
            },
            dictionary
        )
        .is_none()
    );
    program[position] = b'0';
    assert!(
        owned(
            &DecodeBudget::default(),
            Input {
                program: &program,
                length1: length1 - 1,
                length2,
                cmap: &cmap
            },
            dictionary
        )
        .is_none()
    );
}
