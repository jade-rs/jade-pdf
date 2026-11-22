//! 罫線の可視性を推定する前に、固定SDKの公開APIの境界を自作PDFで確認する。
use anyhow::{ensure, Result};

/// 公開APIから得た、一つの自作罫線の属性。
#[derive(Debug)]
struct Observation {
    /// ページが生きている間だけ借用できるクリップハンドルの有無。
    clip_handle_present: bool,
    /// パス数。空の参照と、文字によるクリップを区別するために保持する。
    clip_paths: i32,
    /// 描画色の不透明度。
    alpha: u32,
    /// 透明度または通常以外の合成を持つか。
    transparent: bool,
    /// 内容マークの個数。未確認のOC等を推定で通さないために保持する。
    marks: i32,
}

/// 外部資料や字体を保存せず、クリップと描画状態だけが異なるページを作る。
fn source(mode: &str) -> Vec<u8> {
    let content = match mode {
        "path-clip" => "q 0 0 70 150 re W n 0 g 20 20 100 0.5 re f Q",
        "text-clip" => "BT /F1 12 Tf 7 Tr 20 100 Td (A) Tj ET 0 g 20 103 100 0.5 re f",
        "transparent" => "/GS gs 0 g 20 20 100 0.5 re f",
        "marked" => "/JadeUnknown BMC 0 g 20 20 100 0.5 re f EMC",
        "artifact" => "/Artifact BMC 0 g 20 20 100 0.5 re f EMC",
        _ => "0 g 20 20 100 0.5 re f",
    };
    owned_pdf(content)
}

/// 外部入力を保存せず、自作の描画内容から最小PDFを作る。
fn owned_pdf(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 4 0 R >> /ExtGState << /GS 5 0 R >> >> /Contents 6 0 R >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /ExtGState /ca 0.5 /CA 0.5 >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let mut out = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str("xref\n0 7\n0000000000 65535 f \n");
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    out.into_bytes()
}

/// 閉じた二行二列表ではセル壁を越えて行を結合せず、列より行を先に読む。
#[test]
fn extraction_respects_closed_cells_before_inline_merging_and_column_order() {
    for (left, right) in [(82, 102), (40, 125)] {
        let content = format!(
            "0 g 20 20 160 0.5 re f 20 100 160 0.5 re f 20 180 160 0.5 re f \
             20 20 0.5 160 re f 100 20 0.5 160 re f 180 20 0.5 160 re f \
             BT /F1 10 Tf 1 0 0 1 {left} 140 Tm (A0) Tj 1 0 0 1 {left} 60 Tm (A1) Tj \
             1 0 0 1 {right} 140 Tm (B0) Tj 1 0 0 1 {right} 60 Tm (B1) Tj ET"
        );
        let document =
            crate::load_document_from_bytes(&owned_pdf(&content), &crate::Limits::default())
                .unwrap();
        let page = &document.pages[0];
        assert_eq!(
            page.blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>(),
            ["A0", "B0", "A1", "B1"]
        );
        assert!(page.reading_order_requires_review);
    }
}

/// 物理単位と表示回転が変わっても、閉じた表の読み順と全スカラーを保持する。
#[test]
fn cell_order_survives_user_units_and_display_rotations() {
    let content = "0 g 20 20 160 0.5 re f 20 100 160 0.5 re f 20 180 160 0.5 re f \
        20 20 0.5 160 re f 100 20 0.5 160 re f 180 20 0.5 160 re f \
        BT /F1 10 Tf 40 140 Td (A0) Tj 0 -80 Td (A1) Tj ET \
        BT /F1 10 Tf 125 140 Td (B0) Tj 0 -80 Td (B1) Tj ET";
    for unit in [0.125, 1.0, 8.0] {
        for rotation in [0, 90, 180, 270] {
            let content = format!("q {} 0 0 {} 0 0 cm {content} Q", 1.0 / unit, 1.0 / unit);
            let bytes = owned_pdf(&content);
            let source = String::from_utf8(bytes).unwrap().replace(
                "/MediaBox [0 0 200 200]",
                &format!(
                    "/MediaBox [0 0 {} {}] /UserUnit {unit} /Rotate {rotation}",
                    200.0 / unit,
                    200.0 / unit
                ),
            );
            // 自作元はxrefを組み直す。SDKの破損xref修復をこの検証の前提にしない。
            let document =
                crate::load_document_from_bytes(&reindex(&source), &crate::Limits::default())
                    .unwrap();
            assert_eq!(
                document.pages[0]
                    .blocks
                    .iter()
                    .map(|b| b.text.as_str())
                    .collect::<Vec<_>>(),
                ["A0", "B0", "A1", "B1"],
                "unit={unit} rotate={rotation}"
            );
        }
    }
}

