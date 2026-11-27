//! 対角線の方向・描画状態・clip・描画順を、正式な公開PDF入力から検証する。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
#[allow(dead_code)]
#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// 任意のContents列・継承Resource・ページ属性を持つ原本を作る。
fn document(contents: &[&str], resources: &str, attributes: &str) -> ParsedDocument {
    let refs = (0..contents.len())
        .map(|i| format!("{} 0 R", i + 4))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!(
            "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 500 500] /Resources {resources} >>"
        ),
        format!("<< /Type /Page /Parent 2 0 R /Contents [{refs}] {attributes} >>"),
    ];
    objects.extend(contents.iter().map(|s| fixture::stream(s)));
    parse_pdf_bytes("owned-source-diagonals.pdf", &fixture::pdf(&objects)).unwrap()
}

/// 不正な線幅命令を、既定の線幅の根拠へ黙って戻さない。
#[test]
fn an_invalid_stroke_width_does_not_recover_the_previous_known_width() {
    let parsed = document(&["NaN w 100 200 m 200 100 l S"], "<< >>", "");
    assert!(
        parsed.source_diagonals.is_empty(),
        "an unknown stroke width cannot prove a source diagonal"
    );
}

/// 不明なline cap・変換・座標や、miterが伸びる連結pathを単純な可視線と見なさない。
#[test]
fn unverified_caps_matrices_and_connected_paths_do_not_gain_a_diagonal() {
    for content in [
        "3 J 100 200 m 200 100 l S",
        "NaN 0 0 1 0 0 cm 100 200 m 200 100 l S",
        "100 200 m NaN 100 l 200 100 l S",
        "100 200 m 200 100 l 210 150 l S",
    ] {
        assert!(
            document(&[content], "<< >>", "")
                .source_diagonals
                .is_empty(),
            "{content}"
        );
    }
}

/// 対角線を水平・垂直の表推定graphicsへ足さず、端点の順序を保つ。
#[test]
fn directed_diagonals_remain_separate_from_axis_aligned_rules() {
    let p = document(
        &[".5 w 100 200 m 200 100 l S 20 30 m 80 30 l S"],
        "<< >>",
        "",
    );
    assert_eq!(p.source_diagonals.len(), 1);
    let d = p.source_diagonals[0];
    assert_eq!(
        (d.start, d.end, d.stroke_width),
        ([100., 200.], [200., 100.], 0.5)
    );
    assert_eq!(p.graphics.len(), 1);
    assert_eq!(p.graphics[0].bounds.y0, p.graphics[0].bounds.y1);
}

/// q保存・一様拡縮・逆順の端点と、分割ContentsのSを保持する。
#[test]
fn source_state_and_direction_survive_contents_boundaries() {
    let p = document(
        &[
            "q 2 0 0 2 10 20 cm .5 w 200 100 m 100 200 l ",
            "S Q 100 200 m 200 100 l S",
        ],
        "<< >>",
        "",
    );
    assert_eq!(p.source_diagonals.len(), 2);
    assert_eq!(
        (
            p.source_diagonals[0].start,
            p.source_diagonals[0].end,
            p.source_diagonals[0].stroke_width
        ),
        ([410., 220.], [210., 420.], 1.)
    );
    assert_eq!(
        (
            p.source_diagonals[1].start,
            p.source_diagonals[1].end,
            p.source_diagonals[1].stroke_width
        ),
        ([100., 200.], [200., 100.], 1.)
    );
}

/// 白・破線・透明・mask・blend・色空間置換から黒い実線を推測しない。
#[test]
fn uncertain_or_nonblack_paint_is_not_a_black_source_diagonal() {
    for (prefix, resources) in [
        ("1 G", "<< >>"),
        ("1 0 0 RG", "<< >>"),
        ("[2 2] 0 d", "<< >>"),
        ("/Unknown gs", "<< >>"),
        ("/Half gs", "<< /ExtGState << /Half << /CA .5 >> >> >>"),
        (
            "/Mask gs",
            "<< /ExtGState << /Mask << /SMask << /S /Alpha >> >> >> >>",
        ),
        (
            "/Blend gs",
            "<< /ExtGState << /Blend << /BM /Multiply >> >> >>",
        ),
        ("0 G", "<< /ColorSpace << /DefaultGray /DeviceGray >> >>"),
        ("2 0 0 3 0 0 cm", "<< >>"),
    ] {
        assert!(
            document(&[&format!("{prefix} 100 120 m 140 100 l S")], resources, "")
                .source_diagonals
                .is_empty(),
            "{prefix}: {resources}"
        );
    }
    let p = document(
        &["/Ink gs 0 0 0 1 K .5 w 100 200 m 200 100 l S"],
        "<< /ExtGState << /Ink << /CA 1 /ca 0 /BM /Normal /SMask /None /OP true /OPM 1 >> >> >>",
        "",
    );
    assert_eq!(p.source_diagonals.len(), 1);
}

