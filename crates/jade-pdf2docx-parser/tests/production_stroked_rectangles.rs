//! 元の長方形の四辺を、可視性・描画状態・構築時CTMとともに保持する。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 任意の連続Contentsと継承Resourceを持つ正式な一ページPDFを生成する。
fn source(contents: &[&str], resources: &str) -> Vec<u8> {
    let refs = (0..contents.len())
        .map(|i| format!("{} 0 R", i + 4))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!(
            "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 500 500] /Resources {resources} >>"
        ),
        format!("<< /Type /Page /Parent 2 0 R /Contents [{refs}] >>"),
    ];
    objects.extend(contents.iter().map(|s| page_attribute_pdf::stream(s)));
    page_attribute_pdf::pdf(&objects)
}

/// 四辺の幾何を、公開APIが返す向きと順序に依存せず比較する。
fn edges(parsed: &ParsedDocument) -> Vec<[i32; 4]> {
    let mut result = parsed
        .graphics
        .iter()
        .map(|line| {
            [
                line.bounds.x0 as i32,
                line.bounds.y0 as i32,
                line.bounds.x1 as i32,
                line.bounds.y1 as i32,
            ]
        })
        .collect::<Vec<_>>();
    result.sort();
    result
}

/// re Sを塗りつぶしの中心線へ縮めず、元の黒枠の四辺へ戻す。
#[test]
fn black_stroked_rectangle_preserves_all_four_source_edges() {
    let parsed = parse_pdf_bytes(
        "owned-rectangle.pdf",
        &source(&["0 0 0 1 K .25 w 20 30 40 10 re S"], "<< >>"),
    )
    .unwrap();
    assert_eq!(
        edges(&parsed),
        vec![
            [20, 30, 20, 40],
            [20, 30, 60, 30],
            [20, 40, 60, 40],
            [60, 30, 60, 40]
        ],
        "stroked rectangle must retain its four original edges"
    );
    assert!(parsed.graphics.iter().all(|line| line.stroke_width == 0.25));
}

/// 別ContentsにあるSでも、前のCTM・q保存と構築済みpathを引き継ぐ。
#[test]
fn rectangle_path_and_state_cross_contents_boundaries() {
    let parsed = parse_pdf_bytes(
        "owned-split-rectangle.pdf",
        &source(&["q 2 0 0 3 10 20 cm 1 w 20 30 40 10 re ", "S Q"], "<< >>"),
    )
    .unwrap();
    assert_eq!(
        edges(&parsed),
        vec![
            [50, 110, 50, 140],
            [50, 110, 130, 110],
            [50, 140, 130, 140],
            [130, 110, 130, 140]
        ]
    );
    for line in parsed.graphics {
        assert_eq!(
            line.stroke_width,
            if line.bounds.x0 == line.bounds.x1 {
                2.
            } else {
                3.
            }
        );
    }
}

/// 不透明なCMYK黒の重ね刷りは同じ四辺を保持し、fill側のalphaと混同しない。
#[test]
fn resolved_opaque_stroke_state_is_distinct_from_fill_opacity() {
    let resources = "<< /ExtGState << /Ink << /Type /ExtGState /CA 1 /ca 0 /BM /Normal /SMask /None /AIS false /OP true /OPM 1 /SA true >> >> >>";
    let parsed = parse_pdf_bytes(
        "owned-stroke-state.pdf",
        &source(&["/Ink gs 0 0 0 1 K .25 w 20 30 40 10 re S"], resources),
    )
    .unwrap();
    assert_eq!(
        parsed.graphics.len(),
        4,
        "opaque black stroke state must preserve rectangle edges"
    );
}

/// Wの確定前にSで描いた枠は元のclipで描き、clip境界自体を追加罫線にしない。
#[test]
fn clipping_is_applied_at_paint_time_without_inventing_a_closing_edge() {
    let parsed = parse_pdf_bytes(
        "owned-rectangle-clip.pdf",
        &source(&["q 0 0 45 100 re W n 20 30 40 10 re S Q"], "<< >>"),
    )
    .unwrap();
    assert_eq!(
        edges(&parsed),
        vec![[20, 30, 20, 40]],
        "only the fully visible original side may remain; do not close at x45"
    );
    let parsed = parse_pdf_bytes(
        "owned-paint-then-clip.pdf",
        &source(&["20 30 40 10 re W S"], "<< >>"),
    )
    .unwrap();
    assert_eq!(
        parsed.graphics.len(),
        4,
        "W takes effect after painting the current path"
    );
}

