//! 白い目隠しの塗りを黒罫線へ変え、離れた表を接続しない。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
#[allow(dead_code)]
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 所有する一ページの本文とResourceだけから正式なPDFを組み立てる。
fn source(content: &str, resources: &str) -> Vec<u8> {
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!(
            "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 500 500] /Resources {resources} >>"
        ),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into(),
        page_attribute_pdf::stream(content),
    ])
}

/// 白い中央帯を挟む黒い二本の原本中心線を、小数ptのまま返す。
fn source_edges(prefix: &str, resources: &str) -> Vec<[f32; 4]> {
    let content =
        format!("{prefix} 0 g 20 100 100 1 re f 1 g 120 100 20 1 re f 0 g 140 100 100 1 re f");
    let parsed =
        parse_pdf_bytes("owned-white-clearance.pdf", &source(&content, resources)).unwrap();
    parsed
        .graphics
        .iter()
        .map(|g| [g.bounds.x0, g.bounds.y0, g.bounds.x1, g.bounds.y1])
        .collect()
}

/// 既存の粗いgrid比較は明示的に丸め、小数座標を整数castで切り捨てない。
fn edges(prefix: &str, resources: &str) -> Vec<[i32; 4]> {
    source_edges(prefix, resources)
        .into_iter()
        .map(|b| b.map(|v| v.round() as i32))
        .collect()
}

/// DeviceGrayの白塗りは黒い中心線ではなく、離れた黒罫線の空白帯として残す。
#[test]
fn opaque_white_fill_does_not_bridge_black_rules() {
    assert_eq!(
        source_edges("", "<< >>"),
        [[20., 100.5, 120., 100.5], [140., 100.5, 240., 100.5]]
    );
    assert_eq!(
        edges("", "<< >>"),
        [[20, 101, 120, 101], [140, 101, 240, 101]]
    );
}

/// 印刷用の既定設定が白を反転しないことを確認しても、白い目隠しから線を作らない。
#[test]
fn normal_print_defaults_keep_white_clearance_between_rules() {
    let resources = "<< /ExtGState << /Print << /Type /ExtGState /BG2 /Default /UCR2 /Default /OP false /op false /OPM 1 /SA false /SM 0.02 >> >> >>";
    assert_eq!(
        edges("/Print gs", resources),
        [[20, 101, 120, 101], [140, 101, 240, 101]]
    );
}

/// 候補一本の抽出件数だけを公開APIで確認する。
fn count(content: &str, resources: &str) -> usize {
    parse_pdf_bytes("owned-paint.pdf", &source(content, resources))
        .unwrap()
        .graphics
        .len()
}

/// 三つのDevice色空間と成分範囲のclipを同じ白として扱う。
#[test]
fn device_white_and_clamped_white_are_not_black_rules() {
    for color in [
        "1 g",
        "2 g",
        "1 1 1 rg",
        "2 3 4 rg",
        "0 0 0 0 k",
        "-1 -2 -3 -4 k",
    ] {
        assert_eq!(
            count(&format!("{color} 20 100 100 1 re f"), "<< >>"),
            0,
            "{color}"
        );
    }
}

/// 近い灰色・有色・stroke色だけの変更は既存の罫線を消さない。
#[test]
fn nonwhite_and_stroke_colors_preserve_fill_rules() {
    for color in [
        ".999 g",
        "1 .999 1 rg",
        "0 0 0 .001 k",
        "1 G",
        "1 1 1 RG",
        "0 0 0 0 K",
        "1 g /DeviceGray cs .5 sc",
    ] {
        assert_eq!(
            count(&format!("{color} 20 100 100 1 re f"), "<< >>"),
            1,
            "{color}"
        );
    }
    assert_eq!(count("1 g 20 100 m 120 100 l S", "<< >>"), 1);
}

/// q/Qは非stroke色を復元し、白い一時塗りが後続黒線へ漏れない。
#[test]
fn saved_fill_color_is_restored() {
    assert_eq!(
        count("0 g q 1 g 20 100 100 1 re f Q 20 200 100 1 re f", "<< >>"),
        1
    );
    assert_eq!(
        count("1 g q 0 g 20 100 100 1 re f Q 20 200 100 1 re f", "<< >>"),
        1
    );
}

/// 未知の合成・転写・既定色空間では白を確定せず従来の候補を保存する。
#[test]
fn unknown_effects_and_default_color_spaces_keep_legacy_rules() {
    for state in [
        "/ca .5",
        "/BM /Multiply",
        "/SMask << /S /Alpha >>",
        "/TR /Identity",
        "/BG2 8 0 R",
        "/ca 1 /ca 1",
        "/ca 1 /c#61 .5",
    ] {
        let resources = format!("<< /ExtGState << /P << {state} >> >> >>");
        assert_eq!(
            count("/P gs 1 g 20 100 100 1 re f", &resources),
            1,
            "{state}"
        );
    }
    for resources in [
        "<< /ColorSpace << /DefaultGray /DeviceGray >> >>",
        "<< /ColorSpace << /DefaultRGB /DeviceRGB >> >>",
        "<< /ColorSpace << /DefaultCMYK /DeviceCMYK >> >>",
        "<< /ColorSp#61ce << /DefaultGray /DeviceGray >> >>",
        "<< /ExtGState << /P << >> /P << >> >> >>",
    ] {
        assert_eq!(count("1 g 20 100 100 1 re f", resources), 1, "{resources}");
    }
    assert_eq!(count("/Unknown gs 1 g 20 100 100 1 re f", "<< >>"), 1);
    assert_eq!(
        count(
            "/P#31 gs 1 g 20 100 100 1 re f",
            "<< /ExtGState << /P1 << >> >> >>"
        ),
        1
    );
}

