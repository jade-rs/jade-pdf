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
