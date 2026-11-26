//! 原本罫線の小数座標を、表推定用の整数キーへ丸めて失わない。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;

#[allow(dead_code)]
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 小数座標の線と四辺を持つ正式な一ページPDFを生成する。
fn source(content: &str) -> Vec<u8> {
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 500 500] /Contents 4 0 R /Resources << >> >>"
            .into(),
        page_attribute_pdf::stream(content),
    ])
}

/// 四分の一点の原本矩形は、解析公開値でも四辺すべて同じ小数座標を保持する。
#[test]
fn source_rectangle_keeps_fractional_edges_before_table_inference() {
    let parsed = parse_pdf_bytes(
        "owned-fractional-rules.pdf",
        &source(".5 w 20.25 30.75 40.5 10.5 re S"),
    )
    .unwrap();
    let actual: Vec<_> = parsed
        .graphics
        .iter()
        .map(|g| [g.bounds.x0, g.bounds.y0, g.bounds.x1, g.bounds.y1])
        .collect();
    assert_eq!(
        actual,
        vec![
            [20.25, 30.75, 20.25, 41.25],
            [20.25, 30.75, 60.75, 30.75],
            [20.25, 41.25, 60.75, 41.25],
            [60.75, 30.75, 60.75, 41.25]
        ],
        "the parser must not replace source coordinates with integer grid keys"
    );
}

/// 同じ整数ptへ丸まる二つの平行線を、一本の太い罫線へ誤結合しない。
#[test]
fn close_parallel_rules_remain_distinct() {
    let parsed = parse_pdf_bytes(
        "owned-close-rules.pdf",
        &source(".1 w 20.1 30 m 20.1 60 l S 20.4 30 m 20.4 60 l S"),
    )
    .unwrap();
    assert_eq!(parsed.graphics.len(), 2);
    assert_eq!(parsed.graphics[0].bounds.x0, 20.1);
    assert_eq!(parsed.graphics[1].bounds.x0, 20.4);
}

/// 同一直線の分割描画をまとめても、元の小数端点と線幅を維持する。
#[test]
fn collinear_fragments_keep_source_extents_when_joined() {
    let parsed = parse_pdf_bytes(
        "owned-joined-rules.pdf",
        &source(".5 w 20.25 30.75 m 40.5 30.75 l S 40.5 30.75 m 60.75 30.75 l S"),
    )
    .unwrap();
    assert_eq!(parsed.graphics.len(), 1);
    let g = &parsed.graphics[0];
    assert_eq!(
        [g.bounds.x0, g.bounds.y0, g.bounds.x1, g.bounds.y1],
        [20.25, 30.75, 60.75, 30.75]
    );
    assert_eq!(g.stroke_width, 0.5);
}

/// 同一直線の隣り合う線でも太さが異なれば、原本の幅と区間を潰さない。
#[test]
fn collinear_different_widths_remain_source_fragments() {
    let parsed = parse_pdf_bytes(
        "owned-varying-source-widths.pdf",
        &source(".5 w 20.25 30.75 m 40.5 30.75 l S 1.5 w 40.5 30.75 m 60.75 30.75 l S"),
    )
    .unwrap();
    assert_eq!(parsed.graphics.len(), 2);
    assert_eq!(parsed.graphics[0].stroke_width, 0.5);
    assert_eq!(parsed.graphics[0].bounds.x1, 40.5);
    assert_eq!(parsed.graphics[1].stroke_width, 1.5);
    assert_eq!(parsed.graphics[1].bounds.x0, 40.5);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 二進表現が正確な四分ptの平行移動でも、原本四辺をそのまま公開する。
    #[test]
    fn quarter_point_translation_does_not_quantize_rules(x in 40u16..200,y in 40u16..200,w in 12u16..100,h in 12u16..100) {
        let (x,y,w,h)=(f32::from(x)/4.,f32::from(y)/4.,f32::from(w)/4.,f32::from(h)/4.);
        let parsed=parse_pdf_bytes("owned-translated-fractional-rules.pdf",&source(&format!(".5 w {x} {y} {w} {h} re S"))).unwrap();
        prop_assert_eq!(parsed.graphics.len(),4);
        for g in parsed.graphics {
            prop_assert!([x,x+w].contains(&g.bounds.x0));
            prop_assert!([x,x+w].contains(&g.bounds.x1));
            prop_assert!([y,y+h].contains(&g.bounds.y0));
            prop_assert!([y,y+h].contains(&g.bounds.y1));
        }
    }
}
