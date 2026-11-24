//! 本文の空白計算と同じem高を、Wordの行boxにも明示する。
use docx_rs::{LineSpacing, LineSpacingType};

/// 有限・正でtwips表現可能なem高だけを採用し、段落前後の空白はそのまま保持する。
pub(super) fn spacing(height: f32, before: u32, after: u32) -> Option<LineSpacing> {
    let twips = (f64::from(height) * 20.0).round();
    if !twips.is_finite() || !(1.0..=f64::from(i32::MAX)).contains(&twips) {
        return None;
    }
    spacing_twips(twips as i32, before, after)
}

/// 累積末端から確定済みの正の整数行高を、再度pointへ戻して丸めずに出力する。
pub(super) fn spacing_twips(height: i32, before: u32, after: u32) -> Option<LineSpacing> {
    if height <= 0 {
        return None;
    }
    Some(
        LineSpacing::new()
            .before(before)
            .after(after)
            .line(height)
            .line_rule(LineSpacingType::Exact),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 無効値や表現不能なem高を、整数の飽和変換で有効な行boxにしない。
    #[test]
    fn invalid_height_retains_the_existing_fallback() {
        for height in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -1.0,
            0.0,
            0.01,
            f32::MAX,
        ] {
            assert!(spacing(height, 120, 240).is_none(), "{height}");
        }
        assert!(spacing(0.025, 120, 240).is_some());
        assert!(spacing(72.0, 120, 240).is_some());
    }
}