/// 取消・塗りのみ・斜交・隠れた色・未解決描画状態から枠を発明しない。
#[test]
fn unpainted_hidden_or_uncertain_rectangles_do_not_become_rules() {
    for (content, resources) in [
        ("20 30 40 10 re n", "<< >>"),
        ("20 30 40 10 re f", "<< >>"),
        ("q 1 .2 0 1 0 0 cm 20 30 40 10 re S Q", "<< >>"),
        ("1 G 20 30 40 10 re S", "<< >>"),
        ("[2 2] 0 d 20 30 40 10 re S", "<< >>"),
        ("/Unknown gs 20 30 40 10 re S", "<< >>"),
        (
            "/Half gs 20 30 40 10 re S",
            "<< /ExtGState << /Half << /CA .5 >> >> >>",
        ),
        (
            "/Masked gs 20 30 40 10 re S",
            "<< /ExtGState << /Masked << /SMask << /S /Alpha >> >> >> >>",
        ),
        ("q 100 100 20 20 re W n 20 30 40 10 re S Q", "<< >>"),
        (
            "(20 30 40 10 re S) Tj [20 30 40 10 re S] TJ /re /S",
            "<< >>",
        ),
    ] {
        let parsed = parse_pdf_bytes(
            "owned-uncertain-rectangle.pdf",
            &source(&[content], resources),
        )
        .unwrap();
        assert!(
            parsed.graphics.is_empty(),
            "{content}: {:?}",
            parsed.graphics
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 正立の任意の平行移動・非等方拡大でも、四辺と法線方向の線幅を保持する。
    #[test]
    fn authored_rectangle_edges_transform_without_changing_ownership(x in 10_i16..80,y in 10_i16..80,w in 10_i16..60,h in 10_i16..60,sx in 1_i16..4,sy in 1_i16..4,tx in 0_i16..20,ty in 0_i16..20) {
        let content=format!("{sx} 0 0 {sy} {tx} {ty} cm .5 w {x} {y} {w} {h} re S");
        let parsed=parse_pdf_bytes("owned-affine-rectangle.pdf",&source(&[&content],"<< >>")).unwrap();
        let (l,b,r,t)=(i32::from(x*sx+tx),i32::from(y*sy+ty),i32::from((x+w)*sx+tx),i32::from((y+h)*sy+ty));
        prop_assert_eq!(edges(&parsed),vec![[l,b,l,t],[l,b,r,b],[l,t,r,t],[r,b,r,t]]);
        for line in parsed.graphics {prop_assert_eq!(line.stroke_width,0.5*f32::from(if line.bounds.x0==line.bounds.x1 {sx}else{sy}));}
    }
}

/// 未完の文字列・配列の回復処理から、実行していないre/Sを発明しない。
#[test]
fn malformed_literal_contents_cannot_create_rectangle_rules() {
    for content in [
        "(20 30 40 10 re S",
        "<ZZ 20 30 40 10 re S",
        "[ BT 20 30 40 10 re S",
    ] {
        let parsed = parse_pdf_bytes(
            "owned-malformed-rectangle.pdf",
            &source(&[content], "<< >>"),
        )
        .unwrap();
        assert!(
            parsed.graphics.is_empty(),
            "malformed literal must not invent a stroked rectangle: {content}: {:?}",
            parsed.graphics
        );
    }
}

/// Device色空間の選択で初期色へ戻り、直前の白色を引き継がない。
#[test]
fn selecting_device_color_space_restores_its_initial_black() {
    for name in ["DeviceGray", "DeviceRGB", "DeviceCMYK"] {
        let content = format!("1 G /{name} CS 20 30 40 10 re S");
        let parsed = parse_pdf_bytes(
            "owned-device-initial-color.pdf",
            &source(&[&content], "<< >>"),
        )
        .unwrap();
        assert_eq!(
            parsed.graphics.len(),
            4,
            "known Device color space must restore initial black: {name}"
        );
    }
}

/// Device色の既定置換があるページでは、未確認のCIE黒点をWord黒へ決めつけない。
#[test]
fn substituted_device_space_does_not_invent_black_borders() {
    let resources = "<< /ColorSpace << /DefaultGray [/CalGray << /WhitePoint [1 1 1] /BlackPoint [.2 .1 .1] >>] >> >>";
    let parsed = parse_pdf_bytes(
        "owned-substituted-device.pdf",
        &source(&["0 G 20 30 40 10 re S"], resources),
    )
    .unwrap();
    assert!(
        parsed.graphics.is_empty(),
        "unverified DefaultGray substitution must not assert black rectangle borders"
    );
}
