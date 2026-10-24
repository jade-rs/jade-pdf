//! Independent PDF operators, shared as inputs by native and browser tests.

pub const CASES: &[&str] = &[
    "agl_union",
    "agl_universal",
    "agl_uacute",
    "agl_ugrave",
    "agl_nonbmp",
    "agl_multicomponent",
    "agl_alias",
    "agl_surrogate",
    "agl_lowercase",
    "agl_mixed_unknown",
    "actual_space_separator",
    "marked_scope_page",
    "marked_scope_mcid",
    "marked_scope_nested_mcid",
    "marked_scope_form",
    "marked_scope_outer_form",
    "marked_scope_reused_form",
    "marked_scope_zero",
    "marked_scope_zero_single",
    "marked_scope_zero_rotated",
    "marked_scope_mixed_anchor",
    "marked_scope_no_object",
    "marked_scope_nested_actual",
    "actual_structure_page",
    "actual_structure_form",
    "actual_structure_parent_form",
    "actual_structure_vector",
    "actual_structure_multiple",
    "actual_structure_descendant",
    "actual_structure_unicode",
    "actual_structure_empty",
    "actual_structure_collision",
    "actual_structure_reused_form",
    "actual_structure_bad_parent",
    "actual_structure_bad_backpointer",
    "actual_structure_crosspage",
    "actual_structure_nested_actual",
    "source_unicode_masked",
    "source_unicode_masked_range",
    "source_unicode_actual",
    "source_unicode_actual_named",
    "source_unicode_actual_empty",
    "source_unicode_actual_blank",
    "source_unicode_actual_large",
    "source_unicode_actual_invalid",
    "source_duplicate",
    "source_duplicate_visible",
    "source_duplicate_nested",
    "source_duplicate_quote",
    "source_trailing_invalid",
    "oblique_words",
    "oblique_letters",
    "oblique_shear",
    "oblique_edit",
    "oblique_parallel",
    "oblique_nested",
    "angled_label_rotation",
    "angled_label_units",
    "angled_label_edit",
    "angled_label_nested_rotation",
    "unobserved_space",
    "unobserved_mixed",
    "unobserved_nested",
    "unobserved_nested_mixed",
    "unobserved_units",
    "unobserved_whitespace",
    "form_orphan_merged",
    "form_orphan_parent",
    "form_orphan_partial",
    "form_orphan_cycle",
    "surrogate_unpaired",
    "supplementary_unicode",
    "supplementary_unicode_edit",
    "form_catalog_invalid",
    "form_catalog_null",
    "form_null_entries",
    "form_repeated_widget",
    "empty_form_shell",
    "empty_form_shell_edit",
    "empty_form_orphan_widget",
    "empty_form_xfa",
    "empty_form_action",
    "empty_form_unknown",
    "tagged_table_stale_units",
    "tagged_table_stale_edit",
    "tagged_table_stale_rtl",
    "tagged_table_stale_ambiguous",
    "tagged_table_spans_units",
    "tagged_table_spans_edit",
    "tagged_table_spans_rtl",
    "crop_table_identity",
    "tagged_table_shear_units",
    "tagged_table_shear_edit",
    "tagged_table_diagonal",
    "tagged_table_units",
    "tagged_table_edit",
    "construction_units",
    "construction_shared_units",
    "layout_units",
    "layout_units_edit",
    "space_units",
    "widget_units",
    "widget_units_edit",
    "user_unit_scale",
    "user_unit_invalid",
    "user_unit_limit",
    "partial_scan",
    "image_only",
    "image_reencoded",
    "path",
    "text_color",
    "text_face",
    "line_hyphen",
    "mapping_error_printable",
    "mapping_error_control",
    "mapping_error_mixed",
    "tounicode_overrides",
    "tounicode_override_edit",
    "form_unplaced",
    "form_deep_property",
    "form_multi_export",
    "form_hybrid_xfa",
    "form_partial_property",
    "form_stream_reencoded",
    "form_corrupt_stream",
    "form_value",
    "form_flags",
    "oversized_widget_name",
    "oversized_widget_value",
    "form_hidden",
    "form_unicode",
    "checkbox",
    "checkbox_export",
    "checkbox_unchecked_export",
    "oversized_image",
    "nested_oversized_image",
    "list",
    "combo_custom",
    "combo_no_options",
    "choice_export_label",
    "radio_reordered",
    "budget",
    "oversized",
];

fn stream(data: &str, attributes: &str) -> String {
    format!(
        "<< /Length {} {attributes} >>\nstream\n{data}\nendstream",
        data.len()
    )
}

fn document(objects: Vec<String>) -> Vec<u8> {
    let mut source = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let start = source.len();
    source.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for o in offsets {
        source.push_str(&format!("{o:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
        objects.len() + 1
    ));
    source.into_bytes()
}

fn utf16(s: &str) -> String {
    format!(
        "<FEFF{}>",
        s.encode_utf16()
            .map(|c| format!("{c:04X}"))
            .collect::<String>()
    )
}

/// Two separately painted Symbol construction pieces, each explicitly mapped
/// to a vertical bar. Their small raw separation used to trigger SDK duplicate
/// suppression after an inverse UserUnit rebase.
pub fn construction_pieces(unit: f64, rotation: u16, nested: bool) -> Vec<u8> {
    let cmap = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /IndependentGold def /CMapType 2 def\n1 begincodespacerange <00> <ff> endcodespacerange\n1 beginbfchar <e7> <007c> endbfchar\nendcmap CMapName currentdict /CMap defineresource pop end end";
    let ink = "BT /F1 12 Tf 100 130 Td <e7> Tj ET\nBT /F1 12 Tf 100 134.3 Td <e7> Tj ET";
    let content = if nested { "/Outer Do" } else { ink };
    let page = |unit: f64, content: usize| {
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 5 0 R >> /XObject << /Outer 8 0 R >> >> /Contents {content} 0 R >>", 300.0 / unit)
    };
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".into(),
        page(unit, 7),
        page(1.0, 10),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Symbol /ToUnicode 6 0 R >>".into(),
        stream(cmap, ""),
        stream(&format!("q {0} 0 0 {0} 0 0 cm {content} Q", 1.0 / unit), ""),
        stream("/Inner Do", "/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /XObject << /Inner 9 0 R >> >>"),
        stream(ink, "/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> >>"),
        stream(content, ""),
    ])
}

/// Paint the cells column-first but declare row-first Table/TR/TH/TD order.
/// Malformed/partial modes have matching untagged controls for exact fallback.
pub fn tagged_table_layout(
    unit: f64,
    rotation: u16,
    mode: &str,
    tagged: bool,
    edit: bool,
) -> Vec<u8> {
    let mut content = format!("q {0} 0 0 {0} 0 0 cm BT /F1 12 Tf 30 270 Td (A heading spanning both table columns) Tj ET\n", 1.0/unit);
    for (id, mut x, mut y, text) in [
        (0, 40, 220, "Item"),
        (2, 40, 196, "Apples"),
        (1, 190, 220, "Price"),
        (3, 190, 196, if edit { "11" } else { "10" }),
    ] {
        if mode == "overlapping-rows" && id >= 2 {
            y = 220;
        }
        if mode == "contradictory-columns" && id >= 2 {
            x = 230 - x;
        }
        if id == 3 && mode == "form-collision" {
            content.push_str("/Fm Do\n");
            continue;
        }
        let marked = !(id == 3 && mode == "missing-cell");
        if marked {
            content.push_str(&format!("/P << /MCID {id} >> BDC "));
        }
        if matches!(
            mode,
            "shear-positive" | "shear-negative" | "baseline-diagonal"
        ) {
            let (b, c) = match mode {
                "shear-positive" => (0.0, 1.0 / 3.0),
                "shear-negative" => (0.0, -1.0 / 3.0),
                _ => (1.0 / 3.0, 0.0),
            };
            content.push_str(&format!(
                "BT /F1 12 Tf 1 {b} {c} 1 {x} {y} Tm ({text}) Tj ET\n"
            ));
        } else {
            content.push_str(&format!("BT /F1 12 Tf {x} {y} Td ({text}) Tj ET\n"));
        }
        if marked {
            content.push_str("EMC\n");
        }
    }
    // Real producers can mark row-level whitespace outside individual cells.
    let separator = if mode == "visible-row-text" {
        "Note"
    } else {
        " "
    };
    content.push_str(&format!(
        "/P << /MCID 4 >> BDC BT /F1 12 Tf 250 196 Td ({separator}) Tj ET EMC\n"
    ));
    content.push_str("BT /F1 12 Tf 30 130 Td (A footer spanning both table columns) Tj ET Q");
    let cell = |id, parent, kind, mcid| {
        format!("<< /Type /StructElem /S /{kind} /P {parent} 0 R /Pg 3 0 R /K << /Type /MCR /Pg 3 0 R /MCID {mcid} >> /ID (cell{id}) >>")
    };
    let last_id = if mode == "duplicate-id" { 2 } else { 3 };
    let second_row_first = if mode == "shared-cell" { 9 } else { 11 };
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {} >>", if tagged {"/StructTreeRoot 5 0 R /MarkInfo << /Marked true >>"} else {""}),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /StructParents 0 /Resources << /Font << /F1 4 0 R >> /XObject << /Fm 14 0 R >> >> /Contents 13 0 R >>",300.0/unit),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /StructTreeRoot /K 6 0 R /ParentTree 15 0 R /ParentTreeNextKey 1 >>".into(),
        "<< /Type /StructElem /S /Table /P 5 0 R /K [7 0 R 8 0 R] >>".into(),
        "<< /Type /StructElem /S /TR /P 6 0 R /K [9 0 R 10 0 R] >>".into(),
        format!("<< /Type /StructElem /S /TR /P 6 0 R /K [{second_row_first} 0 R 12 0 R << /Type /MCR /Pg 3 0 R /MCID 4 >>] >>"),
        cell(0,7,"TH",0),cell(1,7,"TH",1),cell(2,8,"TD",2),cell(3,8,"TD",last_id),
        stream(&content,""),
        stream("/P << /MCID 3 >> BDC BT /F1 12 Tf 190 196 Td (10) Tj ET EMC", "/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >>"),
        "<< /Nums [0 [9 0 R 10 0 R 11 0 R 12 0 R 8 0 R]] >>".into(),
    ];
    if mode == "shuffled-rows" {
        objects[5] = objects[5].replace("[7 0 R 8 0 R]", "[8 0 R 7 0 R]");
    }
    if mode == "deep-tree" {
        let depth = 70;
        objects[4] = objects[4].replace("/K 6 0 R", "/K 16 0 R");
        objects[5] = objects[5].replace("/P 5 0 R", &format!("/P {} 0 R", 15 + depth));
        for level in 0..depth {
            let id = 16 + level;
            let parent = if level == 0 { 5 } else { id - 1 };
            let child = if level + 1 == depth { 6 } else { id + 1 };
            objects.push(format!(
                "<< /Type /StructElem /S /Div /P {parent} 0 R /K {child} 0 R >>"
            ));
        }
    }
    if mode.starts_with("crop-") {
        let top = if mode == "crop-old" { 200 } else { 230 };
        objects[2] = objects[2].replace(
            "/StructParents",
            &format!("/CropBox [0 145 300 {top}] /StructParents"),
        );
    }
    document(objects)
}

