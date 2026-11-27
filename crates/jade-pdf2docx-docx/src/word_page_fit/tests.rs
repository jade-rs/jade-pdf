//! 物理長さの縮小、通常sectionと共有partの独立性を検証する。
use super::*;

/// 最小の制御OOXMLを指定倍率で変換する。
fn scaled(xml: &str) -> String {
    String::from_utf8(
        xml::scale(
            xml.as_bytes(),
            &[Plan {
                longest: 40000,
                ..Plan::default()
            }],
            false,
        )
        .unwrap(),
    )
    .unwrap()
}

/// 表の罫線、セル寸法、段落、描画寸法は縮め、百分率と文字列は変えない。
#[test]
fn geometry_units_are_scaled_without_touching_percentages_or_text() {
    let source = r#"<w:hdr><w:tbl><w:tblPr><w:tblW w:w="5000" w:type="pct"/><w:tblBorders><w:left w:val="single" w:sz="20" w:space="5"/></w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="10000"/></w:tblGrid><w:tr><w:trPr><w:trHeight w:val="1000"/></w:trPr><w:tc><w:tcPr><w:tcW w:w="10000" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:ind w:left="1000" w:firstLineChars="100"/><w:spacing w:line="240" w:lineRule="auto"/></w:pPr><w:r><w:rPr/><w:t>10000 &amp; exact</w:t><wp:extent cx="100000" cy="50000"/><a:xfrm rot="5400000"><a:off x="-1000" y="0"/><a:ext cx="100000" cy="50000"/></a:xfrm></w:r></w:p></w:tc></w:tr></w:tbl></w:hdr>"#;
    let out = scaled(source);
    for expected in [
        r#"w:w="5000" w:type="pct""#,
        r#"w:sz="16""#,
        r#"w:space="4""#,
        r#"w:w="7920""#,
        r#"w:val="792""#,
        r#"w:left="792""#,
        r#"w:firstLineChars="100""#,
        r#"w:line="240""#,
        r#"cx="79200" cy="39600""#,
        r#"x="-792" y="0""#,
        r#"rot="5400000""#,
        r#"10000 &amp; exact"#,
    ] {
        assert!(out.contains(expected), "missing {expected}: {out}");
    }
}

