//! 部分clipの文字所属を保存し、短いobjectごとに全ページindexを再確保しない。
use super::{budget, owned_text_pdf, with_native_bytes};
use pdfium_render::prelude::*;
use proptest::prelude::*;

/// 最適化と独立に、SDKの厳密なobject所属APIを使う旧経路の除外maskを求める。
pub(crate) fn native_ownership_reference(page: &PdfPage<'_>) -> Vec<bool> {
    let text = page.text().unwrap();
    let mut result = Vec::new();
    reference_objects(
        page.objects(),
        &text,
        None,
        [1., 0., 0., 1., 0., 0.],
        &mut result,
    );
    result
}

/// 親Formのclipと変換を維持し、幾何keyによる所属検索を一切使わない対照を作る。
fn reference_objects<'a>(
    objects: &'a impl PdfPageObjectsCommon<'a>,
    page_text: &PdfPageText<'_>,
    inherited: Option<&crate::form_clip::Clip>,
    matrix: [f32; 6],
    result: &mut Vec<bool>,
) {
    let budget = budget(16 * 1024 * 1024);
    for object in objects.iter() {
        let own = crate::pdfium_clip::object_clip(&budget, &object, inherited, matrix);
        let region = own.as_ref().or(inherited);
        if let Some(form) = object.as_x_object_form_object() {
            if let Some(matrix) = crate::pdfium_clip::form_matrix(&object, matrix) {
                reference_objects(form, page_text, region, matrix, result);
            }
        } else if let (Some(text), Some(clip), Ok(bounds)) =
            (object.as_text_object(), region, object.bounds())
        {
            let rect = bounds.to_rect();
            if clip.contains_rect(
                [
                    rect.left().value,
                    rect.bottom().value,
                    rect.right().value,
                    rect.top().value,
                ],
                matrix,
            ) {
                continue;
            }
            if result.is_empty() {
                result.resize(page_text.len() as usize, false);
            }
            for character in text.chars(page_text).unwrap().iter() {
                let rect = character.loose_bounds().unwrap();
                if clip
                    .transformed_bounds(
                        &budget,
                        [
                            rect.left().value,
                            rect.bottom().value,
                            rect.right().value,
                            rect.top().value,
                        ],
                        [1., 0., 0., 1., 0., 0.],
                    )
                    .is_none()
                {
                    result[character.index()] = true;
                }
            }
        }
    }
}

/// 同じSDK lifecycleの文字indexへ、共有索引と厳密なnative所属の両方を適用する。
fn compare_with_native_ownership(source: &[u8]) {
    with_native_bytes(source, |page| {
        let expected = native_ownership_reference(page);
        let text = page.text().unwrap();
        let actual =
            crate::pdfium_clip::excluded(&budget(16 * 1024 * 1024), page.objects(), &text).unwrap();
        assert_eq!(
            actual, expected,
            "shared indices must retain exact native object ownership"
        );
    });
}

/// 同じ字面でも位置が異なる160個のowned text objectを作る。
fn distinct_partial_objects() -> String {
    (0..160)
        .map(|i| {
            let x = 5 + (i % 20) * 9;
            let y = 10 + (i / 20) * 20;
            format!(
                "q {} {} 1 14 re W n BT /F 7 Tf {x} {y} Td (W) Tj ET Q ",
                x + 3,
                y - 2,
            )
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 反復文字・同一幾何・色違い・回転・異なるclipでも、SDKの元所属と同じmaskを返す。
    #[test]
    fn repeated_and_rotated_objects_match_exact_native_ownership(
        specs in proptest::collection::vec((0_u8..8, 0_u8..8, 0_u8..5, 0_u8..4, any::<bool>()), 1..16)
    ) {
        let mut content=String::new();
        for (x,y,clip,rotation,red) in specs {
            let x=20+u16::from(x)*15;let y=30+u16::from(y)*15;
            let (a,b,c,d)=match rotation{0=>(1,0,0,1),1=>(0,1,-1,0),2=>(-1,0,0,-1),_=>(0,-1,1,0)};
            let (left,width)=match clip{0=>(x as i32-60,120),1=>(x as i32+2,8),2=>(x as i32+10,10),3=>(x as i32-10,2),_=>(x as i32-3,5)};
            content.push_str(&format!("q {left} {} {width} 24 re W n {} 0 0 rg {a} {b} {c} {d} {x} {y} cm BT /F 12 Tf 0 0 Td (XXalpha) Tj ET Q ",y as i32-8,if red{1}else{0}));
        }
        compare_with_native_ownership(&owned_text_pdf(&content));
    }
}

/// 同じ字面・同じ矩形・同じ行列でも、違うclipのobjectを一つにまとめない。
#[test]
fn coincident_objects_with_different_clips_keep_sdk_ownership() {
    compare_with_native_ownership(&owned_text_pdf(
        "q 30 70 8 20 re W n BT /F 12 Tf 20 80 Td (XXalpha) Tj ET Q q 40 70 8 20 re W n BT /F 12 Tf 20 80 Td (XXalpha) Tj ET Q",
    ));
}

/// 共有Formの同じleaf幾何は曖昧として扱い、継承clipと元SDK所属を維持する。
#[test]
fn repeated_form_leaves_preserve_native_ownership() {
    use crate::page_attribute_fixture::{pdf, stream};
    for paint in [
        "q 20 70 35 40 re W n /Fm Do Q",
        "q 20 70 35 40 re W n /Fm Do Q q 1 0 0 1 50 20 cm 20 70 35 40 re W n /Fm Do Q",
    ] {
        let mut form = stream("BT /F 12 Tf 3.992 80 Td (XXalpha) Tj ET");
        form=form.replacen("<<", "<< /Type /XObject /Subtype /Form /BBox [0 0 200 200] /Resources << /Font << /F 6 0 R >> >>",1);
        let source=pdf(&["<< /Type /Catalog /Pages 2 0 R >>".into(),"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Fm 5 0 R >> >> /Contents 4 0 R >>".into(),stream(paint),form,"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into()]);
        compare_with_native_ownership(&source);
    }
}

/// 共有indexなら256KiBで扱えるページを、object数×全文字数の予約で拒否しない。
#[test]
fn distinct_partial_text_objects_use_shared_character_index_budget() {
    let source = owned_text_pdf(&distinct_partial_objects());
    with_native_bytes(&source, |page| {
        let text = page.text().unwrap();
        assert!(text.len() >= 160);
        let expected = crate::pdfium_clip::excluded(&budget(10_000_000), page.objects(), &text)
            .expect("large-budget native ownership reference");
        assert!(!expected.is_empty());
        let actual = crate::pdfium_clip::excluded(&budget(256 * 1024), page.objects(), &text)
            .expect("distinct partial objects must share page character index workspace");
        assert_eq!(actual, expected);
    });
}