/// clipに部分的に隠れた線を補完せず、SとWの適用順を守る。
#[test]
fn only_fully_clipped_in_source_strokes_are_kept() {
    for (content, count) in [
        ("q 50 50 200 200 re W n 100 200 m 200 100 l S Q", 1),
        ("q 50 50 100 200 re W n 100 200 m 200 100 l S Q", 0),
        ("q 300 300 100 100 re W n 100 200 m 200 100 l S Q", 0),
        ("100 200 m 200 100 l W S", 1),
        ("-10 100 m 100 200 l S", 0),
    ] {
        assert_eq!(
            document(&[content], "<< >>", "").source_diagonals.len(),
            count,
            "{content}"
        );
    }
}

/// 後続の白い覆いを再表示せず、先に描いた塗りと無関係な塗りは区別する。
#[test]
fn later_paint_removes_only_potentially_occluded_strokes() {
    for (content, count) in [
        ("100 200 m 200 100 l S 1 g 90 90 120 120 re f", 0),
        ("100 200 m 200 100 l S 1 g 300 300 20 20 re f", 1),
        ("1 g 90 90 120 120 re f 100 200 m 200 100 l S", 1),
        ("100 200 m 200 100 l S 90 90 120 120 re n", 1),
        ("100 200 m 200 100 l S /Unknown Do", 0),
        ("100 200 m 200 100 l S /OC /Layer BDC EMC", 0),
    ] {
        assert_eq!(
            document(&[content], "<< >>", "").source_diagonals.len(),
            count,
            "{content}"
        );
    }
}

/// crop原点と90/180/270度で、外接矩形へ潰さず二端点を表示座標へ移す。
#[test]
fn viewport_rotation_preserves_directed_endpoints() {
    for (rotation, start, end) in [
        (0, [50., 150.], [150., 50.]),
        (90, [150., 350.], [50., 250.]),
        (180, [350., 250.], [250., 350.]),
        (270, [250., 50.], [350., 150.]),
    ] {
        let p = document(
            &["100 200 m 200 100 l S"],
            "<< >>",
            &format!("/CropBox [50 50 450 450] /Rotate {rotation}"),
        );
        assert_eq!(p.source_diagonals.len(), 1);
        assert_eq!(
            (p.source_diagonals[0].start, p.source_diagonals[0].end),
            (start, end)
        );
    }
}

/// 文字列・配列の偽命令や未strokeのpathは対角線へ変えない。
#[test]
fn lexical_and_unpainted_paths_do_not_invent_source_lines() {
    for content in [
        "(100 200 m 200 100 l S) Tj",
        "[100 200 m 200 100 l S] TJ",
        "100 200 m 200 100 l n",
        "100 200 m 200 100 l f",
        "100 200 m 130 180 150 120 200 100 c S",
    ] {
        assert!(
            document(&[content], "<< >>", "")
                .source_diagonals
                .is_empty(),
            "{content}"
        );
    }
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 移動・一様拡縮・端点逆順でも向きと線幅を同じ原本へ結び付ける。
    #[test]
    fn source_diagonal_geometry_follows_uniform_transforms(dx in 0_f32..20.,dy in 0_f32..20.,scale in 0.5_f32..1.5,reverse in any::<bool>()) {
        let (a,b)=if reverse{([200.,100.],[100.,200.])}else{([100.,200.],[200.,100.])};
        let c=format!("q {scale} 0 0 {scale} {dx} {dy} cm .5 w {} {} m {} {} l S Q",a[0],a[1],b[0],b[1]);
        let p=document(&[&c],"<< >>","");prop_assert_eq!(p.source_diagonals.len(),1);
        let d=p.source_diagonals[0];
        for (actual,expected) in d.start.into_iter().chain(d.end).zip([a[0]*scale+dx,a[1]*scale+dy,b[0]*scale+dx,b[1]*scale+dy]) {prop_assert!((actual-expected).abs()<0.001);}
        prop_assert!((d.stroke_width-0.5*scale).abs()<0.001);
    }
}

/// セル外枠のclipは端のstrokeを切っても、セル全体を横断する原本中心線を失わせない。
#[test]
fn cell_boundary_clipping_preserves_the_original_diagonal_centerline() {
    for content in [
        "q 100 100 100 100 re W n .5 w 100 200 m 200 100 l S Q",
        "q 140.04 142.44 120.001 224.04 re W n .48 w 2 J q 1 0 0 1 260.04 366.48 cm 0 0 m -120 -224.04 l S Q Q",
    ] {
        assert_eq!(
            document(&[content], "<< >>", "").source_diagonals.len(),
            1,
            "{content}"
        );
    }
}

/// 原本と同じ不透明な黒い後続罫線は、黒い対角線を消す覆いとして扱わない。
#[test]
fn opaque_black_border_overpaint_does_not_erase_a_black_diagonal() {
    let p = document(
        &["100 200 m 200 100 l S 0 g 99.5 99.5 1 101 re f"],
        "<< >>",
        "",
    );
    assert_eq!(p.source_diagonals.len(), 1);
}
