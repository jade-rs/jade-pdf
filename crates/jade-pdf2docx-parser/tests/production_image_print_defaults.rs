//! 原本画像のsampleを、画像へ作用しない通常の印刷設定によって白へ置換しない。
#[allow(dead_code)]
#[path="support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{parse_pdf_bytes,QualityIssue};

/// 画像の非stroke alphaや色変換を追加しない、通常の印刷状態。
fn resources()-> &'static str{
    "<< /XObject << /Im 4 0 R >> /ExtGState << /S << /Type /ExtGState /BG2 /Default /UCR2 /Default /OP false /op false /OPM 1 /SA false /SM .02 >> >> >>"
}

/// DeviceRGB画像は通常の印刷設定でも、固有の六つのsource sampleを保持する。
#[test]
fn normal_print_defaults_preserve_color_image_samples(){
    let input=fixture::image_with_resources("/ColorSpace /DeviceRGB /Filter /FlateDecode",&fixture::compressed(&[12,34,56,78,90,123]),8,b"/S gs /Im Do",resources(),false,&[]);
    let parsed=parse_pdf_bytes("owned-normal-print-rgb.pdf",&input).unwrap();
    assert!(!parsed.quality.issues.iter().any(|q|matches!(q,QualityIssue::ImageReplaced{..})),"{:?}",parsed.quality);
    assert_eq!(parsed.image_elements.len(),1);
    assert_eq!(fixture::scanline(&parsed.image_elements[0].png_bytes),[0,12,34,56,78,90,123]);
}

/// stencilの二つのsource bitは、通常の印刷設定でも指定した塗り色と透明部を保持する。
#[test]
fn normal_print_defaults_preserve_stencil_color_and_alpha(){
    let input=fixture::stencil_with_resources(&fixture::compressed(&[0x40]),b".25 .5 .75 rg /S gs /Im Do",resources(),true,&[]);
    let parsed=parse_pdf_bytes("owned-normal-print-stencil.pdf",&input).unwrap();
    assert!(!parsed.quality.issues.iter().any(|q|matches!(q,QualityIssue::ImageReplaced{..})),"{:?}",parsed.quality);
    assert_eq!(parsed.image_elements.len(),1);
    assert_eq!(fixture::scanline(&parsed.image_elements[0].png_bytes),[0,64,128,191,255,64,128,191,0]);
}