/// 分割Contentsを字句の間に余計な空白を挿入せず正式PDFにする。
fn split_source(first: &str, second: &str) -> Vec<u8> {
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 500 500] /Resources << >> >>".into(),
        "<< /Type /Page /Parent 2 0 R /Contents [4 0 R 5 0 R] >>".into(),
        page_attribute_pdf::stream(first),
        page_attribute_pdf::stream(second),
    ])
}

/// ストリーム境界をまたぐQが黒を復元する場合、局所走査の白から黒線を消さない。
#[test]
fn cross_stream_restore_does_not_erase_actual_black_fill() {
    let pdf = split_source("0 g q ", "1 g Q 20 100 100 1 re f");
    assert_eq!(
        parse_pdf_bytes("owned-split-state.pdf", &pdf)
            .unwrap()
            .graphics
            .len(),
        1
    );
}

/// 境界をまたぐ未知gsは、その後の白候補も未確認として保存する。
#[test]
fn split_unknown_operator_disables_white_proof() {
    let pdf = split_source("/Unknown g", "s 1 g 20 100 100 1 re f");
    assert_eq!(
        parse_pdf_bytes("owned-split-gs.pdf", &pdf)
            .unwrap()
            .graphics
            .len(),
        1
    );
}

/// 配列が分割されても後続の確認済み白命令の位置を一致させる。
#[test]
fn split_array_preserves_following_white_operator_identity() {
    let pdf = split_source("[(abc) ", "(def)] TJ 1 g 20 100 100 1 re f");
    assert!(
        parse_pdf_bytes("owned-split-array.pdf", &pdf)
            .unwrap()
            .graphics
            .is_empty()
    );
}

/// ResourcesとgsをObjStmへ保存した、正式xref stream付きの所有PDF。
fn compressed_resources() -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = [0_usize; 9];
    for (number, body) in [
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 500 500] /Resources 5 0 R >>"
                .into(),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (4, page_attribute_pdf::stream("/P gs 1 g 20 100 100 1 re f")),
    ] {
        offsets[number] = bytes.len();
        bytes.extend_from_slice(format!("{number} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let resource = "<< /ExtGState << /P 8 0 R >> >> ";
    let header = format!("5 0 8 {} ", resource.len());
    let payload = format!("{header}{resource}<< /ca 1 /BG2 /Default /UCR2 /Default >>");
    offsets[6] = bytes.len();
    bytes.extend_from_slice(format!("6 0 obj\n<< /Type /ObjStm /N 2 /First {} /Length {} >>\nstream\n{payload}\nendstream\nendobj\n",header.len(),payload.len()).as_bytes());
    offsets[7] = bytes.len();
    let mut entries = Vec::new();
    for (number, offset) in offsets.iter().enumerate() {
        let (kind, field1, field2): (u8, u32, u16) = match number {
            0 => (0, 0, 65535),
            5 => (2, 6, 0),
            8 => (2, 6, 1),
            _ => (1, *offset as u32, 0),
        };
        entries.push(kind);
        entries.extend_from_slice(&field1.to_be_bytes());
        entries.extend_from_slice(&field2.to_be_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "7 0 obj\n<< /Type /XRef /Size 9 /Root 1 0 R /W [1 4 2] /Length {} >>\nstream\n",
            entries.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(&entries);
    bytes.extend_from_slice(
        format!("\nendstream\nendobj\nstartxref\n{}\n%%EOF\n", offsets[7]).as_bytes(),
    );
    bytes
}

/// 検証済みの圧縮Resourceとgsも参照し、白い塗りを黒線として発行しない。
#[test]
fn compressed_resource_and_graphics_state_confirm_white_fill() {
    let parsed = parse_pdf_bytes("owned-compressed-paint.pdf", &compressed_resources()).unwrap();
    assert!(parsed.graphics.is_empty(), "{:?}", parsed.graphics);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 白い間隔の幅・座標・塗る順序が変わっても、元の黒線二本だけを返す。
    #[test]
    fn white_clearance_keeps_two_source_intervals(x in 10i32..100, gap in 3i32..40, reversed in any::<bool>()) {
        let left = format!("0 g {x} 100 40 1 re f ");
        let white = format!("1 g {} 100 {gap} 1 re f ", x + 40);
        let right = format!("0 g {} 100 40 1 re f ", x + 40 + gap);
        let content = if reversed { format!("{right}{white}{left}") } else { format!("{left}{white}{right}") };
        let parsed = parse_pdf_bytes("owned-gap.pdf", &source(&content, "<< >>")).unwrap();
        let actual: Vec<_> = parsed.graphics.iter().map(|g| (g.bounds.x0 as i32, g.bounds.x1 as i32)).collect();
        prop_assert_eq!(actual, vec![(x, x + 40), (x + 40 + gap, x + 80 + gap)]);
    }
}