/// Row/column-spanning headers cross vertical row bands; cells in a row have
/// different logical column indices. Paint columns first, independently of tags.
pub fn spanning_tagged_table_layout(
    unit: f64,
    rotation: u16,
    mode: &str,
    rtl: bool,
    tagged: bool,
    edit: bool,
) -> Vec<u8> {
    let values = [
        (40, 220, "Name"),
        (175, 232, "Totals"),
        (145, 207, "Qty"),
        (225, 207, "Price"),
        (40, 180, "Apple"),
        (145, 180, "5"),
        (225, 180, if edit { "11" } else { "10" }),
        (40, 156, "Pear"),
        (145, 156, "2"),
        (225, 156, "6"),
    ];
    let mut content = format!(
        "q {0} 0 0 {0} 0 0 cm BT /F1 12 Tf 30 270 Td (Heading across the table) Tj ET\n",
        1.0 / unit
    );
    let stale = mode.starts_with("stale-");
    if mode == "stale-whitespace" {
        let x = if rtl { 220 } else { 40 };
        content.push_str(&format!(
            "/P << /MCID 0 >> BDC BT /F1 12 Tf {x} 220 Td ( ) Tj ET EMC\n"
        ));
    }
    for id in [0, 4, 7, 1, 2, 5, 8, 3, 6, 9] {
        let (mut x, mut y, text) = values[id];
        if matches!(mode, "crossing-rows" | "stale-crossed-row") && id == 5 {
            y = 239;
        }
        if mode == "overlapping-cells" && id == 5 {
            x = if rtl { 35 } else { 50 };
        }
        if mode == "contradictory-direction" && id >= 4 {
            x = 260 - x;
        }
        if rtl {
            x = 260 - x;
        }
        let marked = !(mode == "missing-cell" && id == 6 || mode == "stale-unmarked" && id == 0);
        let paint_id = if stale && id == 0 { 10 } else { id };
        if marked {
            content.push_str(&format!("/P << /MCID {paint_id} >> BDC "));
        }
        if matches!(
            mode,
            "shear-positive" | "shear-negative" | "baseline-diagonal"
        ) {
            let (b, c) = match mode {
                "shear-positive" => (0.0, 1.0 / 3.0),
                "shear-negative" => (0.0, -1.0 / 3.0),
                _ => (1.0 / 3.0, 0.0),
            };
            content.push_str(&format!(
                "BT /F1 12 Tf 1 {b} {c} 1 {x} {y} Tm ({text}) Tj ET\n"
            ));
        } else {
            content.push_str(&format!("BT /F1 12 Tf {x} {y} Td ({text}) Tj ET\n"));
        }
        if id == 0 {
            if mode == "stale-two-orphans" {
                content.push_str("EMC /P << /MCID 11 >> BDC ");
            }
            content.push_str(&format!("BT /F1 12 Tf {x} 207 Td (category) Tj ET\n"));
        }
        if marked {
            content.push_str("EMC\n");
        }
    }
    if mode == "stale-other-content" {
        content
            .push_str("/P << /MCID 10 >> BDC BT /F1 12 Tf 40 250 Td (Other content) Tj ET EMC\n");
    }
    if mode == "foreign-text" {
        content.push_str("BT /F1 12 Tf 100 180 Td (X) Tj ET\n");
    }
    content.push_str("BT /F1 12 Tf 30 110 Td (Footer across the table) Tj ET Q");
    let row_order = if mode.contains("shuffled") {
        "10 0 R 7 0 R 9 0 R 8 0 R"
    } else {
        "7 0 R 8 0 R 9 0 R 10 0 R"
    };
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {} >>",if tagged {"/StructTreeRoot 5 0 R /MarkInfo << /Marked true >>"} else {""}),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /StructParents 0 /Resources << /Font << /F1 4 0 R >> >> /Contents 21 0 R >>",300.0/unit),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /StructTreeRoot /K 6 0 R /ParentTree 22 0 R >>".into(),
        format!("<< /Type /StructElem /S /Table /P 5 0 R /K [{row_order}] >>"),
        "<< /Type /StructElem /S /TR /P 6 0 R /K [11 0 R 12 0 R] >>".into(),
        "<< /Type /StructElem /S /TR /P 6 0 R /K [13 0 R 14 0 R] >>".into(),
        "<< /Type /StructElem /S /TR /P 6 0 R /K [15 0 R 16 0 R 17 0 R] >>".into(),
        "<< /Type /StructElem /S /TR /P 6 0 R /K [18 0 R 19 0 R 20 0 R] >>".into(),
    ];
    for id in 0..10 {
        let parent = match id {
            0 | 1 => 7,
            2 | 3 => 8,
            4..=6 => 9,
            _ => 10,
        };
        let role = if id < 4 { "TH" } else { "TD" };
        let span = match id {
            0 => "/A << /O /Table /RowSpan 2 >>",
            1 => "/A << /O /Table /ColSpan 2 >>",
            _ => "",
        };
        objects.push(format!(
            "<< /Type /StructElem /S /{role} /P {parent} 0 R /Pg 3 0 R {span} /K {id} >>"
        ));
    }
    objects.push(stream(&content, ""));
    objects.push(
        "<< /Nums [0 [11 0 R 12 0 R 13 0 R 14 0 R 15 0 R 16 0 R 17 0 R 18 0 R 19 0 R 20 0 R]] >>"
            .into(),
    );
    if stale {
        objects[4] = objects[4].replace(
            "/K 6 0 R",
            if mode == "stale-two-orphans" {
                "/K [6 0 R 23 0 R 24 0 R]"
            } else {
                "/K [6 0 R 23 0 R]"
            },
        );
        objects[21] = objects[21].replace(
            "20 0 R]",
            if mode == "stale-two-orphans" {
                "20 0 R 23 0 R 24 0 R]"
            } else {
                "20 0 R 23 0 R]"
            },
        );
        objects.push("<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 10 >>".into());
        if mode == "stale-two-orphans" {
            objects.push("<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K 11 >>".into());
        }
    }
    if mode == "stale-one-reference" {
        objects[17] = objects[17].replace("/K 7", "/A << /O /Table /RowSpan 2 >> /K 7");
    }
    if mode == "stale-bad-span" {
        objects[11] = objects[11].replace("/ColSpan 2", "/ColSpan 3");
    }
    if mode == "stale-invalid-span" {
        let invalid = match rotation {
            0 => "1000000",
            90 => "0",
            180 => "1.5",
            _ => "(2)",
        };
        objects[11] = objects[11].replace("/ColSpan 2", &format!("/ColSpan {invalid}"));
    }
    if mode == "stale-wrong-owner" {
        objects[10] = objects[10].replace("/O /Table", "/O /Layout");
    }
    if mode == "stale-conflicting-span" {
        objects[10] = objects[10].replace(
            "/A << /O /Table /RowSpan 2 >>",
            "/A [<< /O /Table /RowSpan 2 >> << /O /Table /RowSpan 3 >>]",
        );
    }
    document(objects)
}

