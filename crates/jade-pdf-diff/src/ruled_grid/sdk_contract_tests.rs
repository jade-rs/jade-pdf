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
        _ => "0 g 20 20 100 0.5 re f",
    };
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