/// complex script側のサイズを省略したrunでも、同じ元font sizeを継承する。
#[test]
fn omitted_complex_script_size_inherits_explicit_run_size() {
    let out = scaled(
        r#"<w:hdr><w:p><w:r><w:rPr><w:sz w:val="40"/></w:rPr><w:t>مرحبا</w:t></w:r></w:p></w:hdr>"#,
    );
    assert!(out.contains(r#"<w:szCs w:val="32"/>"#), "{out}");
}

/// 原本の挿入幅をページと同じ割合で縮め、fitTextの識別子は維持する。
#[test]
fn fitted_run_width_scales_with_the_page() {
    let out = scaled(
        r#"<w:hdr><w:p><w:r><w:rPr><w:fitText w:val="1000" w:id="42"/></w:rPr><w:t>様A第5号</w:t></w:r></w:p></w:hdr>"#,
    );
    assert!(out.contains(r#"<w:fitText w:val="792" w:id="42""#), "{out}");
}

/// section属性を持つ段落の本文も前sectionに属し、次段落から倍率を変える。
#[test]
fn section_boundary_preserves_paragraph_ownership_and_order() {
    let input=br#"<w:document><w:body><w:p><w:pPr><w:sectPr><w:pgSz w:w="40000" w:h="20000"/></w:sectPr></w:pPr><w:r><w:rPr><w:sz w:val="40"/></w:rPr><w:t>first</w:t></w:r></w:p><w:p><w:r><w:rPr><w:sz w:val="40"/></w:rPr><w:t>second</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="20000" w:h="10000"/></w:sectPr></w:body></w:document>"#;
    let out = String::from_utf8(xml::scale(input, &plans(input).unwrap(), true).unwrap()).unwrap();
    assert_eq!(out.matches(r#"<w:sz w:val="32"/>"#).count(), 1);
    assert_eq!(out.matches(r#"<w:sz w:val="40"/>"#).count(), 1);
    assert!(out.find("first").unwrap() < out.find("second").unwrap());
}

/// 二度目の変換は全partのbyte列を一切変更せず、通常サイズは初回から変更しない。
#[test]
fn package_fit_is_idempotent_and_normal_packages_are_identical() {
    let mut normal = docx_rs::Docx::new().build();
    let before = normal.document.clone();
    let rels = normal.document_rels.clone();
    fit(&mut normal).unwrap();
    assert_eq!(normal.document, before);
    assert_eq!(normal.document_rels, rels);
    normal.document=br#"<w:document><w:body><w:p><w:r><w:rPr/><w:t>wide</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="40000" w:h="20000"/></w:sectPr></w:body></w:document>"#.to_vec();
    fit(&mut normal).unwrap();
    let once = normal.document.clone();
    fit(&mut normal).unwrap();
    assert_eq!(normal.document, once);
}

/// ヘッダーを共有した大小sectionで、原part・画像payload・通常sectionを保持する。
#[test]
fn inherited_headers_and_footers_are_cloned_for_scaled_sections_only() {
    let mut package = docx_rs::Docx::new().build();
    package.document=br#"<w:document><w:body><w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="H"/><w:footerReference w:type="default" r:id="F"/><w:pgSz w:w="40000" w:h="20000"/></w:sectPr></w:pPr><w:r><w:rPr/><w:t>wide</w:t></w:r></w:p><w:p><w:r><w:rPr/><w:t>normal</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="20000" w:h="10000"/></w:sectPr></w:body></w:document>"#.to_vec();
    package.document_rels=br#"<Relationships><Relationship Id="H" Target="header1.xml"/><Relationship Id="F" Target="footer1.xml"/></Relationships>"#.to_vec();
    package.headers=vec![br#"<w:hdr><w:p><w:r><w:rPr><w:sz w:val="40"/></w:rPr><w:t>header</w:t></w:r></w:p></w:hdr>"#.to_vec()];
    package.footers =
        vec![br#"<w:ftr><w:p><w:r><w:rPr/><w:t>footer</w:t></w:r></w:p></w:ftr>"#.to_vec()];
    package.header_rels = vec![b"owned header rel".to_vec()];
    package.media = vec![("owned.png".into(), vec![1, 2, 3])];
    let original = package.headers[0].clone();
    let footer = package.footers[0].clone();
    fit(&mut package).unwrap();
    assert_eq!(package.headers.len(), 2);
    assert_eq!(package.footers.len(), 2);
    assert_eq!(package.headers[0], original);
    assert_eq!(package.footers[0], footer);
    assert!(String::from_utf8_lossy(&package.headers[1]).contains(r#"<w:sz w:val="32"/>"#));
    assert_eq!(package.header_rels[1], package.header_rels[0]);
    assert_eq!(package.media, vec![("owned.png".into(), vec![1, 2, 3])]);
    let out = String::from_utf8(package.document.clone()).unwrap();
    assert!(
        out.find("w:headerReference").unwrap() < out.find("w:footerReference").unwrap(),
        "section references must retain schema order: {out}"
    );
    assert!(out.contains(r#"r:id="jadePageFitheader2""#));
    assert!(out.contains(r#"r:id="H""#));
    assert!(out.contains(r#"r:id="jadePageFitfooter2""#));
    assert!(out.contains(r#"r:id="F""#));
    let before = (
        package.document.clone(),
        package.headers.clone(),
        package.document_rels.clone(),
        package.content_type.clone(),
    );
    fit(&mut package).unwrap();
    assert_eq!(
        (
            package.document,
            package.headers,
            package.document_rels,
            package.content_type
        ),
        before
    );
}

proptest::proptest! {
    /// 上限を超える全辺で長辺が正確に上限となり、最小1twipへの丸めを含めて短辺の誤差は1twip以内に収まる。
    #[test]
    fn proportional_fit_has_bounded_rounding(longest in 31681u32..=u32::MAX, short in 1u32..=31680u32) {
        let plan=Plan {longest,..Plan::default()};
        proptest::prop_assert_eq!(plan.length(i64::from(longest),true),i64::from(MAX_TWIPS));
        let expected=f64::from(short)*f64::from(MAX_TWIPS)/f64::from(longest);
        let actual=plan.length(i64::from(short),true);
        proptest::prop_assert!((actual as f64-expected).abs()<=1.0);
        proptest::prop_assert_eq!(plan.length(-i64::from(short),false),-plan.length(i64::from(short),false));
        proptest::prop_assert_eq!(plan.length(0,false),0);
    }
}
