use pdf_diff::{
    diff_bytes, load_document_from_bytes, render_page_at_dpi_from_bytes, render_page_from_bytes,
    Bounds, DiffOptions, DocumentIr, Limits, PageViewport,
};
use pdfium_render::prelude::{PdfPoints, PdfRenderConfig};

// Independent PDF objects and painted landmarks; no PDF writer or library under
// test is used to create the input. The page tree exercises inherited boxes too.
fn pdf(media: &str, crop: &str, rotation: u64, inherited: bool, shift: f64, unit: u32) -> Vec<u8> {
    let geometry = format!("/MediaBox [{media}] /CropBox [{crop}] /Rotate {rotation}");
    let stream = format!(
        "q 1 0 0 1 {shift} {shift} cm 0 g 60 80 40 20 re f 0 0 1 rg 180 290 20 30 re f \
         1 0 0 rg BT /F1 10 Tf 60 125 Td (MARK) Tj ET Q"
    );
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [3 0 R] /Count 1 {} >>", if inherited { &geometry } else { "" }),
        format!("<< /Type /Page /Parent 2 0 R {} /UserUnit {unit} /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>", if inherited { "" } else { &geometry }),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
    ];
    let mut source = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = source.len();
    source.push_str("xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        source.push_str(&format!("{offset:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    source.into_bytes()
}

fn assert_close(a: Bounds, b: Bounds, tolerance: f64) {
    for (a, b) in [(a.x0, b.x0), (a.y0, b.y0), (a.x1, b.x1), (a.y1, b.y1)] {
        assert!(
            (a - b).abs() <= tolerance,
            "{a} != {b} (tolerance {tolerance})"
        );
    }
}

fn painted_bounds(png: &[u8], blue: bool) -> Bounds {
    let image = image::load_from_memory(png).unwrap().to_rgb8();
    let mut bounds = Bounds {
        x0: f64::INFINITY,
        y0: f64::INFINITY,
        x1: 0.0,
        y1: 0.0,
    };
    for (x, y, pixel) in image.enumerate_pixels() {
        let [r, g, b] = pixel.0;
        if r < 30 && g < 30 && (if blue { b > 225 } else { b < 30 }) {
            bounds.x0 = bounds.x0.min(f64::from(x));
            bounds.y0 = bounds.y0.min(f64::from(y));
            bounds.x1 = bounds.x1.max(f64::from(x + 1));
            bounds.y1 = bounds.y1.max(f64::from(y + 1));
        }
    }
    assert!(bounds.x0.is_finite(), "Painted landmark is missing");
    bounds
}

#[test]
fn viewport_matches_actual_painted_pixels_and_pdfium_page_to_device() {
    let cases = [
        ("-50 -80 350 420", "30 40 230 340", 0.0, 1),
        ("-450 -480 -50 20", "-370 -360 -170 -60", -400.0, 1),
        ("-50 -80 350 420", "30 40 500 500", 0.0, 1),
        (
            "-50.25 -80.5 350.5 420.25",
            "30.1 40.2 230.35 340.6",
            0.0,
            1,
        ),
        ("-50 -80 350 420", "30 40 230 340", 0.0, 3),
        (
            "99950 99920 100350 100420",
            "100030 100040 100230 100340",
            100_000.0,
            1,
        ),
    ];
    for (media, crop, shift, unit) in cases {
        for rotation in [0, 90, 180, 270] {
            let bytes = pdf(media, crop, rotation, rotation != 0, shift, unit);
            let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
            for dpi in [None, Some(144)] {
                let rendered = if let Some(dpi) = dpi {
                    render_page_at_dpi_from_bytes(&bytes, 1, dpi).unwrap()
                } else {
                    render_page_from_bytes(&bytes, 1, 400).unwrap()
                };
                let viewport = rendered.viewport;
                assert_eq!(
                    ir.pages[0]
                        .viewport(rendered.width, rendered.height)
                        .unwrap(),
                    viewport
                );
                assert_eq!(viewport.rotation, rotation);
                assert_eq!(viewport.user_unit, Some(f64::from(unit)));
                for (blue, bounds) in [
                    (
                        false,
                        Bounds {
                            x0: 60.0 + shift,
                            y0: 80.0 + shift,
                            x1: 100.0 + shift,
                            y1: 100.0 + shift,
                        },
                    ),
                    (
                        true,
                        Bounds {
                            x0: 180.0 + shift,
                            y0: 290.0 + shift,
                            x1: 200.0 + shift,
                            y1: 320.0 + shift,
                        },
                    ),
                ] {
                    let expected = viewport.clipped_bounds(bounds).unwrap();
                    assert_close(painted_bounds(&rendered.png, blue), expected, 1.0);
                    let center = bounds.center();
                    let pixel = viewport.pdf_to_pixel.transform_point(center.0, center.1);
                    let restored = viewport
                        .pdf_to_pixel
                        .inverse()
                        .unwrap()
                        .transform_point(pixel.0, pixel.1);
                    assert!(
                        (restored.0 - center.0).abs() < 1e-7
                            && (restored.1 - center.1).abs() < 1e-7
                    );
                }
                // No nested library bind while a Pdfium handle is held: its
                // thread_safe feature locks the engine for the handle lifetime.
                let pdfium = pdfium_render::prelude::Pdfium::bind_to_library(
                    pdf_diff::pdfium_library_path().unwrap(),
                )
                .map(pdfium_render::prelude::Pdfium::new)
                .unwrap();
                let document = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
                let page = document.pages().get(0).unwrap();
                let config = if dpi.is_some() {
                    PdfRenderConfig::new().scale_page_by_factor(2.0 * unit as f32)
                } else {
                    PdfRenderConfig::new().set_target_width(400)
                };
                let b = viewport.view_box;
                for (x, y) in [
                    (b.x0, b.y0),
                    (b.x1, b.y1),
                    b.center(),
                    (60.0 + shift, 80.0 + shift),
                ] {
                    let actual = page
                        .points_to_pixels(
                            PdfPoints::new(x as f32),
                            PdfPoints::new(y as f32),
                            &config,
                        )
                        .unwrap();
                    let projected = viewport.pdf_to_pixel.transform_point(x, y);
                    assert!(
                        (f64::from(actual.0) - projected.0).abs() <= 1.0
                            && (f64::from(actual.1) - projected.1).abs() <= 1.0,
                        "rotation={rotation}, native={actual:?}, projection={projected:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn crop_origin_change_is_reported_even_with_equal_page_dimensions_and_text() {
    let old = pdf("-50 -80 350 420", "30 40 230 340", 0, false, 0.0, 1);
    let new = pdf("-50 -80 350 420", "40 50 240 350", 0, false, 0.0, 1);
    let diff = diff_bytes(
        "a",
        "b",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(diff.summary.pages_geometry_changed, 1);
    assert_eq!(diff.summary.pages_modified, 1);
    assert_eq!(diff.summary.blocks_changed, 0);
    assert_eq!(diff.summary.blocks_added, 0);
    assert_eq!(diff.summary.blocks_removed, 0);
    assert_ne!(
        diff.pages[0].old_size.unwrap().view_box,
        diff.pages[0].new_size.unwrap().view_box
    );
}

#[test]
fn legacy_ir_defaults_to_unrotated_origin_and_crop_furniture_uses_real_origin() {
    let ir: DocumentIr = serde_json::from_str(
        r#"{"pages":[{"index":1,"width":300,"height":200,"rotation":90,"blocks":[] }]}"#,
    )
    .unwrap();
    ir.validate(&Limits::default()).unwrap();
    assert_eq!(ir.pages[0].view_box, None);
    assert_eq!(ir.pages[0].user_unit, Some(1.0));
    for unit in [0.0, -1.0, 75_001.0, f64::NAN, f64::INFINITY] {
        let mut invalid = ir.clone();
        invalid.pages[0].user_unit = Some(unit);
        assert!(invalid.validate(&Limits::default()).is_err());
    }
    assert_eq!(
        ir.pages[0].effective_view_box(),
        Bounds {
            x0: 0.0,
            y0: 0.0,
            x1: 200.0,
            y1: 300.0
        }
    );
    for rotation in [0, 90, 180, 270] {
        let bytes = pdf("-50 -80 350 420", "30 100 230 200", rotation, true, 0.0, 1);
        let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
        let page = &ir.pages[0];
        assert_eq!(
            page.body_text(),
            "MARK",
            "Body was treated as furniture at rotation {rotation}"
        );
    }
}

#[test]
fn viewport_clips_off_page_highlights_and_rejects_invalid_geometry() {
    let b = Bounds {
        x0: 30.0,
        y0: 40.0,
        x1: 230.0,
        y1: 340.0,
    };
    let viewport = PageViewport::new(b, 0, 400, 600).unwrap();
    assert_eq!(
        viewport.clipped_bounds(Bounds {
            x0: 20.0,
            y0: 30.0,
            x1: 40.0,
            y1: 50.0
        }),
        Some(Bounds {
            x0: 0.0,
            y0: 580.0,
            x1: 20.0,
            y1: 600.0
        })
    );
    assert!(viewport
        .clipped_bounds(Bounds {
            x0: -100.0,
            x1: -90.0,
            ..b
        })
        .is_none());
    assert!(PageViewport::new(b, 45, 400, 600).is_err());
    assert!(PageViewport::new(b, 0, 0, 600).is_err());
    for bad in [Bounds { x0: f64::NAN, ..b }, Bounds { x1: 30.0, ..b }] {
        assert!(PageViewport::new(bad, 0, 400, 600).is_err());
        assert!(viewport.clipped_bounds(bad).is_none());
    }
}

#[test]
fn geometry_edits_match_pages_with_only_furniture_or_no_text() {
    let old = pdf("-50 -80 350 420", "30 110 230 120", 0, false, 0.0, 1);
    let new = pdf("-50 -80 350 420", "40 115 240 125", 0, false, 0.0, 1);
    let mut old = load_document_from_bytes(&old, &Limits::default()).unwrap();
    let mut new = load_document_from_bytes(&new, &Limits::default()).unwrap();
    assert!(old.pages[0].body_text().is_empty());
    assert!(!old.pages[0].blocks.is_empty());
    for blank in [false, true] {
        if blank {
            old.pages[0].blocks.clear();
            new.pages[0].blocks.clear();
        }
        let diff = pdf_diff::diff_documents("a", "b", &old, &new, &DiffOptions::default());
        assert_eq!(diff.summary.pages_geometry_changed, 1);
        assert_eq!(diff.summary.pages_modified, 1);
        assert_eq!(diff.summary.pages_added, 0);
        assert_eq!(diff.summary.pages_removed, 0);
        if blank {
            assert!(!diff.is_text_comparison_complete());
            assert_eq!(diff.pages[0].match_confidence, None);
        }
    }
}

#[test]
fn width_rendering_rejects_excessive_height_and_total_pixels_before_allocation() {
    for media in ["0 0 1 100000", "0 0 100000 1", "0 0 2000 2001", "0 0 0 0"] {
        let bytes = pdf(media, media, 0, false, 0.0, 1);
        assert!(
            render_page_from_bytes(&bytes, 1, 8192).is_err(),
            "Accepted {media}"
        );
    }
    let valid = pdf("0 0 600 800", "0 0 600 800", 0, false, 0.0, 1);
    assert!(render_page_from_bytes(&valid, 1, 400).is_ok());
}

#[test]
fn repeated_pages_prefer_matching_crop_geometry_after_reordering() {
    let a = pdf("-50 -80 350 420", "30 40 230 340", 0, false, 0.0, 1);
    let b = pdf("-50 -80 350 420", "60 60 150 150", 0, false, 0.0, 1);
    let a = load_document_from_bytes(&a, &Limits::default())
        .unwrap()
        .pages
        .remove(0);
    let b = load_document_from_bytes(&b, &Limits::default())
        .unwrap()
        .pages
        .remove(0);
    for physical_only in [false, true] {
        let b = if physical_only {
            let mut page = a.clone();
            page.user_unit = Some(3.0);
            page
        } else {
            b.clone()
        };
        let mut old = DocumentIr {
            pages: vec![a.clone(), a.clone(), b.clone(), b],
        };
        for (i, page) in old.pages.iter_mut().enumerate() {
            page.index = i + 1;
        }
        let mut new = old.clone();
        new.pages.reverse();
        for (i, page) in new.pages.iter_mut().enumerate() {
            page.index = i + 1;
        }
        let result = pdf_diff::diff_documents("a", "b", &old, &new, &DiffOptions::default());
        assert_eq!(
            result.summary.pages_modified, 0,
            "physical_only={physical_only}"
        );
        assert_eq!(
            result.summary.pages_geometry_changed, 0,
            "physical_only={physical_only}"
        );
        assert!(result.summary.pages_moved > 0);
    }
}

#[test]
fn physical_unit_changes_are_geometry_changes_across_bytes_files_and_cli() {
    let old = pdf("0 0 300 300", "0 0 300 300", 0, false, 0.0, 1);
    let new = pdf("0 0 300 300", "0 0 300 300", 0, false, 0.0, 3);
    let diff = diff_bytes(
        "old",
        "new",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(diff.summary.pages_geometry_changed, 1);
    assert_eq!(diff.summary.pages_visual_changed, 1);
    assert_eq!(diff.summary.blocks_changed, 0);
    // Raw font sizes stay equal, but the physical font becomes three times
    // larger. This differs from inversely rebasing coordinates and UserUnit.
    assert_eq!(diff.summary.blocks_format_changed, 1);
    let block = &diff.pages[0].blocks[0];
    assert_eq!(block.old_font_size, block.new_font_size);
    assert!(diff.has_changes() && diff.is_comparison_complete());
    assert_eq!(diff.pages[0].new_size.unwrap().user_unit, Some(3.0));
    let old_file = tempfile::NamedTempFile::new().unwrap();
    let new_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(old_file.path(), &old).unwrap();
    std::fs::write(new_file.path(), &new).unwrap();
    let from_files = pdf_diff::diff_files(
        old_file.path(),
        new_file.path(),
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(from_files.summary.pages_geometry_changed, 1);
    assert_eq!(
        serde_json::to_value(&from_files.summary).unwrap(),
        serde_json::to_value(&diff.summary).unwrap()
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
        .args([old_file.path(), new_file.path()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let cli: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cli["summary"]["pages_geometry_changed"], 1);
    assert_eq!(cli["summary"]["blocks_format_changed"], 1);
    let rendered = pdf_diff::render_page_at_dpi(new_file.path(), 1, 72).unwrap();
    assert_eq!((rendered.width, rendered.height), (900, 900));
    let page = load_document_from_bytes(&new, &Limits::default())
        .unwrap()
        .pages
        .remove(0);
    assert_eq!(page.physical_size_points(), Some((900.0, 900.0)));
}

#[test]
fn unknown_units_preserve_fixed_width_projection_and_require_review() {
    let old = pdf("0 0 300 300", "0 0 300 300", 0, false, 0.0, 0);
    let mut new = old.clone();
    new.extend_from_slice(b"\n% changed representation\n");
    let ir = load_document_from_bytes(&old, &Limits::default()).unwrap();
    assert_eq!(ir.pages[0].user_unit, None);
    assert!(ir.pages[0].physical_size_points().is_none());
    let rendered = render_page_from_bytes(&old, 1, 400).unwrap();
    assert_eq!(rendered.viewport.user_unit, None);
    assert!(render_page_at_dpi_from_bytes(&old, 1, 72)
        .err()
        .unwrap()
        .to_string()
        .contains("Invalid UserUnit"));
    let diff = diff_bytes(
        "old",
        "new",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(!diff.is_comparison_complete());
    assert_eq!(diff.diagnostics.old_pages_with_unknown_units, 1);
    assert_eq!(diff.comparison.as_ref().unwrap().skipped_page_pairs, 1);
    assert!(diff.pages[0]
        .visual
        .as_ref()
        .unwrap()
        .reason
        .as_ref()
        .unwrap()
        .contains("UserUnit"));
    assert!(diff_bytes(
        "same",
        "same",
        &old,
        &old,
        &Limits::default(),
        &DiffOptions::default()
    )
    .unwrap()
    .is_comparison_complete());
    let bytes = pdf("0 0 300 300", "0 0 300 300", 0, false, 0.0, 75_000);
    let mut changed = bytes.clone();
    changed.extend_from_slice(b"\n% limit\n");
    let diff = diff_bytes(
        "old",
        "new",
        &bytes,
        &changed,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(!diff.is_comparison_complete());
    assert_eq!(diff.comparison.unwrap().pixel_budget_used, 0);
}
