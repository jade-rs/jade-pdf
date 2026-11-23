//! 同じUnicode文字列でも異なる元glyph列を、実PDFから失わずに出力へ渡す。
use crate::type1_embedding_tests::{charstring, encrypt};
use proptest::prelude::*;

/// glyphごとの位置を保存しても、previewを一文字ずつ重複除去して原文を壊さない。
#[test]
fn positioned_type1_preview_preserves_repeated_letters_and_logical_gaps() {
    for (content,expected) in [
        ("BT /F1 20 Tf 20 80 Td (BAAB) Tj ET","ffffff"),
        ("BT /F1 20 Tf 20 80 Td [(A) -400 (A)] TJ ET","f f"),
        ("BT /F1 20 Tf 20 80 Td (B) Tj 0 -40 Td (AA) Tj ET","ff"),
    ] {
        let parsed=crate::parse_pdf_bytes("owned-positioned-preview.pdf",&pdf_with_content(content.as_bytes(),None)).unwrap();
        assert_eq!(parsed.text_preview,expected,"{content}");
    }
}

proptest! {
    /// Tc・Tzと負のTJが変わっても、元glyphの幅と各原点をUnicode字数から独立に保つ。
    #[test]
    fn original_glyph_origins_survive_spacing_and_horizontal_scale(
        codes in prop::collection::vec(any::<bool>(), 2..12),
        tc in -2i32..3, scale in 40i32..181, tj in 0i32..71,
    ) {
        let operand = codes.iter().map(|b| if *b { "(B)" } else { "(A)" }).collect::<Vec<_>>().join(&format!(" {tj} "));
        let content = format!("BT /F1 20 Tf {tc} Tc {scale} Tz 20 80 Td [{operand}] TJ ET");
        let parsed = crate::parse_pdf_bytes("owned-glyph-positions.pdf", &pdf_with_content(content.as_bytes(), None)).unwrap();
        prop_assert_eq!(parsed.text_elements.len(), codes.len());
        let factor = scale as f32 / 100.;
        let mut x = 20.;
        for (e, ligature) in parsed.text_elements.iter().zip(codes) {
            let width = if ligature { 10. } else { 6. };
            prop_assert_eq!(e.text.as_str(), if ligature { "ff" } else { "f" });
            prop_assert!((e.bounds.x0 - x).abs() < 0.002, "{:?}, expected x={}", e, x);
            prop_assert!((e.bounds.x1 - e.bounds.x0 - width * factor).abs() < 0.002);
            x += (width + tc as f32 - tj as f32 * 0.02) * factor;
        }
    }
}

/// 既知の小整数を、テスト側の独立Type1 encoderで書く。
fn number(out: &mut Vec<u8>, value: i32) {
    match value {
        -107..=107 => out.push((value + 139) as u8),
        108..=1131 => {
            let v = value - 108;
            out.extend_from_slice(&[(247 + v / 256) as u8, (v % 256) as u8]);
        }
        -1131..=-108 => {
            let v = -value - 108;
            out.extend_from_slice(&[(251 + v / 256) as u8, (v % 256) as u8]);
        }
        _ => panic!("owned range"),
    }
}

/// 元advanceと同じ幅の所有三角形を、一つのType1 glyphにする。
fn triangle(width: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    number(&mut bytes, 0);
    number(&mut bytes, width);
    bytes.push(13);
    number(&mut bytes, 0);
    number(&mut bytes, 0);
    bytes.push(21);
    for [x, y] in [[width / 2, 700], [width - width / 2, -700], [-width, 0]] {
        number(&mut bytes, x);
        number(&mut bytes, y);
        bytes.push(5);
    }
    bytes.extend_from_slice(&[9, 14]);
    charstring(&bytes)
}