/// Independent empty-shell and adversarial form structures. Objects outside
/// Fields remain in the PDF so the no-field proof must inspect all objects.
pub fn empty_form_shell(mode: &str, edit: bool, indirect: bool) -> Vec<u8> {
    let fields = match mode {
        "missing" => "",
        "null" => "/Fields null",
        "number" => "/Fields 42",
        "dangling" => "/Fields 99 0 R",
        "field-tree" => "/Fields [8 0 R]",
        _ if indirect => "/Fields 7 0 R",
        _ => "/Fields []",
    };
    let extras = match mode {
        "defaults" => "/DA (/F1 12 Tf 0 g) /DR << /Font << /F1 4 0 R >> >>",
        "indirect-defaults" => "/DA 9 0 R /DR 10 0 R /NeedAppearances false",
        "need-appearances" => "/NeedAppearances true",
        "unknown" => "/Unverified true",
        "xfa" => "/XFA []",
        "xfa-null" => "/XFA null",
        "calculation-order" => "/CO []",
        "signature-flags" => "/SigFlags 0",
        "form-action" => "/AA << /K 9 0 R >>",
        _ => "",
    };
    let catalog_extra = match mode {
        "action" => "/OpenAction 9 0 R",
        "names" => "/Names << /JavaScript << /Names [(script) 9 0 R] >> >>",
        "permissions" => "/Perms << >>",
        "structure-title" | "structure-field" => "/StructTreeRoot 12 0 R",
        _ => "",
    };
    let annotation = match mode {
        "orphan-widget" | "annotation" => "/Annots [8 0 R]",
        "malformed-annots" => "/Annots 42",
        "empty-annots" => "/Annots 11 0 R",
        _ => "",
    };
    let potential = match mode {
        "orphan-widget" => "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Orphan) /V (value) /Rect [30 150 160 175] /P 3 0 R >>",
        "annotation" => "<< /Type /Annot /Subtype /Text /Rect [30 150 50 175] /Contents (note) >>",
        "field-tree" | "unplaced" => "<< /FT /Tx /T (Unplaced) /V (value) >>",
        "bare-field" => "<< /T (Incomplete) >>",
        "nested-field" => "<< /Container [<< /FT /Tx /T (Nested) /V (value) >>] >>",
        "stream-field" => "<< /Length 0 /FT /Tx /T (StreamField) >>\nstream\n\nendstream",
        "signature" => "<< /Type /Sig /ByteRange [0 1 2 3] >>",
        "linearization" => "<< /Linearized 1 /T 123 /L 300 /H [0 1] /O 3 /E 100 /N 1 >>",
        "hint-stream" => "<< /Length 0 /I 137 /S 38 >>\nstream\n\nendstream",
        _ => "null",
    }.to_owned();
    let defaults = mode == "indirect-defaults";
    let action = "<< /S /JavaScript /JS (var value = 1;) >>";
    let content = format!(
        "BT /F1 12 Tf 30 100 Td ({}) Tj ET",
        if edit { "Edited page" } else { "Stable page" }
    );
    document(vec![
        format!("<< /Type /Catalog /Pages 2 0 R {} {catalog_extra} >>", if mode == "no-form" { "" } else { "/AcroForm 6 0 R" }),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R {annotation} >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        stream(&content, ""),
        format!("<< {fields} {extras} >>"),
        if mode == "cycle" { "7 0 R" } else { "[]" }.into(),
        potential,
        if defaults { "(/F1 12 Tf 0 g)" } else if matches!(mode, "action" | "form-action" | "names") { action } else { "null" }.into(),
        if defaults { "<< /Font << /F1 4 0 R >> >>" } else { "null" }.into(),
        "[]".into(),
        if matches!(mode,"structure-title" | "structure-field") { "<< /Type /StructTreeRoot /K 13 0 R >>" } else { "null" }.into(),
        if mode == "structure-field" { "<< /S /P /P 12 0 R /K 0 /T (Title) /FT /Tx >>" }
        else if mode == "structure-title" { "<< /S /P /P 12 0 R /K 0 /T (Title) >>" } else { "null" }.into(),
    ])
}

/// A non-null wrong-type catalog entry is deliberately invisible to SDK
/// form-type detection. Null/dangling references are the absence controls.
pub fn catalog_form_entry(mode: &str) -> Vec<u8> {
    let entry = match mode {
        "absent" => "",
        "null" => "/AcroForm null",
        "null-ref" => "/AcroForm 7 0 R",
        "dangling" => "/AcroForm 99 0 R",
        "cycle" => "/AcroForm 8 0 R",
        "number" => "/AcroForm 42",
        "name" => "/AcroForm /Unknown",
        "string" => "/AcroForm (Unknown)",
        "array" => "/AcroForm [6 0 R]",
        "stream" => "/AcroForm 9 0 R",
        _ => "/AcroForm << /Fields [] >>",
    };
    document(vec![
        format!("<< /Type /Catalog /Pages 2 0 R {entry} >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        stream("BT /F1 12 Tf 30 100 Td (Stable page) Tj ET", ""),
        "<< /FT /Tx /T (Unused) /V (unused) >>".into(),
        "null".into(),
        "8 0 R".into(),
        stream("", "/Fields []"),
    ])
}

/// Two logical roots, one unnamed control. No widget is placed on a page:
/// changes to Other must be found from Fields even when pixels are identical.
pub fn repeated_field_control(mode: &str, revised: bool) -> Vec<u8> {
    let text = mode == "text-equivalent";
    let kind = if text { "Tx" } else { "Btn" };
    let parent_value = if text { "(Alpha)" } else { "/One" };
    let flags = if text { 0 } else { 32768 };
    let child = match mode {
        "indirect" => "/FT 9 0 R",
        "flags" => "/FT /Btn /Ff 32768",
        "default-flags" => "/FT /Btn /Ff 0 /Q 0",
        "value" => "/FT /Btn /V /One",
        "text-equivalent" => "/FT /Tx /V <FEFF0041006C007000680061>",
        "null" => "/FT /Btn /Ff null /V null /AA null",
        "conflict-type" => "/FT /Ch",
        "conflict-flags" => "/FT /Btn /Ff 0",
        "conflict-value" => "/FT /Btn /V /Two",
        "conflict-options" => "/FT /Btn /Opt [(one)]",
        "conflict-action" => "/FT /Btn /AA << /K << /S /JavaScript /JS (var value=1;) >> >>",
        _ => "/FT /Btn",
    };
    let parent_flags = if mode == "default-flags" {
        String::new()
    } else {
        format!("/Ff {flags}")
    };
    let parent_name = if mode == "root-unnamed" {
        ""
    } else {
        "/T (Parent)"
    };
    let child_name = if mode == "child-named" {
        "/T (Child)"
    } else {
        ""
    };
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R 8 0 R] >> >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        stream("BT /F1 12 Tf 30 100 Td (Stable page) Tj ET", ""),
        format!("<< /FT /{kind} {parent_name} {parent_flags} /V {parent_value} /Kids [7 0 R] >>"),
        format!("<< /Type /Annot /Subtype /Widget /Parent 6 0 R /P 3 0 R /Rect [30 150 150 175] {child} {child_name} >>"),
        format!("<< /FT /Tx /T (Other) /V ({}) >>", if revised { "changed" } else { "original" }),
        format!("/{kind}"),
    ])
}

