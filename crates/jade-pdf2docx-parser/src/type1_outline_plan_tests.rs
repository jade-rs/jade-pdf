//! 幅違いの外形参照・合字・未知codeを、独立した元幅モデルと照合する。
use super::*;
use crate::{
    ParseError, ParseLimits,
    type1_outline::{file, resource::Slot},
};
use proptest::prelude::*;

/// 所有の二層FontFileを読み、元の三角形と.notdefを渡す。
fn font() -> Font<'static> {
    let (source, first, second, _) = crate::type1_embedding_tests::program();
    // headerが借用する原本だけをテストの寿命へ残す。
    let source = Box::leak(source.into_boxed_slice());
    file::read(&DecodeBudget::default(), source, first, second).unwrap()
}

/// 元code順を変えない、未確認256slotの所有入力を作る。
fn mappings() -> Vec<Mapping> {
    (0..256)
        .map(|_| Mapping {
            source: Slot {
                glyph: None,
                width: 0.,
                explicit_width: false,
            },
            scalars: None,
            from_tounicode: false,
        })
        .collect()
}

/// 確認済みのglyph・幅・原本Unicode列を、一つのcodeへ指定する。
fn binding(glyph: Option<u16>, width: f64, scalars: Option<Vec<u32>>) -> Mapping {
    Mapping {
        source: Slot {
            glyph,
            width,
            explicit_width: true,
        },
        scalars,
        from_tounicode: true,
    }
}

/// 元glyphを全部残し、同じ外形でも幅の異なるcodeだけに別GIDを割り当てる。
#[test]
fn distinct_pdf_widths_do_not_overwrite_shared_original_glyph() {
    let original = font();
    let mut input = mappings();
    for (code, width) in [(65, 600.), (66, 450.), (67, 450.), (68, 675.)] {
        input[code] = binding(Some(1), width, Some(vec![code as u32]));
    }
    input[69] = binding(Some(0), 600., Some(vec![88]));
    input[70] = binding(None, 200., Some(vec![89]));
    let budget = DecodeBudget::default();
    let plan = build(&budget, &original, input).unwrap();
    assert_eq!(plan.glyphs.len(), 4);
    assert_eq!(
        plan.glyphs
            .iter()
            .map(|g| (g.original, g.pdf_width))
            .collect::<Vec<_>>(),
        [(0, None), (1, None), (1, Some(450.)), (1, Some(675.))]
    );
    assert_eq!(
        (65..=70).map(|c| plan.codes[c].glyph).collect::<Vec<_>>(),
        [Some(1), Some(2), Some(2), Some(3), None, None]
    );
    assert_eq!(plan.codes[69].mapping.source.glyph, Some(0));
    assert_eq!(plan.codes[70].mapping.source.glyph, None);
    assert_eq!(original.glyphs[1].outline.advance, 600.);
    assert!(budget.check().is_ok());
}

/// 同じUnicodeでも違う幅の外形と合字を残し、後段で衝突を解消できるようにする。
#[test]
fn unicode_sequences_empty_and_unknown_do_not_collapse_glyph_identity() {
    let original = font();
    let mut input = mappings();
    input[1] = binding(Some(1), 600., Some(vec![0x66, 0x66]));
    input[2] = binding(Some(1), 450., Some(vec![0x66, 0x66]));
    input[3] = binding(Some(1), 675., None);
    input[4] = binding(Some(1), 700., Some(vec![]));
    input[5] = binding(Some(1), 600., Some(vec![0x1f600]));
    input[3].from_tounicode = false;
    let plan = build(&DecodeBudget::default(), &original, input).unwrap();
    assert_eq!(plan.codes[1].mapping.scalars, Some(vec![0x66, 0x66]));
    assert_eq!(plan.codes[2].mapping.scalars, Some(vec![0x66, 0x66]));
    assert_ne!(plan.codes[1].glyph, plan.codes[2].glyph);
    assert_eq!(plan.codes[3].mapping.scalars, None);
    assert!(!plan.codes[3].mapping.from_tounicode);
    assert_eq!(plan.codes[4].mapping.scalars, Some(vec![]));
    assert_eq!(plan.codes[4].glyph, Some(4));
    assert_eq!(plan.codes[5].mapping.scalars, Some(vec![0x1f600]));
}