/// f幅300とff幅500を持ち、埋め込み許可も明示する所有fontを作る。
pub(crate) fn font_program() -> (Vec<u8>, usize, usize, usize) {
    let clear=b"%!PS-AdobeFont-1.0: JadeOwnedClusters 1.0\n\
        % Copyright Jade owned; these test outlines may be embedded and modified.\n\
        11 dict begin /FontType 1 def /FontName /JadeOwnedClusters def\n\
        /FontMatrix [0.001 0 0 0.001 0 0] readonly def /FontBBox [0 0 500 700] readonly def\n\
        /FontInfo 4 dict dup begin /FSType 0 def /FullName (JadeOwnedClusters) def\n\
        /FamilyName (JadeOwnedClusters) def /Notice (Copyright Jade owned; owned test font.) def end readonly def\n\
        /Encoding 256 array 0 1 255 {1 index exch /.notdef put} for dup 65 /f put dup 66 /ff put readonly def\n\
        currentdict end currentfile eexec\n";
    let mut private = b"abcddup /Private 8 dict dup begin\n\
        /RD {string currentfile exch readstring pop} executeonly def\n\
        /ND {noaccess def} executeonly def /NP {noaccess put} executeonly def\n\
        /lenIV 4 def /Subrs 0 array def end readonly put\n\
        dup /CharStrings 3 dict dup begin\n"
        .to_vec();
    for (name, program) in [
        (".notdef", charstring(&[139, 248, 236, 13, 14])),
        ("f", triangle(300)),
        ("ff", triangle(500)),
    ] {
        private.extend_from_slice(format!("/{name} {} RD ", program.len()).as_bytes());
        private.extend_from_slice(&program);
        private.extend_from_slice(b" ND\n");
    }
    private.extend_from_slice(
        b"end readonly put dup /FontName get exch definefont pop mark currentfile closefile\n",
    );
    let cipher = encrypt(&private, 55665);
    let tail = format!("\n{}\ncleartomark\n", "0".repeat(512));
    let mut source = clear.to_vec();
    source.extend_from_slice(&cipher);
    source.extend_from_slice(tail.as_bytes());
    (source, clear.len(), cipher.len(), tail.len())
}

/// Lengthを持つ所有streamを作り、バイナリ内の改行を境界に使わない。
fn stream(bytes: &[u8], attributes: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {} {attributes} >>\nstream\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\nendstream");
    out
}

/// 上の行はcode B一個、下の行はcode A二個で、両方の原文Unicodeをffとする。
fn pdf() -> Vec<u8> {
    pdf_with_content(b"BT /F1 20 Tf 20 80 Td (B) Tj 0 -40 Td (AA) Tj ET", None)
}

/// 同じ原本programを使い、描画命令と別resourceの幅だけを変更する所有PDF。
pub(crate) fn pdf_with_content(content: &[u8], second_width: Option<u16>) -> Vec<u8> {
    encode_objects(objects(content, second_width))
}

/// TJによる論理空白を、元fontのcode32へ割り当てず独立した位置として保つ。
#[test]
fn positioned_type1_keeps_tj_space_geometry() {
    let input = pdf_with_content(b"BT /F1 20 Tf 20 80 Td [(A) -400 (A)] TJ ET", None);
    let parsed = crate::parse_pdf_bytes("owned-positioned-gap.pdf", &input).unwrap();
    let text = &parsed.text_elements;
    assert!(text.iter().all(|e| e.baseline_y == Some(80.)));
    assert_eq!(
        text.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
        ["f", " ", "f"]
    );
    assert_eq!(text[1].font_name, None);
    assert!((text[1].bounds.x0 - 26.).abs() < 0.001);
    assert!((text[1].bounds.x1 - 34.).abs() < 0.001);
    assert!((text[2].bounds.x0 - 34.).abs() < 0.001);
    assert_eq!(text[0].font_name, text[2].font_name);
}

/// 合字faceから通常faceへの切り替えでも、TJの単語境界を保持する。
#[test]
fn positioned_type1_keeps_space_across_face_changes() {
    let input = pdf_with_content(b"BT /F1 20 Tf 20 80 Td [(B) -400 (A)] TJ ET", None);
    let parsed = crate::parse_pdf_bytes("owned-positioned-face-gap.pdf", &input).unwrap();
    let text = &parsed.text_elements;
    assert_eq!(
        text.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
        ["ff", " ", "f"]
    );
    assert!((text[1].bounds.x0 - 30.).abs() < 0.001);
    assert!((text[1].bounds.x1 - 38.).abs() < 0.001);
    assert_ne!(text[0].font_name, text[2].font_name);
}

/// 小さな字間調整を論理空白へ変えず、同じface内の元glyph位置を残す。
#[test]
fn positioned_type1_preserves_glyph_origins_inside_one_face() {
    let input = pdf_with_content(b"BT /F1 20 Tf 20 80 Td [(AA) -50 (AA)] TJ ET", None);
    let parsed = crate::parse_pdf_bytes("owned-positioned-kerning.pdf", &input).unwrap();
    assert_eq!(parsed.text_elements.len(), 4);
    for (element, x) in parsed.text_elements.iter().zip([20., 26., 33., 39.]) {
        assert_eq!(element.text, "f");
        assert!((element.bounds.x0 - x).abs() < 0.001, "{element:?}");
        assert!((element.bounds.x1 - element.bounds.x0 - 6.).abs() < 0.001);
    }
}

