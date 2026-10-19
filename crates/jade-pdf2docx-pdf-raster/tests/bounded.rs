//! 事前検査した寸法を描画コマンドとPNG検証の両方で守る。
use jade_pdf2docx_pdf_raster::render_pdf_page_to_png_bounded;
use std::{fs, path::Path};

/// 不正な容量・辺長・ページ番号は外部コマンドや出力ディレクトリ作成前に拒否する。
#[test]
fn invalid_raster_budget_fails_before_spawn() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("out");
    for (dpi, page, width, height, budget) in [
        (0, 0, 10, 10, 121),
        (72, usize::MAX, 10, 10, 121),
        (72, 0, 0, 10, 121),
        (72, 0, u32::MAX, 10, u64::MAX),
        (72, 0, 10, 10, 120),
    ] {
        let error = render_pdf_page_to_png_bounded(
            Path::new("missing.pdf"),
            &out,
            dpi,
            page,
            width,
            height,
            budget,
        )
        .unwrap_err();
        assert!(error.to_string().contains("raster budget"), "{error}");
        assert!(!out.exists());
    }
}

/// 端数のあるCropBoxと回転を持つ正式xref付きPDFを生成する。
fn rotated_pdf(rotation: i32) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /CropBox [1.25 2.25 101.5 52.5] /Rotate {rotation} /Contents 4 0 R >>"
        ),
        "<< /Length 0 >>\nstream\n\nendstream".to_owned(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

/// CropBoxの原点・端数・回転を含むページでも全描画寸法を保持する。
#[test]
#[ignore = "requires real pdftoppm; OCR workflow invokes explicitly"]
fn fractional_crop_and_rotation_keep_full_dimensions() {
    for rotation in [0, 90, 180, 270, -90] {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("rotated.pdf");
        fs::write(&input, rotated_pdf(rotation)).unwrap();
        let (width, height) = if rotation % 180 == 0 {
            (209, 105)
        } else {
            (105, 209)
        };
        let path = render_pdf_page_to_png_bounded(
            &input,
            directory.path(),
            150,
            0,
            width,
            height,
            u64::from(width + 1) * u64::from(height + 1),
        )
        .unwrap();
        assert_eq!(
            jade_pdf2docx_pdf_raster::png_dimensions(&fs::read(path).unwrap()),
            Some((width, height)),
            "rotation={rotation}"
        );
    }
}
