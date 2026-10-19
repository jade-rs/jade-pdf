//! 透明度の欠落を不透明画像の成功へ変えない回帰。
use super::*;
use fax::Color;
use flate2::{Compression, write::ZlibEncoder};
use proptest::prelude::*;

/// 対応する8 bit Gray maskの必須属性。
const MASK: &str =
    "/Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /FlateDecode";
/// maskを参照するRGB画像の属性。
const IMAGE: &[u8] =
    b"<< /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode /SMask 5 0 R >>";

/// 画素列をFlateで圧縮する。
fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// 辞書と本文を正確なLengthでmask objectへ格納する。
fn mask_pdf(attributes: &str, data: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "%PDF-1.7\n5 0 obj\n<< {attributes} /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes
}

/// 指定maskを使って2画素のRGB画像を復号する。
fn image(budget: &DecodeBudget, attributes: &str, alpha: &[u8]) -> Option<Vec<u8>> {
    let pdf = mask_pdf(attributes, alpha);
    cache_mask_streams(budget, &pdf);
    decode_image_stream_as_png(
        budget,
        &pdf,
        IMAGE,
        &compressed(&[10, 20, 30, 40, 50, 60]),
        2,
        1,
    )
}

/// 公開parserと同じ構造検査済みLength境界をunit fixtureへ設定する。
fn cache_mask_streams(budget: &DecodeBudget, pdf: &[u8]) {
    let limits = ParseLimits::default();
    let streams = crate::structure::validate_structure(
        std::path::Path::new("mask.pdf"),
        pdf,
        9,
        &limits,
        budget,
    )
    .unwrap();
    budget.cache_streams(pdf, &streams);
}

/// 壊れたmask・不足する画素を不透明画像へフォールバックしない。
#[test]
fn failed_soft_mask_rejects_base_image() {
    for bytes in [b"broken".to_vec(), compressed(&[77])] {
        let budget = DecodeBudget::new(&ParseLimits::default());
        assert!(image(&budget, MASK, &bytes).is_none());
        assert!(budget.check().is_ok());
    }
}

/// alphaのSub差分を復元して透明度を保持する。
#[test]
fn predicted_soft_mask_preserves_alpha() {
    let budget = DecodeBudget::new(&ParseLimits::default());
    let png = image(
        &budget,
        &format!("{MASK} /DecodeParms << /Predictor 11 /Columns 2 >>"),
        &compressed(&[1, 77, 101]),
    )
    .unwrap();
    assert_eq!(
        decode_filter0_png_to_rgba(&budget, &png).unwrap().rgba,
        [10, 20, 30, 77, 40, 50, 60, 178]
    );
}

/// 未対応のbit深度・Decode・Matteと不正寸法は透明度を無視しない。
#[test]
fn unsupported_soft_mask_attributes_reject_base_image() {
    for attributes in [
        MASK.replace("/BitsPerComponent 8", "/BitsPerComponent 1"),
        MASK.replace("/BitsPerComponent 8", ""),
        MASK.replace("/Width 2", "/Width 0"),
        MASK.replace("/Width 2", "/Width 2.5"),
        format!("{MASK} /Decode [0 .5]"),
        format!("{MASK} /Decode [0 1 0]"),
        format!("{MASK} /Matte [1 1]"),
        format!("{MASK} /DecodeParms << /Predictor 12 >>"),
        format!("{MASK} /ImageMask true"),
    ] {
        let budget = DecodeBudget::new(&ParseLimits::default());
        assert!(
            image(&budget, &attributes, &compressed(&[0, 77])).is_none(),
            "{attributes}"
        );
        assert!(budget.check().is_ok());
    }
}