/// stream長と原本fontを保った、拡張可能な所有object列。
fn objects(content: &[u8], second_width: Option<u16>) -> Vec<Vec<u8>> {
    let (font, first, second, third) = font_program();
    let cmap=b"begincmap\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<41> <0066>\n<42> <00660066>\nendbfchar\nendcmap";
    let resources = if second_width.is_some() {
        "/F1 5 0 R /F2 9 0 R"
    } else {
        "/F1 5 0 R"
    };
    let mut objects=[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 120] /Resources << /Font << {resources} >> >> /Contents 4 0 R >>").into_bytes(),
        stream(content,""),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /JadeOwnedClusters /FirstChar 65 /LastChar 66 /Widths [300 500] /FontDescriptor 6 0 R /ToUnicode 8 0 R >>".to_vec(),
        b"<< /Type /FontDescriptor /FontName /JadeOwnedClusters /Flags 32 /FontBBox [0 0 500 700] /Ascent 700 /Descent 0 /CapHeight 700 /StemV 80 /FontFile 7 0 R >>".to_vec(),
        stream(&font,&format!("/Length1 {first} /Length2 {second} /Length3 {third}")),stream(cmap,"")].to_vec();
    if let Some(width) = second_width {
        objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /JadeOwnedClusters /FirstChar 65 /LastChar 66 /Widths [{width} 500] /FontDescriptor 6 0 R /ToUnicode 8 0 R >>").into_bytes());
    }
    objects
}

/// 任意の所有object列へ正しいxrefとSizeを付ける。
fn encode_objects(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let count = objects.len() + 1;
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        bytes.extend_from_slice(obj);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size {count} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// 同じresource名を持つFormの局所Tfと、親から選択済みのfontを区別する。
#[test]
fn form_local_and_inherited_font_routes_preserve_resource_scope() {
    let expected = crate::parse_pdf_bytes(
        "owned-page-ligature.pdf",
        &pdf_with_content(b"BT /F1 20 Tf 20 80 Td (B) Tj ET", None),
    )
    .unwrap()
    .text_elements[0]
        .font_name
        .clone();
    for (form, inherit, width) in [
        (b"BT 20 40 Td (B) Tj ET".as_slice(), true, 10.),
        (b"BT /F1 20 Tf 20 40 Td (AA) Tj ET".as_slice(), false, 18.),
    ] {
        let mut objects = objects(b"BT /F1 20 Tf 20 80 Td (AA) Tj ET /X Do", Some(450));
        objects[2]=b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 120] /Resources << /Font << /F1 5 0 R >> /XObject << /X 10 0 R >> >> /Contents 4 0 R >>".to_vec();
        objects.push(stream(form,"/Type /XObject /Subtype /Form /FormType 1 /BBox [0 0 200 120] /Resources << /Font << /F1 9 0 R >> >>"));
        let parsed =
            crate::parse_pdf_bytes("owned-scoped-form.pdf", &encode_objects(objects)).unwrap();
        assert_eq!(parsed.text_elements.len(), if inherit { 3 } else { 4 });
        let form = &parsed.text_elements[2..];
        assert!((form.last().unwrap().bounds.x1 - form[0].bounds.x0 - width).abs() < 0.001);
        for e in form {
            if inherit {
                assert_eq!(e.font_name, expected);
            } else {
                assert_ne!(e.font_name, parsed.text_elements[0].font_name);
                assert!((e.bounds.x1 - e.bounds.x0 - 9.).abs() < 0.001);
            }
            assert!(
                parsed
                    .embedded_fonts
                    .iter()
                    .any(|f| Some(&f.font_name) == e.font_name.as_ref())
            );
        }
    }
}

