//! 原本の完成グリッドを、セル内の表示行が多いだけの推測表へ置換しない。
use super::*;

/// 所有する矩形字形をAへ対応させ、独立readerで読めるchecksum付きfontを作る。
fn owned_font() -> ParsedEmbeddedFont {
    let mut bytes = layout_space::font("OwnedGridProof");
    let (cmap, loca, hhea, head, os2) = {
        let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
        let offset = |tag| {
            face.raw_face()
                .table_records
                .into_iter()
                .find(|record| record.tag == ttf_parser::Tag::from_bytes(tag))
                .unwrap()
                .offset as usize
        };
        (
            offset(b"cmap"),
            offset(b"loca"),
            offset(b"hhea"),
            offset(b"head"),
            offset(b"OS/2"),
        )
    };
    for (at, value) in [(26, 65u16), (32, 65), (36, (-64i16) as u16)] {
        bytes[cmap + at..cmap + at + 2].copy_from_slice(&value.to_be_bytes());
    }
    // 元の矩形をgid1へ移し、.notdefを空にする。字形命令自体は所有するものを使う。
    bytes[loca + 2..loca + 4].copy_from_slice(&0u16.to_be_bytes());
    bytes[hhea + 14..hhea + 16].copy_from_slice(&500u16.to_be_bytes());
    for at in [64, 66] {
        bytes[os2 + at..os2 + at + 2].copy_from_slice(&65u16.to_be_bytes());
    }
    bytes[head + 8..head + 12].fill(0);
    let count = usize::from(u16::from_be_bytes(bytes[4..6].try_into().unwrap()));
    for index in 0..count {
        let entry = 12 + 16 * index;
        let offset = u32::from_be_bytes(bytes[entry + 8..entry + 12].try_into().unwrap()) as usize;
        let length = u32::from_be_bytes(bytes[entry + 12..entry + 16].try_into().unwrap()) as usize;
        let sum = checksum(&bytes[offset..offset + length]);
        bytes[entry + 4..entry + 8].copy_from_slice(&sum.to_be_bytes());
    }
    let adjustment = 0xb1b0afbau32.wrapping_sub(checksum(&bytes));
    bytes[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    assert_eq!(checksum(&bytes), 0xb1b0afba);
    let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
    assert_eq!(face.glyph_index('A'), Some(ttf_parser::GlyphId(1)));
    assert!(face.glyph_bounding_box(ttf_parser::GlyphId(1)).is_some());
    assert_eq!(face.glyph_hor_advance(ttf_parser::GlyphId(1)), Some(1000));
    ParsedEmbeddedFont {
        font_name: "OwnedGridProof".into(),
        bytes,
    }
}

/// 所有fontの表と全体を、padding込みのSFNT checksumで検証する。
fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// 全四セルに二つの表示行がある、原本の2列2行の完成罫線表を作る。
fn source() -> ParsedDocument {
    let mut graphics = Vec::new();
    for x in [0., 80., 160.] {
        graphics.push(ParsedGraphicElement {
            page: 0,
            bounds: ParsedBounds {
                x0: x,
                x1: x,
                y0: 0.,
                y1: 80.,
            },
            stroke_width: 0.5,
        });
    }
    for y in [0., 40., 80.] {
        graphics.push(ParsedGraphicElement {
            page: 0,
            bounds: ParsedBounds {
                x0: 0.,
                x1: 160.,
                y0: y,
                y1: y,
            },
            stroke_width: 0.5,
        });
    }
    let mut texts = Vec::new();
    for baseline in [60., 45., 20., 5.] {
        for x in [10., 90.] {
            let bounds = ParsedBounds {
                x0: x,
                x1: x + 30.,
                y0: baseline,
                y1: baseline + 10.,
            };
            texts.push(ParsedTextElement {
                source_background: None,
                fill_stroke_width: None,
                font_weight: Some(400),
                page: 0,
                text: "AAA".into(),
                font_name: Some("OwnedGridProof".into()),
                font_size: 10.,
                fill_color: Some("123456".into()),
                baseline_y: Some(baseline),
                bounds,
            });
        }
    }
    ParsedDocument {
        path: "owned-source-grid-selection.pdf".into(),
        byte_len: 1,
        header: "%PDF-1.7".into(),
        version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
        page_count: 1,
        pages: vec![ParsedPage {
            page_number: 0,
            width: 200.,
            height: 120.,
            rotation: 0,
        }],
        graphics,
        source_diagonals: Vec::new(),
        text_elements: texts,
        image_elements: Vec::new(),
        embedded_fonts: vec![owned_font()],
        text_preview: String::new(),
        quality: Default::default(),
    }
}

/// 表示行を別セルと数えた文字表の利得だけでは、照合済みの原本表を捨てない。
#[test]
fn complete_verified_source_grid_is_not_replaced_by_more_text_rows() {
    let parsed = source();
    let graphic = infer_positioned_table_from_graphics(&parsed, 0, &parsed.graphics)
        .expect("complete source grid");
    assert_eq!(graphic.table.row_count(), 2);
    assert_eq!(graphic.table.col_count(), Some(2));
    assert!(graphic.docx_table.is_none(), "complete grid needs no spans");
    assert!(native_table_text::fractional_geometry_is_safe(
        &graphic.native_cells,
        &docx_table_model_from_ocr_table(&graphic.table)
    ));
    let text = infer_positioned_table_from_text_grid_page(&parsed, 0, false)
        .expect("four displayed text rows reproduce the misleading alternative");
    assert_eq!(text.table.row_count(), 4);
    let selected = infer_positioned_tables_for_page(&parsed, 0, vec![graphic], false, false);
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].table.row_count(),
        2,
        "verified source cells must survive a text-only row-count gain"
    );
    assert_eq!(selected[0].bounds.x0, 0.);
    assert_eq!(selected[0].bounds.x1, 160.);
}
