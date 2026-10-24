//! 契約書の比較・描画に使う入力をメモリ内で組み立てる。

/// 3 ページの契約書と、変更・挿入を含む 4 ページ版を生成する。
pub fn bytes(revised: bool) -> Vec<u8> {
    let mut pages = vec![vec![
        "業務委託契約書".to_owned(),
        format!(
            "Article 2: {} yen",
            if revised { "550,000" } else { "500,000" }
        ),
        format!("Article 3: {} months", if revised { 24 } else { 12 }),
    ]];
    if revised {
        pages[0].push("Article 4: Confidentiality".into());
        pages.push(vec!["Preface: New requirements".into()]);
    }
    pages.push(vec![
        "Appendix A: Services".into(),
        if revised {
            "1. Planning 2. Design 3. Delivery 4. Support".into()
        } else {
            "1. Planning 2. Design 3. Delivery".into()
        },
    ]);
    pages.push(vec![
        "Appendix B: Contact information".into(),
        "Contact: example@example.com".into(),
    ]);
    let kids = (0..pages.len())
        .map(|i| format!("{} 0 R", 5 + 2 * i))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /Font /Subtype /Type0 /BaseFont /HeiseiKakuGo-W5 /Encoding /UniJIS-UCS2-H /DescendantFonts [<< /Type /Font /Subtype /CIDFontType0 /BaseFont /HeiseiKakuGo-W5 /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 2 >> /DW 1000 >>] >>".into(),
    ];
    for (index, lines) in pages.iter().enumerate() {
        let mut content = String::new();
        for (line, text) in lines.iter().enumerate() {
            let y = 760 - line * 32;
            if text.is_ascii() {
                content.push_str(&format!("BT /F1 14 Tf 60 {y} Td ({text}) Tj ET\n"));
            } else {
                let hex = text
                    .encode_utf16()
                    .map(|c| format!("{c:04X}"))
                    .collect::<String>();
                content.push_str(&format!("BT /F2 18 Tf 60 {y} Td <{hex}> Tj ET\n"));
            }
        }
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595.276 841.89] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>", 6 + index * 2));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ));
    }
    let mut source = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = source.len();
    let size = objects.len() + 1;
    source.push_str(&format!("xref\n0 {size}\n0000000000 65535 f \n"));
    for offset in offsets {
        source.push_str(&format!("{offset:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    source.into_bytes()
}

/// 子プロセスの実行中も存続する一時ディレクトリへ生成入力を配置する。
#[cfg(not(target_arch = "wasm32"))]
pub fn path(name: &str) -> std::path::PathBuf {
    static DIRECTORY: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let directory = DIRECTORY.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        for (name, revised) in [("report-old.pdf", false), ("report-new.pdf", true)] {
            std::fs::write(directory.path().join(name), bytes(revised)).unwrap();
        }
        directory
    });
    assert!(matches!(name, "report-old.pdf" | "report-new.pdf"));
    directory.path().join(name)
}