/// 同じ辞書byteでも、新文書の権利とprogramへ切り替え、以前の生成fontを再使用しない。
#[test]
fn document_revalidation_drops_previous_font_outputs_and_aliases() {
    let first = pdf_with_content(b"BT /F1 20 Tf 20 80 Td (AA) Tj ET", None);
    let mut second = first.clone();
    let offset = second
        .windows(b"/FSType 0".len())
        .position(|s| s == b"/FSType 0")
        .unwrap()
        + b"/FSType ".len();
    second[offset] = b'2';
    let limits = crate::ParseLimits::default();
    let budget = crate::decode::DecodeBudget::default();
    let parsed = crate::parser::parse_pdf_bytes_pure_with_budget(
        "owned-first.pdf".into(),
        &first,
        &limits,
        &budget,
    )
    .unwrap();
    assert!(!parsed.embedded_fonts.is_empty());
    let denied = crate::parser::parse_pdf_bytes_pure_with_budget(
        "owned-denied.pdf".into(),
        &second,
        &limits,
        &budget,
    )
    .unwrap();
    assert!(denied.embedded_fonts.is_empty());
    assert_eq!(
        denied.text_elements[0].font_name.as_deref(),
        Some("JadeOwnedClusters")
    );
}

/// 未使用のresourceや合字faceを、出力fontの件数上限へ数えて有効な文書を拒否しない。
#[test]
fn unused_resource_fonts_do_not_exhaust_the_published_font_limit() {
    let mut objects = objects(b"BT /F0 20 Tf 20 80 Td (AA) Tj ET", None);
    let mut resources = String::from("/F0 5 0 R");
    for index in 1..17 {
        let number = objects.len() + 1;
        resources.push_str(&format!(" /F{index} {number} 0 R"));
        objects.push(format!("<< /Type /Font /Subtype /Type1 /BaseFont /JadeOwnedClusters /FirstChar 65 /LastChar 66 /Widths [{} 500] /FontDescriptor 6 0 R /ToUnicode 8 0 R >>",300+index).into_bytes());
    }
    objects[2]=format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 120] /Resources << /Font << {resources} >> >> /Contents 4 0 R >>").into_bytes();
    let parsed =
        crate::parse_pdf_bytes("owned-unused-resource-fonts.pdf", &encode_objects(objects))
            .unwrap();
    assert_eq!(parsed.text_elements.len(), 2);
    assert!(parsed.text_elements.iter().all(|e| e.text == "f"));
    assert_eq!(parsed.embedded_fonts.len(), 1);
    assert!(
        parsed
            .text_elements
            .iter()
            .all(|e| e.font_name.as_deref() == Some(parsed.embedded_fonts[0].font_name.as_str()))
    );
}

/// 一つのTj内の合字・通常二字・合字を、escapeやhexでも元の位置へ振り分ける。
#[test]
fn mixed_source_clusters_in_one_operand_keep_distinct_routes_and_positions() {
    for operand in ["(BAAB)", "<42 4141 42>", "(\\102\\101A\\102)"] {
        let content = format!("BT /F1 20 Tf 20 80 Td {operand} Tj ET");
        let parsed = crate::parse_pdf_bytes(
            "owned-mixed-clusters.pdf",
            &pdf_with_content(content.as_bytes(), None),
        )
        .unwrap();
        assert_eq!(parsed.text_elements.len(), 4, "{operand}");
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .map(|e| e.text.as_str())
                .collect::<Vec<_>>(),
            ["ff", "f", "f", "ff"]
        );
        assert_eq!(
            parsed.text_elements[0].font_name,
            parsed.text_elements[3].font_name
        );
        assert_ne!(
            parsed.text_elements[0].font_name,
            parsed.text_elements[1].font_name
        );
        for (e, (x, width)) in
            parsed
                .text_elements
                .iter()
                .zip([(20., 10.), (30., 6.), (36., 6.), (42., 10.)])
        {
            assert!(
                (e.bounds.x0 - x).abs() < 0.001
                    && (e.bounds.x1 - e.bounds.x0 - width).abs() < 0.001,
                "{e:?}"
            );
            assert!(
                parsed
                    .embedded_fonts
                    .iter()
                    .any(|f| Some(&f.font_name) == e.font_name.as_ref())
            );
        }
    }
}