/// 間接数値と明示的な正符号、間接Decodeを解決する。
#[test]
fn indirect_soft_mask_attributes_preserve_alpha() {
    let attributes = "/Width 6 0 R /Height 7 0 R /BitsPerComponent 8 0 R /ColorSpace /DeviceGray /Filter /FlateDecode /Decode 9 0 R";
    let mut pdf = mask_pdf(attributes, &compressed(&[0, 77]));
    pdf.extend_from_slice(
        b"6 0 obj\n+2\nendobj\n7 0 obj\n+1\nendobj\n8 0 obj\n+8\nendobj\n9 0 obj\n[1 0]\nendobj\n",
    );
    let budget = DecodeBudget::new(&ParseLimits::default());
    let png = decode_image_stream_as_png(
        &budget,
        &pdf,
        IMAGE,
        &compressed(&[10, 20, 30, 40, 50, 60]),
        2,
        1,
    )
    .unwrap();
    assert_eq!(
        decode_filter0_png_to_rgba(&budget, &png).unwrap().rgba,
        [10, 20, 30, 255, 40, 50, 60, 178]
    );
}

/// 存在するSMaskの非参照値・欠落objectを未指定扱いしない。
#[test]
fn invalid_soft_mask_reference_rejects_base_image() {
    for value in ["99 0 R", "false", "/None", "<< >>"] {
        let budget = DecodeBudget::new(&ParseLimits::default());
        let dict = format!(
            "<< /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode /SMask {value} >>"
        );
        assert!(
            decode_image_stream_as_png(&budget, b"", dict.as_bytes(), &compressed(&[10; 6]), 2, 1)
                .is_none(),
            "{value}"
        );
    }
}

/// nullと未指定はRGBのまま、指定された有効maskはRGBAを返す。
#[test]
fn optional_soft_mask_keeps_opaque_and_alpha_pixels() {
    for suffix in ["", "/SMask null", "/Note << /SMask 99 0 R >>"] {
        let budget = DecodeBudget::new(&ParseLimits::default());
        let dict = format!(
            "<< /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode {suffix} >>"
        );
        let png =
            decode_image_stream_as_png(&budget, b"", dict.as_bytes(), &compressed(&[10; 6]), 2, 1)
                .unwrap();
        assert_eq!(png[25], 2);
    }
    let budget = DecodeBudget::new(&ParseLimits::default());
    let png = image(&budget, MASK, &compressed(&[0, 77])).unwrap();
    assert_eq!(
        decode_filter0_png_to_rgba(&budget, &png).unwrap().rgba,
        [10, 20, 30, 0, 40, 50, 60, 77]
    );
}

/// CCITTにも同じ外部maskを適用し、失敗したmaskは画像を拒否する。
#[test]
fn ccitt_soft_mask_preserves_alpha_and_rejects_failure() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    encoder
        .encode_line([Color::Black; 2].into_iter(), 2)
        .unwrap();
    let mut writer = encoder.finish().unwrap();
    writer.pad();
    let data = writer.finish();
    let dictionary =
        b"<< /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns 2 >> /SMask 5 0 R >>";
    let budget = DecodeBudget::new(&ParseLimits::default());
    let png = decode_image_stream_as_png(
        &budget,
        &mask_pdf(MASK, &compressed(&[0, 77])),
        dictionary,
        &data,
        2,
        1,
    )
    .unwrap();
    assert_eq!(
        decode_filter0_png_to_rgba(&budget, &png).unwrap().rgba,
        [0, 0, 0, 0, 0, 0, 0, 77]
    );
    assert!(
        decode_image_stream_as_png(&budget, &mask_pdf(MASK, b"broken"), dictionary, &data, 2, 1)
            .is_none()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// Decodeの既定/明示identity/反転は全8 bit alphaに一致する。
    #[test]
    fn soft_mask_decode_matches_alpha_model(alpha in any::<[u8;2]>(), inverted in any::<bool>()) {
        let budget = DecodeBudget::new(&ParseLimits::default());
        let attributes = format!("{MASK} /Decode {}", if inverted {"[1 0]"} else {"[0 1]"});
        let png = image(&budget, &attributes, &compressed(&alpha)).unwrap();
        let pixels = decode_filter0_png_to_rgba(&budget, &png).unwrap().rgba;
        let expected = alpha.map(|value| if inverted {255-value} else {value});
        prop_assert_eq!([pixels[3],pixels[7]], expected);
    }
}