/// 明示のspace glyphが無いcode32から、空白のUnicodeや外形を作らない。
#[test]
fn missing_shape_with_positive_width_does_not_invent_a_space() {
    let original = font();
    let mut input = mappings();
    input[32] = binding(Some(0), 525., Some(vec![0x2423]));
    input[33] = binding(None, 277.8, None);
    let plan = build(&DecodeBudget::default(), &original, input).unwrap();
    assert_eq!(plan.glyphs.len(), 2);
    assert_eq!(plan.codes[32].glyph, None);
    assert_eq!(plan.codes[32].mapping.scalars, Some(vec![0x2423]));
    assert_eq!(plan.codes[32].mapping.source.width, 525.);
    assert_eq!(plan.codes[33].mapping.scalars, None);
}

/// 途中の不正slotや停止では、使える先頭codeだけを公開しない。
#[test]
fn invalid_or_stopped_inputs_never_publish_partial_plans() {
    let original = font();
    for invalid in [
        binding(Some(2), 600., Some(vec![65])),
        binding(Some(1), f64::NAN, Some(vec![65])),
        binding(Some(1), -1., Some(vec![65])),
        binding(Some(1), 600., Some(vec![0xd800])),
    ] {
        let mut input = mappings();
        input[65] = binding(Some(1), 600., Some(vec![65]));
        input[255] = invalid;
        assert!(build(&DecodeBudget::default(), &original, input).is_none());
    }
    let mut input = mappings();
    input.pop();
    assert!(build(&DecodeBudget::default(), &original, input).is_none());
    let stopped = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert!(stopped.font_vector::<u8>(1).is_none());
    assert!(build(&stopped, &original, mappings()).is_none());
}

/// 追加の外形参照とcode headerを正確に予約し、一byte不足でも部分出力しない。
#[test]
fn plan_storage_is_exact_and_atomic() {
    let original = font();
    let bytes = (3 * std::mem::size_of::<Glyph>() + 256 * std::mem::size_of::<Code>()) as u64;
    for allowed in [bytes, bytes - 1] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: allowed,
            ..Default::default()
        });
        let mut input = mappings();
        input[65] = binding(Some(1), 450., Some(vec![65]));
        let result = build(&budget, &original, input);
        if allowed == bytes {
            assert!(result.is_some());
            assert!(budget.check().is_ok());
        } else {
            assert!(result.is_none());
            assert!(
                matches!(budget.check(),Err(ParseError::ResourceLimit {resource:"embedded font bytes",limit,actual})if limit==allowed && actual==bytes)
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意の幅列で、元幅と同じcodeだけを元GIDへ戻し、幅違いの初出順を保つ。
    #[test]
    fn arbitrary_widths_keep_every_code_advance(widths in prop::collection::vec(0u16..1201,1..32)) {
        let original=font();let mut input=mappings();let mut distinct=Vec::new();
        for (code,&width) in widths.iter().enumerate() {
            input[code]=binding(Some(1),f64::from(width),Some(vec![65]));
            if width!=600 && !distinct.contains(&width) {distinct.push(width);}
        }
        let budget=DecodeBudget::default();let plan=build(&budget,&original,input).unwrap();
        prop_assert_eq!(plan.glyphs.len(),2+distinct.len());
        for (code,&width) in widths.iter().enumerate() {
            let expected=if width==600 {1}else{2+distinct.iter().position(|w|*w==width).unwrap()};
            prop_assert_eq!(plan.codes[code].glyph,Some(expected as u16));
            let g=&plan.glyphs[expected];prop_assert_eq!(g.original,1);
            prop_assert_eq!(g.pdf_width.unwrap_or(original.glyphs[usize::from(g.original)].outline.advance),f64::from(width));
            prop_assert_eq!(&plan.codes[code].mapping.scalars,&Some(vec![65]));
        }
        prop_assert!(budget.check().is_ok());
    }
}