/// Page-side declarations independent of the catalog inventory. All widgets
/// are hidden so a saved-value edit cannot be explained by changed pixels.
pub fn orphan_widgets(mode: &str, revised: bool) -> Vec<u8> {
    let value = if revised { "changed" } else { "original" };
    let form = match mode {
        "no-form" | "parent" | "missing-backlink" | "cycle" | "duplicate" | "alias"
        | "unused-page" => "",
        "null-form" => "/AcroForm null",
        "missing-fields" => "/AcroForm << /DA (/Helv 12 Tf 0 g) >>",
        "partial" => "/AcroForm << /Fields [6 0 R] >>",
        _ => "/AcroForm << /Fields [] >>",
    };
    let parent = matches!(mode, "parent" | "missing-backlink" | "cycle");
    let annotations = if mode == "duplicate" {
        "[7 0 R 8 0 R]"
    } else if mode == "alias" {
        "[7 0 R 7 0 R]"
    } else if mode == "unused-page" {
        "[]"
    } else {
        "[7 0 R]"
    };
    let branch = if parent {
        format!(
            "<< /FT /Tx /T (Parent) /V ({value}) {} >>",
            if mode == "missing-backlink" {
                ""
            } else {
                "/Kids [7 0 R]"
            }
        )
    } else {
        format!("<< /FT /Tx /T (Declared) /V ({value}) >>")
    };
    let widget = if parent {
        format!(
            "<< /Type /Annot /Subtype /Widget /Parent {} 0 R /Rect [30 150 150 175] /F 2 >>",
            if mode == "cycle" { 7 } else { 6 }
        )
    } else {
        let name = if mode == "partial" {
            if revised {
                "OtherNew"
            } else {
                "OtherOld"
            }
        } else {
            "Orphan"
        };
        format!("<< /Type /Annot /Subtype /Widget /FT /Tx /T ({name}) /V ({value}) /Rect [30 150 150 175] /F 2 >>")
    };
    document(vec![
        format!("<< /Type /Catalog /Pages 2 0 R {form} >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R /Annots {annotations} >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        stream("BT /F1 12 Tf 30 100 Td (Stable page) Tj ET", ""),
        branch,
        widget.clone(),
        widget,
        "<< /Type /Page /MediaBox [0 0 300 300] /Annots [7 0 R] >>".into(),
    ])
}

pub fn supplementary_unicode(unit: f64, rotation: u16, revised: bool) -> Vec<u8> {
    supplementary_unicode_mode(unit, rotation, if revised { "edited" } else { "valid" })
}

pub fn supplementary_unicode_mode(unit: f64, rotation: u16, mode: &str) -> Vec<u8> {
    let (first, second) = match mode {
        "edited" => ("D83DDE01", "D835DC50D835DC50"),
        "split" | "coincident" => ("D83D", "DE00"),
        "lone-high" => ("D83D", "0042"),
        "lone-low" => ("DE00", "0042"),
        "bad-order" => ("DE00", "D83D"),
        _ => ("D83DDE00", "D835DC50D835DC50"),
    };
    let cmap=format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Unicode def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange 4 beginbfchar <20> <D83DDE00> <41> <{first}> <42> <{second}> <43> <0043> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end");
    let content = format!(
        "q {0} 0 0 {0} 0 0 cm BT /F1 12 Tf 30 200 Td (ABC) Tj ET Q",
        1.0 / unit
    );
    let content = if mode == "invisible" {
        format!(
            "q {0} 0 0 {0} 0 0 cm BT /F1 12 Tf 3 Tr 30 200 Td (A) Tj ET Q",
            1.0 / unit
        )
    } else if mode == "space" {
        format!(
            "q {0} 0 0 {0} 0 0 cm BT /F1 12 Tf 30 200 Td ( ) Tj ET Q",
            1.0 / unit
        )
    } else if mode == "coincident" {
        format!("q {0} 0 0 {0} 0 0 cm BT /F1 12 Tf 30 200 Td (A) Tj ET BT /F1 12 Tf 30 200 Td (B) Tj ET BT /F1 12 Tf 80 200 Td (C) Tj ET Q",1.0/unit)
    } else {
        content
    };
    let objects=vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",300.0/unit),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>".into(),
        stream(&content,""),stream(&cmap,""),
    ];
    document(objects)
}

/// A literal space glyph has an empty font box; its declared Unicode can still
/// be meaningful. SDK character enumeration omits the entire text object.
/// Independent encoded-glyph gold: one C versus two coincident Cs. Pdfium
/// suppresses the second record inside an otherwise observed text object.
/// Repeated BDC instances share one property dictionary, while each instance
/// can own multiple painted objects. Source strings deliberately differ from ink.
pub fn marked_actual_scopes(unit: f64, rotation: u16, mode: &str, revised: bool) -> Vec<u8> {
    let replacement = if revised { "B" } else { "A" };
    let property = utf16(replacement);
    let zero = mode.starts_with("zero");
    let show = |x: f64, glyph: &str| format!("BT /F1 12 Tf 3 Tr {x} 150 Td ({glyph}) Tj ET");
    let part = |x: f64| {
        let anchored = mode == "mixed_anchor" && x == 30.;
        let body = if mode == "no_object" {
            String::new()
        } else if mode == "zero_single" || anchored {
            show(x, " ")
        } else {
            let first = show(x, if zero { " " } else { "C" });
            let second = show(x + 15., if zero { " " } else { "D" });
            if mode == "nested_mcid" {
                format!("{first} /Span << /MCID 1 >> BDC {second} EMC")
            } else if mode == "nested_actual" {
                format!("{first} /Span << /ActualText (X) >> BDC {second} EMC")
            } else {
                format!("{first} {second}")
            }
        };
        format!("/Span /Same BDC {body} EMC")
    };
    let scopes = format!(
        "{} {}",
        part(30.),
        part(if mode == "mixed_anchor" { 45. } else { 110. })
    );
    let inside_form = matches!(mode, "form" | "reused_form" | "zero_rotated");
    let body = if mode == "zero_rotated" {
        "q 0.70710678 0.70710678 -0.70710678 0.70710678 180 20 cm /X Do Q".into()
    } else if inside_form {
        format!(
            "q 2 0 0 1 40 25 cm /X Do Q {}",
            if mode == "reused_form" {
                "q 1 0 0 1 0 80 cm /X Do Q"
            } else {
                ""
            }
        )
    } else if mode == "outer_form" {
        "/Span /Same BDC q 2 0 0 1 40 25 cm /X Do Q EMC /Span /Same BDC q 1 0 0 1 160 80 cm /X Do Q EMC".into()
    } else {
        scopes.clone()
    };
    let body = if mode == "mcid" || mode == "nested_mcid" {
        format!("/P << /MCID 0 >> BDC {body} EMC")
    } else {
        body
    };
    let form = if mode == "outer_form" {
        format!("{} {}", show(30., "C"), show(45., "D"))
    } else {
        scopes
    };
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> /XObject << /X 6 0 R >> /Properties << /Same << /ActualText {property} >> >> >> /Contents 5 0 R >>",400./unit),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        stream(&format!("q {0} 0 0 {0} 0 0 cm {body} Q",1./unit),""),
        stream(&form,&format!("/Type /XObject /Subtype /Form /BBox [0 0 400 400] /Resources << /Font << /F1 4 0 R >> /Properties << /Same << /ActualText {property} >> >> >>")),
    ])
}

/// ActualText in a structure element owns independently indexed marked content.
/// Logical strings deliberately differ from the painted glyphs.
pub fn structure_actual(unit: f64, rotation: u16, mode: &str, revised: bool) -> Vec<u8> {
    let replacement = if revised {
        match mode {
            "empty" => utf16(""),
            "space" => utf16(" "),
            "unicode" => utf16("日本語🦊"),
            _ => utf16("B"),
        }
    } else {
        utf16("A")
    };
    let paint = "BT /F1 12 Tf 3 Tr 30 150 Td (C) Tj ET";
    let marked = format!("/Span << /MCID 0 >> BDC {paint} EMC");
    let in_form = matches!(mode, "form" | "reused_form");
    let body = match mode {
        "form" => "/X Do".into(),
        "reused_form" => "/X Do q 1 0 0 1 20 0 cm /X Do Q".into(),
        "parent_form" => "/Span << /MCID 0 >> BDC /X Do EMC".into(),
        "collision" => format!("{marked} /X Do"),
        "vector" => "/Span << /MCID 0 >> BDC 30 130 30 20 re f EMC".into(),
        "multiple" => {
            format!("{marked} /Span << /MCID 1 >> BDC BT /F1 12 Tf 3 Tr 50 150 Td (D) Tj ET EMC")
        }
        "nested_actual" => {
            format!("/Span << /MCID 0 >> BDC /Span << /ActualText (X) >> BDC {paint} EMC EMC")
        }
        _ => marked.clone(),
    };
    let kid = if in_form {
        "<< /Type /MCR /Pg 3 0 R /Stm 9 0 R /MCID 0 >>"
    } else if mode == "multiple" {
        "[0 1]"
    } else if mode == "crosspage" {
        "[0 << /Type /MCR /Pg 11 0 R /MCID 0 >>]"
    } else if mode == "descendant" {
        "13 0 R"
    } else {
        "0"
    };
    let parents = if mode == "bad_backpointer" {
        "1 0 R"
    } else if mode == "descendant" {
        "13 0 R"
    } else if mode == "multiple" {
        "7 0 R 7 0 R"
    } else {
        "7 0 R"
    };
    let form_body = if mode == "parent_form" {
        paint
    } else {
        &marked
    };
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 6 0 R >>".into(),
        format!("<< /Type /Pages /Kids [3 0 R {}] /Count {} >>", if mode == "crosspage" {"11 0 R"} else {""},if mode == "crosspage" {2} else {1}),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /StructParents 0 /Resources << /Font << /F1 4 0 R >> /XObject << /X 9 0 R >> >> /Contents 5 0 R >>",300.0/unit),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        stream(&format!("q {0} 0 0 {0} 0 0 cm {body} Q",1.0/unit),""),
        format!("<< /Type /StructTreeRoot /K [7 0 R] {} >>",if mode == "missing_parent_tree" {""} else {"/ParentTree 8 0 R"}),
        format!("<< /Type /StructElem /S /Span /P {} 0 R /Pg 3 0 R /ActualText {replacement} /K {kid} >>",if mode == "bad_parent" {1} else {6}),
        format!("<< /Nums [0 [{parents}] 1 [7 0 R] 2 [7 0 R]] >>"),
        stream(form_body,"/Type /XObject /Subtype /Form /BBox [0 0 300 300] /StructParents 1 /Resources << /Font << /F1 4 0 R >> >>"),
        "null".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /StructParents 2 /Resources << /Font << /F1 4 0 R >> >> /Contents 12 0 R >>".into(),
        stream(&marked,""),
        "<< /Type /StructElem /S /Span /P 7 0 R /Pg 3 0 R /K 0 >>".into(),
    ])
}