/// 実SDK照合後も、元ff一glyphと二glyphの選択・幅を消さない。
#[test]
#[cfg(not(target_arch = "wasm32"))]
#[ignore = "explicit PDFIUM_LIB_PATH required for real native routing qualification"]
fn native_mixed_clusters_keep_embedded_routes_after_sdk_reconciliation() {
    let bytes = pdf_with_content(b"BT /F1 20 Tf 20 80 Td (BAAB) Tj ET", None);
    let limits = crate::ParseLimits::default();
    let budget = crate::decode::DecodeBudget::default();
    let parsed = crate::pdfium_backend::parse_pdf(
        std::path::Path::new("owned-native-clusters.pdf"),
        &bytes,
        &limits,
        &budget,
    )
    .unwrap();
    assert_eq!(parsed.text_elements.len(), 4);
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>(),
        ["ff", "f", "f", "ff"]
    );
    assert_ne!(
        parsed.text_elements[0].font_name,
        parsed.text_elements[1].font_name
    );
    for (e, width) in parsed.text_elements.iter().zip([10., 6., 6., 10.]) {
        assert!((e.bounds.x1 - e.bounds.x0 - width).abs() < 0.001, "{e:?}");
        assert!(
            parsed
                .embedded_fonts
                .iter()
                .any(|f| Some(&f.font_name) == e.font_name.as_ref())
        );
    }
}

/// TJの移動とTcはUnicode文字数でなく元glyph数へ適用し、分割後も次の原点を保つ。
#[test]
fn source_cluster_routing_preserves_tj_and_original_glyph_character_spacing() {
    for (content, positions, widths) in [
        (
            "BT /F1 20 Tf 20 80 Td [(B) -100 (AA) 50 (B)] TJ ET",
            [20., 32., 38., 43.],
            [10., 6., 6., 10.],
        ),
        (
            "BT /F1 20 Tf 1 Tc 20 80 Td (BAAB) Tj ET",
            [20., 31., 38., 45.],
            [10., 6., 6., 10.],
        ),
    ] {
        let parsed = crate::parse_pdf_bytes(
            "owned-spaced-clusters.pdf",
            &pdf_with_content(content.as_bytes(), None),
        )
        .unwrap();
        assert_eq!(parsed.text_elements.len(), 4);
        for (i, e) in parsed.text_elements.iter().enumerate() {
            assert!((e.bounds.x0 - positions[i]).abs() < 0.001);
            assert!(
                (e.bounds.x1 - e.bounds.x0 - widths[i]).abs() < 0.001,
                "{e:?}"
            );
        }
    }
}

/// 同名・同じFontFile/Descriptorでも、別Font辞書のWidthsを混同しない。
#[test]
fn shared_fontfile_and_base_name_do_not_merge_different_resource_widths() {
    let content = b"BT /F1 20 Tf 20 80 Td (AA) Tj /F2 20 Tf 0 -40 Td (AA) Tj ET";
    let parsed = crate::parse_pdf_bytes(
        "owned-shared-fontfile.pdf",
        &pdf_with_content(content, Some(450)),
    )
    .unwrap();
    assert_eq!(parsed.text_elements.len(), 4);
    assert_ne!(
        parsed.text_elements[0].font_name,
        parsed.text_elements[2].font_name
    );
    for (e, width) in parsed.text_elements.iter().zip([6., 6., 9., 9.]) {
        assert_eq!(e.text, "f");
        assert!((e.bounds.x1 - e.bounds.x0 - width).abs() < 0.001);
        assert!(
            parsed
                .embedded_fonts
                .iter()
                .any(|f| Some(&f.font_name) == e.font_name.as_ref())
        );
    }
}

/// 現在のIRのfont選択はfamily名なので、一つの合字と二つのfへ同じrequired GSUBを当てない。
#[test]
fn identical_unicode_retains_distinct_encoded_glyph_clusters_for_docx() {
    let parsed = crate::parse_pdf_bytes("owned-source-clusters.pdf", &pdf()).unwrap();
    assert_eq!(parsed.text_elements.len(), 3);
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>(),
        ["ff", "f", "f"]
    );
    assert_ne!(
        parsed.text_elements[0].font_name, parsed.text_elements[1].font_name,
        "one source ff glyph and two source f glyphs need distinct effective font routes"
    );
    for element in &parsed.text_elements {
        let font = parsed
            .embedded_fonts
            .iter()
            .find(|font| Some(&font.font_name) == element.font_name.as_ref())
            .expect("each routed original glyph cluster must have a portable font");
        assert_eq!(font.bytes.get(..4), Some(b"OTTO".as_slice()));
    }
    for (element, expected) in parsed.text_elements.iter().zip([10., 6., 6.]) {
        assert!(
            (element.bounds.x1 - element.bounds.x0 - expected).abs() < 0.001,
            "original width must survive: {element:?}"
        );
    }
}
