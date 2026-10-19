use pdf_diff::{BlockIr, Bounds, DocumentIr, Limits, PageIr};

#[test]
fn rotated_page_furniture_uses_the_unrotated_text_coordinate_space() {
    for rotation in [0, 90, 180, 270] {
        let rotated = rotation == 90 || rotation == 270;
        let block = |index, text: &str, y0, y1| BlockIr {
            index,
            text: text.into(),
            bounds: Bounds {
                x0: 80.0,
                x1: 220.0,
                y0,
                y1,
            },
            size: None,
        };
        let page = PageIr {
            index: 1,
            width: if rotated { 800.0 } else { 600.0 },
            height: if rotated { 600.0 } else { 800.0 },
            rotation,
            blocks: vec![
                block(1, "契約書", 610.0, 640.0),
                block(2, "柱", 780.0, 790.0),
                block(3, "1", 8.0, 18.0),
            ],
        };
        assert_eq!(page.body_text(), "契約書", "rotation={rotation}");
    }
}
fn document() -> DocumentIr {
    DocumentIr {
        pages: vec![PageIr {
            index: 1,
            width: 600.0,
            height: 800.0,
            rotation: 0,
            blocks: vec![BlockIr {
                index: 1,
                text: "OCR 金額100円".into(),
                size: None,
                bounds: Bounds {
                    x0: 100.0,
                    y0: 500.0,
                    x1: 400.0,
                    y1: 520.0,
                },
            }],
        }],
    }
}
#[test]
fn ocr_ir_roundtrip_preserves_text_bounds_and_unknown_font_size() {
    let expected = document();
    let restored: DocumentIr =
        serde_json::from_value(serde_json::to_value(&expected).unwrap()).unwrap();
    restored.validate(&Limits::default()).unwrap();
    assert_eq!(restored.pages[0].blocks[0].text, "OCR 金額100円");
    assert_eq!(
        restored.pages[0].blocks[0].bounds,
        expected.pages[0].blocks[0].bounds
    );
    assert_eq!(restored.pages[0].blocks[0].size, None);
}
#[test]
fn ocr_ir_rejects_invalid_geometry_indices_and_resource_overflow() {
    let mut ir = document();
    ir.pages[0].width = f64::NAN;
    assert!(ir.validate(&Limits::default()).is_err());
    ir = document();
    ir.pages[0].blocks[0].bounds.x1 = 10.0;
    assert!(ir.validate(&Limits::default()).is_err());
    ir = document();
    ir.pages[0].rotation = 45;
    assert!(ir.validate(&Limits::default()).is_err());
    ir = document();
    ir.pages[0].index = 2;
    assert!(ir.validate(&Limits::default()).is_err());
    ir = document();
    ir.pages[0].blocks[0].index = 2;
    assert!(ir.validate(&Limits::default()).is_err());
    ir = document();
    assert!(ir
        .validate(&Limits {
            max_pages: 0,
            ..Limits::default()
        })
        .is_err());
    assert!(ir
        .validate(&Limits {
            max_blocks_per_page: 0,
            ..Limits::default()
        })
        .is_err());
    assert!(ir
        .validate(&Limits {
            max_input_mb: 0,
            ..Limits::default()
        })
        .is_err());
}
