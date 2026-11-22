//! 外部資料を保存せず、同じ公開APIの期待をnativeと実ブラウザーで確認する。

/// 指定した描画状態の表を、すべて自作のPDFオブジェクトから生成する。
pub fn fixture(mode: &str, unit: f64, rotation: u16, revised: bool) -> Vec<u8> {
    let mut rules = String::from(
        "0 g 20 20 160 0.5 re f 20 180 160 0.5 re f \
        20 20 0.5 160 re f 100 20 0.5 160 re f 180 20 0.5 160 re f ",
    );
    if mode != "paragraphs" {
        rules.push_str("20 100 160 0.5 re f ");
    }
    if mode == "header" {
        rules.push_str("20 205 80 0.5 re f 20 180 0.5 25 re f 100 180 0.5 25 re f ");
    }
    rules = match mode {
        "clip" => format!("q 0 0 150 190 re W n {rules} Q"),
        "transparent" => format!("q /GS gs {rules} Q"),
        "marked" => format!("/Unknown BMC {rules} EMC"),
        "artifact" => format!("/Artifact BMC {rules} EMC"),
        _ => rules,
    };
    let (left, right) = if matches!(
        mode,
        "plain" | "artifact" | "header" | "paragraphs" | "edit" | "actual"
    ) {
        (40, 125)
    } else {
        (82, 102)
    };
    let last = if revised { "B2" } else { "B1" };
    let header = if mode == "header" {
        "BT /F1 10 Tf 40 190 Td (Title) Tj ET BT /F1 10 Tf 125 190 Td (Date) Tj ET"
    } else {
        ""
    };
    let body = if mode == "multiline" {
        "BT /F1 10 Tf 25 160 Td (L0\\() Tj 0 -30 Td (L1\\() Tj ET BT /F1 10 Tf 90 160 Td (\\)) Tj 0 -30 Td (\\)) Tj ET BT /F1 10 Tf 125 140 Td (B0) Tj ET BT /F1 10 Tf 40 60 Td (A1) Tj ET BT /F1 10 Tf 125 60 Td (B1) Tj ET".into()
    } else if mode == "actual" {
        // 一つの原文置換領域が左右のセルにまたがる。意味をセルごとに切り分けない。
        "/Span << /ActualText <FEFF0052006500670069006F006E> >> BDC BT /F1 10 Tf 40 140 Td (A0) Tj ET BT /F1 10 Tf 125 140 Td (B0) Tj ET EMC".into()
    } else {
        format!("BT /F1 10 Tf {left} 140 Td (A0) Tj 0 -80 Td (A1) Tj ET BT /F1 10 Tf {right} 140 Td (B0) Tj 0 -80 Td ({last}) Tj ET")
    };
    let content = format!(
        "q {} 0 0 {} 0 0 cm {rules} {header} {body} Q",
        1.0 / unit,
        1.0 / unit
    );
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> /ExtGState << /GS 5 0 R >> >> /Contents 6 0 R >>",220.0/unit,220.0/unit),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /ExtGState /ca 0.5 /CA 0.5 >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream",content.len()),
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

/// IRと文字レイヤの順序・スカラー・セル数を、事前宣言した自作の原文と照合する。
pub fn check() -> Result<usize, String> {
    let mut count = 0;
    for mode in [
        "plain",
        "artifact",
        "clip",
        "transparent",
        "marked",
        "paragraphs",
        "header",
        "actual",
        "multiline",
    ] {
        for unit in [0.125, 1.0, 8.0] {
            for rotation in [0, 90, 180, 270] {
                let bytes = fixture(mode, unit, rotation, false);
                let ir = pdf_diff::load_document_from_bytes(&bytes, &pdf_diff::Limits::default())
                    .map_err(|e| e.to_string())?;
                let layer =
                    pdf_diff::text_layer_from_bytes(&bytes, 1).map_err(|e| e.to_string())?;
                let expected: &[&str] = match mode {
                    "clip" | "transparent" | "marked" => &["A0 B0", "A1 B1"],
                    "paragraphs" => &["A0", "A1", "B0", "B1"],
                    "header" => &["Title", "Date", "A0", "B0", "A1", "B1"],
                    "actual" => &["Region"],
                    "multiline" => &["L0(", ")", "L1(", ")", "B0", "A1", "B1"],
                    _ => &["A0", "B0", "A1", "B1"],
                };
                if ir.pages[0]
                    .blocks
                    .iter()
                    .map(|b| b.text.as_str())
                    .collect::<Vec<_>>()
                    != expected
                    || layer.iter().map(|b| b.text.as_str()).collect::<Vec<_>>() != expected
                    || layer
                        .iter()
                        .any(|b| b.text.chars().count() != b.chars.len())
                {
                    return Err(format!("{mode}, unit={unit}, rotate={rotation}: IR/layer differ from {expected:?}; actual {:?}",ir.pages[0].blocks.iter().map(|b|b.text.as_str()).collect::<Vec<_>>()));
                }
                if matches!(mode, "plain" | "artifact" | "header")
                    && !ir.pages[0].reading_order_requires_review
                {
                    return Err("Inferred table lost review guard".into());
                }
                count += 1;
            }
        }
    }
    let old = fixture("edit", 1.0, 0, false);
    let new = fixture("edit", 1.0, 0, true);
    let diff = pdf_diff::diff_bytes(
        "old",
        "new",
        &old,
        &new,
        &pdf_diff::Limits::default(),
        &pdf_diff::DiffOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    if diff.summary.blocks_changed != 1
        || diff.summary.blocks_added != 0
        || diff.summary.blocks_removed != 0
    {
        return Err("A single cell edit became insert/delete or multiple edits".into());
    }
    Ok(count + 1)
}