pub fn source_unicode(
    unit: f64,
    rotation: u16,
    nesting: usize,
    mode: &str,
    revised: bool,
) -> Vec<u8> {
    let actual = mode.starts_with("actual");
    let mapping = if actual {
        "1 beginbfchar <41> <0042> endbfchar"
    } else if mode == "range" {
        "1 beginbfrange <41> <41> [<00430043>] endbfrange"
    } else {
        "1 beginbfchar <41> <00430043> endbfchar"
    };
    let cmap=format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /SourceGold def /CMapType 2 def 1 begincodespacerange <00> <ff> endcodespacerange {mapping} 1 beginbfchar <43> <0043> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end");
    let large = utf16(&"D".repeat(3000));
    let replacement = if revised {
        if mode == "actual_large" {
            large.as_str()
        } else if mode == "actual_blank" {
            "<FEFF00090020000A>"
        } else if mode == "actual_empty" {
            "<>"
        } else if mode == "actual_invalid" {
            "<FEFF0043FF>"
        } else {
            "<FEFF0044>"
        }
    } else {
        "<FEFF0043>"
    };
    let props = format!("/Properties << /Replacement << /ActualText {replacement} >> >>");
    let text = if actual {
        let property = if mode == "actual_named" {
            "/Replacement".into()
        } else {
            format!("<< /ActualText {replacement} >>")
        };
        if mode == "actual_graphics" {
            format!("/Span {property} BDC 0 0 m 0 0 l S EMC")
        } else {
            format!("/Span {property} BDC BT /F1 12 Tf 3 Tr 30 150 Td (A) Tj ET EMC")
        }
    } else {
        format!(
            "BT /F1 12 Tf 3 Tr 30 200 Td (A) Tj ET BT /F1 12 Tf 3 Tr 30 150 Td {} ET",
            if revised {
                "[(C) 722 (C)] TJ"
            } else {
                "(C) Tj"
            }
        )
    };
    let body = if nesting == 0 {
        text.as_str()
    } else {
        "/Next Do"
    };
    let mut objects=vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> /XObject << /Next 7 0 R >> {props} >> /Contents 5 0 R >>",300.0/unit),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>".into(),
        stream(&format!("q {0} 0 0 {0} 0 0 cm {body} Q",1.0/unit),""),
        stream(&cmap,""),
    ];
    for index in 0..nesting {
        objects.push(stream(if index+1==nesting {&text}else{"/Next Do"},&format!("/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> /XObject << /Next {} 0 R >> {props} >>",8+index)));
    }
    document(objects)
}

pub fn source_duplicate(
    unit: f64,
    rotation: u16,
    nesting: usize,
    render_mode: u8,
    operator: &str,
    revised: bool,
) -> Vec<u8> {
    let show = if !revised {
        "(C) Tj"
    } else {
        match operator {
            "quote" => "(C) Tj 0 TL (C) '",
            "double" => "(C) Tj 0 TL 0 0 (C) \"",
            "invalid" => "(C) Tj ET (unconsumed)",
            _ => "[(C) 722 (C)] TJ",
        }
    };
    let close = if revised && operator == "invalid" {
        ""
    } else {
        "ET"
    };
    let text = format!("BT /F1 12 Tf {render_mode} Tr 30 150 Td {show} {close}");
    let body = if nesting == 0 {
        text.as_str()
    } else {
        "/Next Do"
    };
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> /XObject << /Next 6 0 R >> >> /Contents 5 0 R >>",300.0/unit),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        stream(&format!("q {0} 0 0 {0} 0 0 cm {body} Q",1.0/unit),""),
    ];
    for index in 0..nesting {
        objects.push(stream(if index+1==nesting { &text } else { "/Next Do" },
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> /XObject << /Next {} 0 R >> >>",7+index)));
    }
    document(objects)
}

pub fn unobserved_text(
    unit: f64,
    rotation: u16,
    mixed: bool,
    nesting: usize,
    whitespace: bool,
    revised: bool,
) -> Vec<u8> {
    let scalar = if whitespace {
        "0020"
    } else if revised {
        "D83DDE01"
    } else {
        "D83DDE00"
    };
    let cmap = format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Omitted def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange 2 beginbfchar <20> <{scalar}> <43> <0043> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end");
    let missing = "BT /F1 12 Tf 30 200 Td ( ) Tj ET";
    let visible = if mixed {
        "BT /F1 12 Tf 30 150 Td (C) Tj ET"
    } else {
        ""
    };
    let body = if nesting == 0 { missing } else { "/Next Do" };
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> /XObject << /Next 7 0 R >> >> /Contents 5 0 R >>",300.0/unit),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>".into(),
        stream(&format!("q {0} 0 0 {0} 0 0 cm {visible} {body} Q",1.0/unit),""),
        stream(&cmap,""),
    ];
    for index in 0..nesting {
        objects.push(stream(if index+1==nesting { missing } else { "/Next Do" },
          &format!("/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> /XObject << /Next {} 0 R >> >>",8+index)));
    }
    let mut bytes = document(objects);
    if whitespace && revised {
        bytes.extend_from_slice(b"\n% distinct whitespace source\n");
    }
    bytes
}

/// Horizontal and oblique labels have known source strings but no claim that
/// their geometric page reading order is a universal logical reading order.
pub fn angled_label_layout(unit: f64, rotation: u16, nested: bool, revised: bool) -> Vec<u8> {
    let label = if revised {
        "MARK 98 76 54"
    } else {
        "MARK 12 34 56"
    };
    let ink=format!("BT /F1 12 Tf 25 270 Td (Survey heading) Tj ET BT /F1 12 Tf 0.8660254 0.5 -0.5 0.8660254 40 100 Tm ({label}) Tj ET BT /F1 12 Tf 0.5 -0.8660254 0.8660254 0.5 180 200 Tm (EAST 90 20) Tj ET BT /F1 12 Tf 0 1 -1 0 260 80 Tm (NORTH 45) Tj ET");
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [-20 -10 {0} {0}] /CropBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> /XObject << /Outer 6 0 R >> >> /Contents 5 0 R >>",300.0/unit),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        stream(&format!("q {0} 0 0 {0} 0 0 cm {1} Q",1.0/unit, if nested {"/Outer Do"} else {&ink}),""),
        stream("/Inner Do","/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /XObject << /Inner 7 0 R >> >>"),
        stream(&ink,"/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >>"),
    ])
}