/// ページ辞書の長さ変更後にも、元の各オブジェクト位置を正確に書き直す。
fn reindex(source: &str) -> Vec<u8> {
    let mut result = source[..source.find("xref\n").unwrap()].to_owned();
    let xref = result.len();
    result.push_str("xref\n0 7\n0000000000 65535 f \n");
    for i in 1..=6 {
        let offset = source.find(&format!("{i} 0 obj\n")).unwrap();
        result.push_str(&format!("{offset:010} 00000 n \n"));
    }
    result.push_str(&format!(
        "trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    result.into_bytes()
}

/// 二つの囲み段落に横罫線がなければ、複数行を行優先に混ぜない。
#[test]
fn framed_columns_without_multiple_cell_rows_keep_paragraph_order() {
    let content = "0 g 20 20 160 0.5 re f 20 180 160 0.5 re f \
        20 20 0.5 160 re f 100 20 0.5 160 re f 180 20 0.5 160 re f \
        BT /F1 10 Tf 40 140 Td (A0) Tj 0 -80 Td (A1) Tj ET \
        BT /F1 10 Tf 125 140 Td (B0) Tj 0 -80 Td (B1) Tj ET";
    let document =
        crate::load_document_from_bytes(&owned_pdf(content), &crate::Limits::default()).unwrap();
    assert_eq!(
        document.pages[0]
            .blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>(),
        ["A0", "A1", "B0", "B1"]
    );
}

/// 成否を返す前にページを閉じ、テストのassertでSDKの所有を残さない。
fn observe(mode: &str) -> Observation {
    let bytes = source(mode);
    let pdfium = crate::render::bind().unwrap();
    crate::input::with_raw_bytes(&pdfium, &bytes, &crate::Limits::default(), |document| {
        let b = pdfium.bindings();
        let page = b.FPDF_LoadPage(document.cast(), 0);
        ensure!(!page.is_null(), "Cannot open owned clip page");
        let result: Result<Observation> = (|| {
            let count = b.FPDFPage_CountObjects(page);
            ensure!((1..=4).contains(&count), "Unexpected owned object count");
            let paths: Vec<_> = (0..count)
                .map(|i| b.FPDFPage_GetObject(page, i))
                .filter(|&object| !object.is_null() && b.FPDFPageObj_GetType(object) == 2)
                .collect();
            ensure!(paths.len() == 1, "Missing owned ruling");
            let object = paths[0];
            let clip = b.FPDFPageObj_GetClipPath(object);
            let (mut r, mut g, mut blue, mut alpha) = (0, 0, 0, 0);
            ensure!(
                b.FPDFPageObj_GetFillColor(object, &mut r, &mut g, &mut blue, &mut alpha) != 0,
                "Cannot inspect owned fill"
            );
            ensure!([r, g, blue] == [0, 0, 0], "Owned fill differs");
            Ok(Observation {
                clip_handle_present: !clip.is_null(),
                clip_paths: b.FPDFClipPath_CountPaths(clip),
                alpha,
                transparent: b.FPDFPageObj_HasTransparency(object) != 0,
                marks: b.FPDFPageObj_CountMarks(object),
            })
        })();
        // GetClipPath is borrowed; only the page/document owns its lifetime.
        b.FPDF_ClosePage(page);
        result
    })
    .unwrap()
}

/// 有効な借用ハンドルでも、幾何パスゼロの文字クリップを空の状態と混同しない。
#[test]
fn empty_reference_path_clip_and_text_clip_are_distinct() {
    let none = observe("plain");
    assert!(none.clip_handle_present);
    // Fixed8111 fpdf_transformpage.cpp: !HasRef returns -1. This is not a
    // general public-contract proof that an arbitrary SDK error means no clip.
    assert_eq!(none.clip_paths, -1);
    let path = observe("path-clip");
    assert!(path.clip_handle_present);
    assert!(path.clip_paths > 0);
    let text = observe("text-clip");
    assert!(text.clip_handle_present);
    assert_eq!(text.clip_paths, 0);
}

/// 黒い幾何形状があっても、透明な描画や未確認マークを既定の罫線にしない。
#[test]
fn opacity_and_marks_are_independent_of_black_geometry() {
    let plain = observe("plain");
    assert_eq!(plain.alpha, 255);
    assert!(!plain.transparent);
    assert_eq!(plain.marks, 0);
    let transparent = observe("transparent");
    assert!(transparent.alpha < 255);
    assert!(transparent.transparent);
    assert!(observe("marked").marks > 0);
}

/// 同じ矩形でも、公開APIで確認できた不透明で無条件の描画だけを候補にする。
#[test]
fn ruling_reader_rejects_clipped_transparent_and_unknown_marked_material() {
    for (mode, expected) in [
        ("plain", 1),
        ("artifact", 1),
        ("path-clip", 0),
        ("text-clip", 0),
        ("transparent", 0),
        ("marked", 0),
    ] {
        let pdfium = crate::render::bind().unwrap();
        crate::input::with_raw_bytes(
            &pdfium,
            &source(mode),
            &crate::Limits::default(),
            |document| {
                let b = pdfium.bindings();
                let page = b.FPDF_LoadPage(document.cast(), 0);
                ensure!(!page.is_null(), "Cannot open owned ruling page");
                let lines = super::reader::read(&pdfium, page.cast(), 1.0);
                b.FPDF_ClosePage(page);
                ensure!(
                    lines.as_ref().is_some_and(|l| l.len() == expected),
                    "{mode}: wrong ruling candidates"
                );
                Ok(())
            },
        )
        .unwrap();
    }
}
