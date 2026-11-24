//! 長さだけでは落ちる部分外辺を、実際の上下横罫線と縦罫線で確認して表の列を保持する。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// ヘッダーの片方だけ外辺を省いた所有表を生成する。本文の語や位置は公開原本から転記しない。
fn source(left: u16, width: u16, body_height: u16, short_left: bool) -> Vec<u8> {
    let (left, width, body_height) = (f64::from(left), f64::from(width), f64::from(body_height));
    let middle = left + width;
    let right = middle + width;
    let bottom = 300.;
    let top = 700.;
    let body = bottom + body_height;
    let gap = (top - body) / 3.;
    let short = if short_left { left } else { right };
    let full = if short_left { right } else { left };
    let (header_left, header_right) = if short_left { (left, middle) } else { (middle, right) };
    let mut content = format!("1 w {left} {bottom} m {right} {bottom} l S {left} {body} m {right} {body} l S {left} {top} m {right} {top} l S {middle} {bottom} m {middle} {top} l S {full} {bottom} m {full} {top} l S {short} {bottom} m {short} {body} l S {short} {} m {short} {} l S ",body+gap,body+gap*2.);
    for i in 1..3 {
        let y = body + gap * f64::from(i);
        content.push_str(&format!("{header_left} {y} m {header_right} {y} l S "));
    }
    for (x,y,label) in [(left+10.,400.,"BodyL"),(middle+10.,400.,"BodyR"),(full.min(middle)+10.,(top+body)/2.,"LongHead")] {
        content.push_str(&format!("BT /F1 8 Tf {x} {y} Td ({label}) Tj ET "));
    }
    for (i,label) in ["HeadA","HeadB","HeadC"].into_iter().enumerate() {
        let y = body + gap * (i as f64 + 0.5);
        content.push_str(&format!("BT /F1 8 Tf {} {y} Td ({label}) Tj ET ",header_left+10.));
    }
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream",content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i,object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n",i+1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets { bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()); }
    bytes.extend_from_slice(format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    bytes
}

/// 公開byte変換の実際のWord表から行・列順の本文を読み、通常段落での単なる語の存在と区別する。
fn rows(left:u16,width:u16,body_height:u16,short_left:bool) -> Vec<Vec<String>> {
    let bytes=convert_minimum_pdf_bytes_to_docx_bytes("owned-partial-outer-edge.pdf",&source(left,width,body_height,short_left)).unwrap();
    let mut zip=zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml=String::new();zip.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    let mut reader=Reader::from_str(&xml);let mut rows=Vec::new();let mut row=Vec::new();let mut cell=String::new();let mut in_text=false;let mut in_cell=false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(t) if t.name().as_ref()==b"w:tr"=>row.clear(),
            Event::Start(t) if t.name().as_ref()==b"w:tc"=>{cell.clear();in_cell=true;},
            Event::Start(t) if t.name().as_ref()==b"w:t"=>in_text=true,
            Event::End(t) if t.name().as_ref()==b"w:t"=>in_text=false,
            Event::Text(t) if in_text && in_cell=>cell.push_str(&t.decode().unwrap()),
            Event::End(t) if t.name().as_ref()==b"w:tc"=>{row.push(cell.clone());in_cell=false;},
            Event::End(t) if t.name().as_ref()==b"w:tr"=>rows.push(row.clone()),
            Event::Eof=>return rows,
            _=>{}
        }
    }
}

/// 左右いずれの短い外辺でも、部分ヘッダーの三行と本文の列所属を保つ。
#[test]
fn partial_outer_side_keeps_the_complete_editable_table() {
    for short_left in [false,true] {
        let mut expected=vec![vec!["LongHead","HeadC"],vec!["","HeadB"],vec!["","HeadA"],vec!["BodyL","BodyR"]];
        if short_left { for row in expected.iter_mut().take(3) {row.swap(0,1);} }
        assert_eq!(rows(72,100,300,short_left),expected,"partial outer edge must not discard source table columns");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 表の水平移動・列幅・外辺の長さ・左右反転を変えても、独立した原本の行列対応を変えない。
    #[test]
    fn partial_outer_side_ownership_survives_translation_and_width(left in 20u16..150,width in 60u16..150,body_height in 260u16..317,short_left in any::<bool>()) {
        let mut expected=vec![vec!["LongHead","HeadC"],vec!["","HeadB"],vec!["","HeadA"],vec!["BodyL","BodyR"]];
        if short_left { for row in expected.iter_mut().take(3) {row.swap(0,1);} }
        prop_assert_eq!(rows(left,width,body_height,short_left),expected);
    }
}