/// Source-declared 600-unit glyph advances and separate local coordinates
/// distinguish word spaces, parallel lines and edits independently of extraction.
pub fn oblique_layout(
    unit: f64,
    rotation: u16,
    degrees: f64,
    shape: [f64; 3],
    mode: &str,
    revised: bool,
) -> Vec<u8> {
    let [shear, x_scale, y_scale] = shape;
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (a, b, c, d) = (
        cos * x_scale,
        sin * x_scale,
        (cos * shear - sin) * y_scale,
        (sin * shear + cos) * y_scale,
    );
    let font_size = if mode == "negative-size" { -12 } else { 12 };
    let show = |x: f64, y: f64, text: &str| {
        format!(
            "BT /F1 {font_size} Tf {a} {b} {c} {d} {} {} Tm ({text}) Tj ET\n",
            280.0 + a * x + c * y,
            280.0 + b * x + d * y
        )
    };
    let mut ink = String::new();
    if mode == "letters" {
        for (x, ch) in [
            (0.0, "A"),
            (8.0, "L"),
            (16.0, "P"),
            (24.0, "H"),
            (32.0, "A"),
            (44.0, if revised { "9" } else { "1" }),
            (52.0, if revised { "8" } else { "2" }),
            (64.0, "B"),
            (72.0, "E"),
            (80.0, "T"),
            (88.0, "A"),
        ] {
            ink.push_str(&show(x, 0.0, ch));
        }
    } else {
        ink.push_str(&show(
            0.0,
            0.0,
            if revised {
                "ALPHA 98 BETA"
            } else {
                "ALPHA 12 BETA"
            },
        ));
    }
    if mode == "parallel" {
        ink.push_str(&show(0.0, 32.0, "OTHER 77"));
    }
    if mode == "crossed" {
        ink = show(0.0, 0.0, "A") + &show(8.0, 14.0, "B");
    }
    if mode == "backtrack" {
        ink = show(0.0, 0.0, "A") + &show(-30.0, 0.0, "B");
    }
    if mode == "far" {
        ink = show(0.0, 0.0, "ALPHA") + &show(130.0, 0.0, "BETA");
    }
    let cmap=format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /ObliqueGold def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange 37 beginbfchar <20> <0020> {} endbfchar endcmap CMapName currentdict /CMap defineresource pop end end",
       (b'0'..=b'9').chain(b'A'..=b'Z').map(|x|format!("<{x:02X}> <{x:04X}>")).collect::<Vec<_>>().join(" "));
    let font=format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 32 /LastChar 90 /Widths [{}] /ToUnicode 6 0 R >>",
        (32..=90).map(|x| if x==32 {"300"} else {"600"}).collect::<Vec<_>>().join(" "));
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> /XObject << /Outer 7 0 R >> >> /Contents 5 0 R >>",600.0/unit),
        font,
        stream(&format!("q {0} 0 0 {0} 0 0 cm {1} Q",1.0/unit,if mode=="nested" {"/Outer Do"} else {&ink}),""),
        stream(&cmap,""),
        stream("/Inner Do","/Type /XObject /Subtype /Form /BBox [0 0 600 600] /Resources << /XObject << /Inner 8 0 R >> >>"),
        stream(&ink,"/Type /XObject /Subtype /Form /BBox [0 0 600 600] /Resources << /Font << /F1 4 0 R >> >>"),
    ])
}

