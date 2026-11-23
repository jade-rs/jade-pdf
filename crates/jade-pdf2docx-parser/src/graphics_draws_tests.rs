//! 罫線抽出が、矩形のストロークや閉じたパスの辺も罫線として取り出すことを確認する。
use super::*;
use crate::{decode::DecodeBudget, limits::ParseLimits};

/// content stream から罫線を取り出し、丸めた座標で返す。
fn lines(content: &[u8]) -> Vec<(i32, i32, i32, i32)> {
    let budget = DecodeBudget::new(&ParseLimits::default());
    let mut output = Vec::new();
    append(&budget, 0, content, &mut output).expect("append");
    let mut lines = output
        .iter()
        .map(|graphic| {
            let b = graphic.bounds;
            (
                b.x0.round() as i32,
                b.y0.round() as i32,
                b.x1.round() as i32,
                b.y1.round() as i32,
            )
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines
}

/// `re S` で描いたセル枠は 4 辺の罫線になる。
#[test]
fn stroked_rectangle_becomes_four_rules() {
    assert_eq!(
        lines(b"0.5 w 10 20 30 40 re S"),
        vec![
            (10, 20, 10, 60),
            (10, 20, 40, 20),
            (10, 60, 40, 60),
            (40, 20, 40, 60)
        ]
    );
}

/// `h` で閉じたパスは、最後の点から始点へ戻る辺も罫線になる。
#[test]
fn closed_path_adds_closing_edge() {
    assert_eq!(
        lines(b"10 10 m 50 10 l 50 30 l 10 30 l h S"),
        vec![
            (10, 10, 10, 30),
            (10, 10, 50, 10),
            (10, 30, 50, 30),
            (50, 10, 50, 30)
        ]
    );
}

/// 塗りと線を同時に行う `B` も線として扱う。
#[test]
fn fill_and_stroke_draws_rules() {
    assert_eq!(lines(b"10 20 30 0.0 re B").len(), 2);
    assert_eq!(lines(b"10 10 m 50 10 l b").len(), 2);
}

/// 白以外の塗りつぶし矩形は塗り色つきで取り出し、白や細い矩形は除く。
#[test]
fn filled_rectangles_keep_their_color() {
    let budget = DecodeBudget::new(&ParseLimits::default());
    let mut output = Vec::new();
    let mut fills = Vec::new();
    append_with_fills(
        &budget,
        0,
        b"0.85 g 10 20 30 40 re f 1 1 1 rg 0 0 50 50 re f 1 0 0 rg 0 0 50 0.5 re f",
        &mut output,
        &mut fills,
    )
    .expect("append");
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].color, "D9D9D9");
    assert_eq!(output.len(), 1, "thin red rectangle stays a rule");
}

/// 正の dash array で描いた線は破線、空の dash array に戻した線と `q`/`Q` で戻した線は実線になる。
#[test]
fn dash_pattern_marks_rules_as_dashed() {
    let budget = DecodeBudget::new(&ParseLimits::default());
    let mut output = Vec::new();
    append(
        &budget,
        0,
        b"q [3.96 1.92] 0 d 0 0 m 100 0 l S Q 0 10 m 100 10 l S [3] 0 d 0 20 m 100 20 l S [] 0 d 0 30 m 100 30 l S",
        &mut output,
    )
    .expect("append");
    let dashed = output
        .iter()
        .map(|graphic| (graphic.bounds.y0.round() as i32, graphic.dashed))
        .collect::<Vec<_>>();
    assert_eq!(
        dashed,
        vec![(0, true), (10, false), (20, true), (30, false)]
    );
}
