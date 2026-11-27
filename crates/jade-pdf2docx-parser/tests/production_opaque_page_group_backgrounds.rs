//! 原本の不透明なRGB背景を、確認済みのページ透明グループと未知の合成から区別する。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
use proptest::prelude::*;
#[allow(dead_code)]
#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// 正式なxrefを持つ一ページPDFで、ページGroupと合成状態を独立に指定する。
fn document(group: &str, resources: &str, paint: &str, extra: &[&str]) -> ParsedDocument {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 200 300] >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R {group} /Resources << /Font << /F 5 0 R >> {resources} >> /Contents 4 0 R >>"
        ),
        fixture::stream(&format!(
            "{paint} .553 .702 .886 rg 10 100 180 30 re f* 0 g q 10 100 180 30 re W* n BT /F 12 Tf 20 110 Td (Header) Tj ET Q"
        )),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    objects.extend(extra.iter().map(|s| (*s).to_owned()));
    parse_pdf_bytes("owned-opaque-page-group.pdf", &fixture::pdf(&objects)).unwrap()
}

/// 合成状態が不透明な場合、既定の非分離・非knockout RGBページGroupでも原本色を残す。
#[test]
fn default_rgb_page_group_preserves_verified_opaque_fill() {
    for group in [
        "/Group << /S /Transparency /CS /DeviceRGB >>",
        "/Group << /Type /Group /S /Transparency /CS /DeviceRGB /I false /K false >>",
        "/Gr#6Fup << /S /Transp#61rency /CS /DeviceR#47B >>",
    ] {
        let parsed = document(group, "", "", &[]);
        let background = parsed.text_elements[0]
            .source_background
            .expect("opaque RGB page group does not remove the original background");
        assert_eq!(background.rgb, [141, 179, 226]);
        assert_eq!(background.bounds.x0, 10.);
        assert_eq!(background.bounds.x1, 190.);
    }
}

/// Group辞書とその属性の間接参照でも、同じ所属ページの不透明なRGBだけを確認する。
#[test]
fn indirect_group_and_attributes_keep_the_same_source_fill() {
    let parsed = document(
        "/Group 6 0 R",
        "/ExtGState << /GS0 << /Type /ExtGState /ca 1 /CA 1 /BM /Normal /SMask /None >> >>",
        "/GS0 gs",
        &[
            "<< /Type /Group /S /Transparency /CS 7 0 R /I 8 0 R /K 8 0 R >>",
            "/DeviceRGB",
            "false",
        ],
    );
    assert_eq!(
        parsed.text_elements[0].source_background.unwrap().rgb,
        [141, 179, 226]
    );
}

/// 色空間・knockout・未解決Groupや曖昧な属性は、RGBの不透明な証明へ昇格させない。
#[test]
fn unsupported_or_ambiguous_groups_do_not_invent_source_backgrounds() {
    for group in [
        "/Group << /S /Transparency /CS /DeviceRGB /K true >>",
        "/Group << /S /Transparency /CS /DeviceRGB /I true >>",
        "/Group << /S /Transparency /CS /DeviceCMYK >>",
        "/Group << /S /Transparency /CS /DeviceGray >>",
        "/Group << /S /Transparency >>",
        "/Group << /S /Unknown /CS /DeviceRGB >>",
        "/Group << /Type /Unknown /S /Transparency /CS /DeviceRGB >>",
        "/Group << /S /Transparency /CS /DeviceRGB /K 0 >>",
        "/Group << /S /Transparency /CS /DeviceRGB /K false /K true >>",
        "/Group << /S /Transparency /CS /DeviceRGB /K false /#4B false >>",
        "/Group << /S /Transparency /CS /DeviceRGB /I false /#49 false >>",
        "/Group << /S /Transparency /CS /DeviceRGB /Unknown true >>",
        "/Group << /S /Transparency /CS /DeviceRGB >> /Gr#6Fup << /S /Transparency /CS /DeviceRGB >>",
        "/Group 99 0 R",
        "/Group null",
        "/Group []",
    ] {
        let parsed = document(group, "", "", &[]);
        assert_eq!(parsed.text_elements[0].text, "Header");
        assert_eq!(parsed.text_elements[0].source_background, None, "{group}");
    }
}

/// RGBページGroupの確認は、既存の半透明・別blend・soft mask・既定色置換の拒否を回避しない。
#[test]
fn group_does_not_override_existing_paint_and_color_guards() {
    for resources in [
        "/ExtGState << /GS << /ca .5 >> >>",
        "/ExtGState << /GS << /CA .5 >> >>",
        "/ExtGState << /GS << /BM /Multiply >> >>",
        "/ExtGState << /GS << /SMask << /S /Alpha >> >> >>",
        "/ExtGState << /GS << /ca 1 >> >> /ColorSpace << /DefaultRGB /DeviceRGB >>",
    ] {
        let parsed = document(
            "/Group << /S /Transparency /CS /DeviceRGB >>",
            resources,
            "/GS gs",
            &[],
        );
        assert_eq!(parsed.text_elements[0].source_background, None);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 不透明な既定Groupの有無と属性順は、原本矩形の色・範囲を変更しない。
    #[test]
    fn verified_group_keeps_the_no_group_control(order in 0_u8..3) {
        let group = match order {
            0 => "/Group << /S /Transparency /CS /DeviceRGB >>",
            1 => "/Group << /K false /CS /DeviceRGB /I false /S /Transparency >>",
            _ => "/Group << /Type /Group /I false /S /Transparency /K false /CS /DeviceRGB >>",
        };
        let parsed=document(group,"","",&[]);
        let control=document("","","",&[]);
        prop_assert!(control.text_elements[0].source_background.is_some());
        prop_assert_eq!(parsed.text_elements[0].source_background,control.text_elements[0].source_background);
    }
}