pub fn fixture(case: &str, revised: bool) -> Vec<u8> {
    if let Some(mode) = case.strip_prefix("agl_") {
        return glyph_named_text(mode, revised);
    }
    if case == "actual_space_separator" {
        return marked_space_between_touching_words(revised);
    }
    if let Some(mode) = case.strip_prefix("marked_scope_") {
        return marked_actual_scopes(1., 0, mode, revised);
    }
    if let Some(mode) = case.strip_prefix("actual_structure_") {
        return structure_actual(1.0, 0, mode, revised);
    }
    if case.starts_with("source_unicode_") {
        let mode = match case {
            "source_unicode_masked_range" => "range",
            "source_unicode_actual" => "actual",
            "source_unicode_actual_named" => "actual_named",
            "source_unicode_actual_empty" => "actual_empty",
            "source_unicode_actual_blank" => "actual_blank",
            "source_unicode_actual_large" => "actual_large",
            "source_unicode_actual_invalid" => "actual_invalid",
            _ => "masked",
        };
        return source_unicode(
            1.0,
            0,
            if mode == "actual_named" { 2 } else { 0 },
            mode,
            revised,
        );
    }

    if case.starts_with("source_") {
        return source_duplicate(
            1.0,
            0,
            usize::from(case == "source_duplicate_nested") * 2,
            if case == "source_duplicate_visible" {
                0
            } else {
                3
            },
            if case == "source_duplicate_quote" {
                "quote"
            } else if case == "source_trailing_invalid" {
                "invalid"
            } else {
                "TJ"
            },
            revised,
        );
    }

    assert!(CASES.contains(&case), "Unknown appearance fixture: {case}");
    if let Some(mode) = case.strip_prefix("oblique_") {
        return oblique_layout(
            if revised && mode != "edit" { 64.0 } else { 1.0 },
            if revised && mode != "edit" { 90 } else { 0 },
            45.0,
            [
                if mode == "shear" { 0.25 } else { 0.0 },
                if mode == "shear" { 1.5 } else { 1.0 },
                1.0,
            ],
            if mode == "shear" || mode == "edit" {
                "whole"
            } else {
                mode
            },
            revised && mode == "edit",
        );
    }
    if case.starts_with("angled_label_") {
        return angled_label_layout(
            if revised && case == "angled_label_units" {
                64.0
            } else {
                1.0
            },
            if revised && case.ends_with("rotation") {
                90
            } else {
                0
            },
            case.contains("nested"),
            revised && case == "angled_label_edit",
        );
    }
    if case.starts_with("unobserved_") {
        return unobserved_text(
            if revised && case == "unobserved_units" {
                64.0
            } else {
                1.0
            },
            0,
            case.ends_with("mixed"),
            if case.contains("nested") { 2 } else { 0 },
            case.ends_with("whitespace"),
            revised,
        );
    }
    if case == "surrogate_unpaired" {
        let mut bytes = supplementary_unicode_mode(1.0, 0, "split");
        if revised {
            bytes.extend_from_slice(b"\n% surrogate rewrite\n");
        }
        return bytes;
    }
    if case.starts_with("supplementary_unicode") {
        return supplementary_unicode(
            if revised && case == "supplementary_unicode" {
                64.0
            } else {
                1.0
            },
            0,
            revised && case == "supplementary_unicode_edit",
        );
    }

    if let Some(mode) = case.strip_prefix("form_orphan_") {
        return orphan_widgets(
            match mode {
                "merged" => "no-form",
                other => other,
            },
            revised,
        );
    }
    if matches!(case, "form_catalog_invalid" | "form_catalog_null") {
        let mut bytes = catalog_form_entry(match case {
            "form_catalog_invalid" => {
                if revised {
                    "name"
                } else {
                    "number"
                }
            }
            _ => {
                if revised {
                    "null"
                } else {
                    "absent"
                }
            }
        });
        if revised {
            bytes.extend_from_slice(
                b"
% rewritten
",
            );
        }
        return bytes;
    }
    if case == "form_repeated_widget" {
        return repeated_field_control("flags", revised);
    }
    if case == "form_null_entries" {
        return document(vec![
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] >> >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
            stream("BT /F1 12 Tf 30 100 Td (Stable page) Tj ET", ""),
            format!("<< /FT /Tx /T (Name) {} >>", if revised { "/V null /Ff null /Q null /AA null" } else { "" }),
        ]);
    }

    if case.starts_with("empty_form_") {
        let mode = match case {
            "empty_form_shell" | "empty_form_shell_edit" => {
                if revised {
                    "indirect-defaults"
                } else {
                    "defaults"
                }
            }
            "empty_form_orphan_widget" => "orphan-widget",
            "empty_form_xfa" => "xfa",
            "empty_form_action" => "action",
            _ => "unknown",
        };
        let mut bytes = empty_form_shell(mode, revised && case == "empty_form_shell_edit", revised);
        if revised {
            bytes.extend_from_slice(b"\n% harmless rewrite\n");
        }
        return bytes;
    }
    if case.starts_with("tagged_table_stale_") {
        return spanning_tagged_table_layout(
            if revised && case != "tagged_table_stale_edit" {
                64.0
            } else {
                1.0
            },
            0,
            if case == "tagged_table_stale_ambiguous" {
                "stale-two-orphans"
            } else if revised {
                "stale-shuffled-rows"
            } else {
                "stale-header"
            },
            case == "tagged_table_stale_rtl",
            true,
            revised && case == "tagged_table_stale_edit",
        );
    }
    if case.starts_with("tagged_table_spans_") {
        return spanning_tagged_table_layout(
            if revised && case != "tagged_table_spans_edit" {
                64.0
            } else {
                1.0
            },
            0,
            if revised { "shuffled-rows" } else { "complete" },
            case == "tagged_table_spans_rtl",
            true,
            revised && case == "tagged_table_spans_edit",
        );
    }
    if case == "crop_table_identity" {
        return tagged_table_layout(
            1.0,
            0,
            if revised { "crop-new" } else { "crop-old" },
            true,
            false,
        );
    }
    if matches!(
        case,
        "tagged_table_shear_units" | "tagged_table_shear_edit" | "tagged_table_diagonal"
    ) {
        return tagged_table_layout(
            if revised { 64.0 } else { 1.0 },
            0,
            if case == "tagged_table_diagonal" {
                "baseline-diagonal"
            } else {
                "shear-positive"
            },
            true,
            revised && case == "tagged_table_shear_edit",
        );
    }
    if case.starts_with("tagged_table_") {
        return tagged_table_layout(
            if revised { 64.0 } else { 1.0 },
            0,
            "complete",
            true,
            revised && case == "tagged_table_edit",
        );
    }
    if case.starts_with("construction_") {
        return construction_pieces(
            if revised { 64.0 } else { 1.0 },
            0,
            case == "construction_shared_units",
        );
    }
    if matches!(case, "layout_units" | "layout_units_edit") {
        return rescaled_layout(
            if revised { 64.0 } else { 1.0 },
            0,
            revised && case == "layout_units_edit",
        );
    }
    if case == "space_units" {
        return spaced_layout(if revised { 64.0 } else { 1.0 });
    }
    if matches!(case, "widget_units" | "widget_units_edit") {
        return rescaled_radio(
            if revised { 72.0 } else { 1.0 },
            revised && case == "widget_units_edit",
        );
    }
    if case == "form_deep_property" {
        let mut action = format!(
            "<< /S /JavaScript /JS ({}) >>",
            if revised { "2" } else { "1" }
        );
        for _ in 0..60 {
            action = format!("<< /S /JavaScript /JS (void 0) /Next {action} >>");
        }
        return document(vec![
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] >> >>".into(),
            "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
            stream("BT /F1 12 Tf 30 100 Td (Stable page) Tj ET",""),
            format!("<< /FT /Tx /T (deep) /AA << /K {action} >> >>"),
        ]);
    }
    if case.starts_with("user_unit_") {
        let unit = match case {
            "user_unit_scale" => {
                if revised {
                    3
                } else {
                    1
                }
            }
            "user_unit_invalid" => 0,
            "user_unit_limit" => 75_000,
            _ => unreachable!(),
        };
        let mut bytes = document(vec![
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /UserUnit {unit} /Resources << >> /Contents 4 0 R >>"),
            stream("0 g 60 80 40 20 re f", ""),
        ]);
        if revised {
            bytes.extend_from_slice(b"\n% revised\n");
        }
        return bytes;
    }
    if matches!(
        case,
        "form_hybrid_xfa"
            | "form_partial_property"
            | "form_stream_reencoded"
            | "form_corrupt_stream"
    ) {
        let text = "Alpha ".repeat(64);
        let hex = if case == "form_corrupt_stream" && revised {
            "789c73cc29c84854701c250788040081af8180"
        } else {
            "789c73cc29c84854701c250788040081af8181"
        };
        let value = if case == "form_partial_property" {
            "<FEFF0041FF>".to_owned()
        } else if case == "form_hybrid_xfa" {
            format!("({})", if revised { "changed" } else { "original" })
        } else if case == "form_stream_reencoded" && !revised {
            format!("({text})")
        } else {
            "7 0 R".to_owned()
        };
        let flags =
            usize::from(revised && matches!(case, "form_partial_property" | "form_corrupt_stream"));
        let xfa = if case == "form_hybrid_xfa" {
            "/XFA [(datasets) 8 0 R]"
        } else {
            ""
        };
        return document(vec![
            format!("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] {xfa} >> >>"),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
            stream("BT /F1 12 Tf 30 100 Td (Stable page) Tj ET",""),
            format!("<< /FT /Tx /T (Name) /Ff {flags} /V {value} >>"),
            stream(&format!("{hex}>"),"/Filter [/ASCIIHexDecode /FlateDecode]"),
            stream("<xfa:datasets xmlns:xfa=\"http://www.xfa.org/schema/xfa-data/1.0/\"><xfa:data/></xfa:datasets>",""),
        ]);
    }
    if matches!(case, "form_unplaced" | "form_multi_export") {
        let field = if case == "form_unplaced" {
            format!(
                "<< /FT /Tx /T (Unplaced) /V ({}) >>",
                if revised { "changed" } else { "original" }
            )
        } else {
            format!("<< /FT /Ch /T (Choices) /Ff 2097152 /Opt [(one) (two) (three)] /I [0 {}] /V [(one) ({})] >>",if revised {2}else{1},if revised {"three"}else{"two"})
        };
        return document(vec![
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] >> >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
            stream("BT /F1 12 Tf 30 100 Td (Stable page) Tj ET",""),field,
        ]);
    }
    if case.starts_with("tounicode_override") {
        let mut objects = vec![
            format!("<< /Type /Catalog /Pages 2 0 R /JadeRevision {revised} >>"),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>".into(),
            stream("BT /F1 12 Tf 50 100 Td <41427071> Tj ET", ""),
            stream("/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Jade) /Ordering (Unicode) /Supplement 0 >> def /CMapName /JadeOverrides def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfrange <00> <FF> <0000> endbfrange 2 beginbfchar <41> <00F3> <42> <20AC> endbfchar 1 beginbfrange <70> <71> <00E9> endbfrange endcmap CMapName currentdict /CMap defineresource pop end end", ""),
        ];
        if case == "tounicode_override_edit" && revised {
            // Stream length stays equal; only the declared Unicode changes.
            objects[5] = objects[5].replace("<00F3>", "<00F2>");
        }
        return document(objects);
    }
    if case.starts_with("mapping_error_") {
        let mixed = case == "mapping_error_mixed";
        let code = if case == "mapping_error_control" {
            "000D"
        } else {
            "0041"
        };
        return document(vec![
            format!("<< /Type /Catalog /Pages 2 0 R /JadeRevision {} >>", revised),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R /F2 8 0 R >> >> /Contents 5 0 R >>".into(),
            "<< /Type /Font /Subtype /Type0 /BaseFont /JadeUnmapped /Encoding /Identity-H /DescendantFonts [6 0 R] >>".into(),
            stream(&format!("BT /F1 12 Tf 50 100 Td <{code}> Tj ET {}", if mixed {"BT /F2 12 Tf 58 100 Td (ok) Tj ET"} else {""}), ""),
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /JadeUnmapped /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 7 0 R /DW 500 /CIDToGIDMap /Identity >>".into(),
            "<< /Type /FontDescriptor /FontName /JadeUnmapped /Flags 4 /FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 >>".into(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        ]);
    }
    if case == "budget" {
        return document(vec![
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> /Contents 5 0 R >>".into(),
            "null".into(),stream("0.1 g 70 80 40 20 re f",""),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> /Contents 7 0 R >>".into(),
            stream(if revised {"0.8 g 70 80 40 20 re f"}else{"0.1 g 70 80 40 20 re f"},""),
        ]);
    }
    let form = matches!(
        case,
        "form_value"
            | "form_flags"
            | "oversized_widget_name"
            | "oversized_widget_value"
            | "form_hidden"
            | "form_unicode"
            | "checkbox"
            | "checkbox_export"
            | "checkbox_unchecked_export"
            | "list"
            | "combo_custom"
            | "combo_no_options"
            | "choice_export_label"
            | "radio_reordered"
    );
    let image = matches!(
        case,
        "partial_scan"
            | "image_only"
            | "image_reencoded"
            | "oversized_image"
            | "nested_oversized_image"
    );
    let font = if case == "text_face" && revised {
        "Courier"
    } else {
        "Helvetica"
    };
    let mut content = if case == "line_hyphen" {
        format!(
            "BT /F1 12 Tf 20 220 Td (well{}) Tj 0 -16 Td (known) Tj ET",
            if revised { "=" } else { "-" }
        )
    } else if case == "image_only" {
        String::new()
    } else {
        format!(
            "BT /F1 12 Tf {} 20 260 Td (LOGO MMMM) Tj ET\n",
            if case == "text_color" && revised {
                "1 0 0 rg"
            } else {
                "0 g"
            }
        )
    };
    if image {
        content.push_str("q 100 0 0 100 50 50 cm /I Do Q\n");
    }
    if case == "path" || case == "oversized" {
        content.push_str(if revised {
            "0.8 g 70 80 40 20 re f"
        } else {
            "0.1 g 70 80 40 20 re f"
        });
    }
    let mut objects=vec![
        format!("<< /Type /Catalog /Pages 2 0 R {} >>",if form {if case=="radio_reordered" {"/AcroForm 10 0 R"}else{"/AcroForm 8 0 R"}}else{""}),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /Resources << /Font << /F1 4 0 R >> {1} >> /Contents 5 0 R {2} >>",
            if case=="oversized"{12000}else{300},if image {"/XObject << /I 6 0 R >>"}else{""},if form {if case=="radio_reordered" {if revised {"/Annots [9 0 R 6 0 R]"}else{"/Annots [6 0 R 9 0 R]"}}else{"/Annots [6 0 R]"}}else{""}),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /{font} >>"),stream(&content,""),
    ];
    if image {
        let red = if revised && case != "image_reencoded" {
            80
        } else {
            40
        };
        let bytes = [red, 20, 10, red, 20, 10, red, 20, 10, red, 20, 10];
        let raw = case == "image_reencoded" && revised;
        let data = if raw {
            String::from_utf8(bytes.to_vec()).unwrap()
        } else {
            bytes.iter().map(|b| format!("{b:02X}")).collect::<String>() + ">"
        };
        let side = if matches!(case, "oversized_image" | "nested_oversized_image") {
            100_000
        } else {
            2
        };
        if case == "nested_oversized_image" {
            objects.push(stream("/I2 Do","/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources << /XObject << /I2 7 0 R >> >>"));
        }
        objects.push(stream(&data,&format!("/Type /XObject /Subtype /Image /Width {side} /Height {side} /BitsPerComponent 8 /ColorSpace /DeviceRGB {}",if raw {""}else{"/Filter /ASCIIHexDecode"})));
    } else if form {
        let field=match case {
            "checkbox" | "checkbox_export" | "checkbox_unchecked_export" => {
                let on=if case!="checkbox"&&revised {"Yep"}else{"Yes"};
                let value=if case=="checkbox_unchecked_export" {"Off"}else if revised||case=="checkbox_export" {on}else{"Off"};
                format!("/FT /Btn /T (agree) /V /{value} /AS /{value} /AP << /N << /Off 7 0 R /{on} 7 0 R >> >>")
            },
            "list"=>format!("/FT /Ch /T (choices) /Ff 2097152 /Opt [(A) (B) (C)] /V [(A) ({})] /I [0 {}] /AP << /N 7 0 R >>",if revised {"C"}else{"B"},if revised {2}else{1}),
            "choice_export_label"=>format!("/FT /Ch /T (choice_export) /Opt [[(foo) (Foo)] [(bar) (Bar)]] /V ({}) /I [{}] /AP << /N 7 0 R >>",if revised {"bar"}else{"foo"},if revised {1}else{0}),
            "combo_no_options"=>format!("/FT /Ch /T (editable) /Ff 393216 /V (Typed {}) /AP << /N 7 0 R >>",if revised {"new"}else{"old"}),
            "combo_custom"=>format!("/FT /Ch /T (editable) /Ff 393216 /Opt [(A) (B)] /V (Typed {}) /I [] /AP << /N 7 0 R >>",if revised {"new"}else{"old"}),
            "radio_reordered"=>"/Parent 8 0 R /AS /A /AP << /N << /Off 7 0 R /A 7 0 R >> >>".into(),
            "form_flags"=>format!("/FT /Tx /T (customer) /V (Alice) /Ff {} /DA (/F1 12 Tf 0 g) /AP << /N 7 0 R >>",if revised {3}else{0}),
            "oversized_widget_name" | "oversized_widget_value" => {
                let huge = if revised { "x".repeat(2_000_001) } else { "short".into() };
                let (name,value) = if case == "oversized_widget_name" { (huge.as_str(), "Alice") } else { ("customer", huge.as_str()) };
                format!("/FT /Tx /T ({name}) /V ({value}) /DA (/F1 12 Tf 0 g) /AP << /N 7 0 R >>")
            },
            _=>format!("/FT /Tx /T {} /V {} /DA (/F1 12 Tf 0 g) /AP << /N 7 0 R >>",
                if case=="form_unicode" {utf16("氏名")}else{"(customer)".into()},
                if case=="form_unicode" {utf16(if revised {"山田花子"}else{"山田太郎"})}else{format!("({})",if revised {"Bob"}else{"Alice"})}),
        };
        objects.push(format!(
            "<< /Type /Annot /Subtype /Widget /P 3 0 R /Rect [50 50 150 80] /F {} {field} >>",
            if case == "form_hidden" { 2 } else { 4 }
        ));
        objects.push(stream("q 1 g 0 0 100 30 re f 0 g BT /F1 12 Tf 4 10 Td (STATIC) Tj ET Q",
            "/Type /XObject /Subtype /Form /BBox [0 0 100 30] /Resources << /Font << /F1 4 0 R >> >>"));
        if case == "radio_reordered" {
            objects.push(format!(
                "<< /FT /Btn /Ff 32768 /T (choice) /V /A /Kids [{}] >>",
                if revised {
                    "9 0 R 6 0 R"
                } else {
                    "6 0 R 9 0 R"
                }
            ));
            objects.push("<< /Type /Annot /Subtype /Widget /P 3 0 R /Rect [160 50 260 80] /F 4 /Parent 8 0 R /AS /Off /AP << /N << /Off 7 0 R /B 7 0 R >> >> >>".into());
            objects.push(
                "<< /Fields [8 0 R] /DR << /Font << /F1 4 0 R >> >> /NeedAppearances false >>"
                    .into(),
            );
        } else {
            objects.push(
                "<< /Fields [6 0 R] /DR << /Font << /F1 4 0 R >> >> /NeedAppearances false >>"
                    .into(),
            );
        }
    }
    document(objects)
}

/// Identical physical page/text with a different raw coordinate basis.
pub fn rescaled_layout(unit: f64, rotation: u64, edited: bool) -> Vec<u8> {
    let scale = 1.0 / unit;
    let content = format!(
        "q {scale:.17} 0 0 {scale:.17} 0 0 cm
BT /F1 12 Tf 20 270 Td (A heading spanning the two columns) Tj ET
BT /F1 12 Tf 20 200 Td (Left first) Tj ET
BT /F1 12 Tf 180 200 Td (Right first) Tj ET
BT /F1 12 Tf 20 180 Td (Left last) Tj ET
BT /F1 12 Tf 180 180 Td (Right {}) Tj ET
BT /F1 12 Tf 20 100 Td (A footer spanning the two columns) Tj ET Q",
        if edited { "edit" } else { "last" }
    );
    let dimension = 300.0 / unit;
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {dimension:.14} {dimension:.14}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        stream(&content, ""),
    ])
}

fn rescaled_radio(unit: f64, edited: bool) -> Vec<u8> {
    let scale = 1.0 / unit;
    let dimension = 300.0 / unit;
    let rect = |left: f64, right: f64| {
        format!(
            "{:.14} {:.14} {:.14} {:.14}",
            left / unit,
            50.0 / unit,
            right / unit,
            80.0 / unit
        )
    };
    let content = format!(
        "q {scale:.17} 0 0 {scale:.17} 0 0 cm BT /F1 12 Tf 20 200 Td (Stable page) Tj ET Q"
    );
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R /AcroForm 10 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {dimension:.14} {dimension:.14}] /UserUnit {unit} /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R /Annots [6 0 R 9 0 R] >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        stream(&content, ""),
        format!("<< /Type /Annot /Subtype /Widget /P 3 0 R /Rect [{}] /F 4 /Parent 8 0 R /AS /{} /AP << /N << /Off 7 0 R /A 7 0 R >> >> >>",rect(50.0,150.0),if edited {"Off"} else {"A"}),
        stream("q 1 g 0 0 100 30 re f 0 g BT /F1 12 Tf 4 10 Td (STATIC) Tj ET Q", "/Type /XObject /Subtype /Form /BBox [0 0 100 30] /Resources << /Font << /F1 4 0 R >> >>"),
        format!("<< /FT /Btn /Ff 32768 /T (choice) /V /{} /Kids [6 0 R 9 0 R] >>",if edited {"B"} else {"A"}),
        format!("<< /Type /Annot /Subtype /Widget /P 3 0 R /Rect [{}] /F 4 /Parent 8 0 R /AS /{} /AP << /N << /Off 7 0 R /B 7 0 R >> >> >>",rect(160.0,260.0),if edited {"B"} else {"Off"}),
        "<< /Fields [8 0 R] /DR << /Font << /F1 4 0 R >> >> /NeedAppearances false >>".into(),
    ])
}

pub fn spaced_layout(unit: f64) -> Vec<u8> {
    let scale = 1.0 / unit;
    let dimension = 300.0 / unit;
    let content = format!(
        "q {scale:.17} 0 0 {scale:.17} 0 0 cm
BT /F1 12 Tf 20 200 Td (Alpha) Tj ET
BT /F2 12 Tf 60 200 Td (Beta) Tj ET Q"
    );
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {dimension:.14} {dimension:.14}] /UserUnit {unit} /Resources << /Font << /F1 4 0 R /F2 5 0 R >> >> /Contents 6 0 R >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".into(),
        stream(&content, ""),
    ])
}

pub fn marked_space_between_touching_words(revised: bool) -> Vec<u8> {
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        stream(&format!("BT /F1 12 Tf 3 Tr 30 150 Td (A) Tj ET /Span << /ActualText ( ) >> BDC BT /F1 12 Tf 3 Tr 38 150 Td (C) Tj ET EMC BT /F1 12 Tf 3 Tr 38 150 Td ({}) Tj ET",if revised {"D"} else {"B"}), ""),
    ])
}

/// The encoded byte and invisible paint are fixed; only its source glyph name
/// changes. AGL names can begin with uni/u without being algorithmic names.
pub fn glyph_named_text(mode: &str, revised: bool) -> Vec<u8> {
    let name = if !revised {
        "A"
    } else {
        match mode {
            "union" => "union",
            "universal" => "universal",
            "uacute" => "uacute",
            "ugrave" => "ugrave",
            "nonbmp" => "u1040C",
            "multicomponent" => "Lcommaaccent_uni20AC0308_u1040C.alternate",
            "alias" => "circlecopyrt",
            "surrogate" => "uniD801DC0C",
            "lowercase" => "uni20ac",
            "mixed_unknown" => "A_unknown",
            _ => panic!("Unknown glyph mapping mode"),
        }
    };
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding << /BaseEncoding /WinAnsiEncoding /Differences [65 /{name}] >> >>"),
        stream("BT /F1 12 Tf 3 Tr 30 150 Td (A) Tj ET", ""),
    ])
}
