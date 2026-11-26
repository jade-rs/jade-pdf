//! 確認済みの軸平行矩形clipだけを、背景塗りの可視範囲へ適用する。
use crate::ParsedBounds;

/// 未制限・確認済み矩形・交差のない空領域を区別する。
#[derive(Clone, Copy, Default)]
pub(super) enum Clip {
    /// まだclipを設定していない状態。
    #[default]
    Unbounded,
    /// 原本座標で確認した軸平行矩形。
    Bounds {
        /// 実際に描画される範囲。
        bounds: ParsedBounds,
    },
    /// 有効な矩形同士が交差せず、塗りが残らない状態。
    Empty,
}

impl Clip {
    /// 新しい矩形clipを現在のclipと交差させる。
    pub(super) fn intersect(self, next: ParsedBounds) -> Self {
        self.apply(next)
            .map_or(Self::Empty, |bounds| Self::Bounds { bounds })
    }

    /// 一様な矩形塗りが実際に残る範囲だけを返す。
    pub(super) fn apply(self, fill: ParsedBounds) -> Option<ParsedBounds> {
        match self {
            Self::Unbounded => Some(fill),
            Self::Empty => None,
            Self::Bounds { bounds } => {
                let visible = ParsedBounds {
                    x0: fill.x0.max(bounds.x0),
                    y0: fill.y0.max(bounds.y0),
                    x1: fill.x1.min(bounds.x1),
                    y1: fill.y1.min(bounds.y1),
                };
                (visible.x0 < visible.x1 && visible.y0 < visible.y1).then_some(visible)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 有限で正の矩形を、追加の座標推測なしで構築する。
    fn bounds(x: i16, y: i16, w: u8, h: u8) -> ParsedBounds {
        ParsedBounds {
            x0: f32::from(x),
            y0: f32::from(y),
            x1: f32::from(x) + f32::from(w),
            y1: f32::from(y) + f32::from(h),
        }
    }

    /// 空clipを未制限のclipへ取り違えず、後続のclipでも領域を復活させない。
    #[test]
    fn an_empty_intersection_never_reopens_the_clip() {
        let first = bounds(0, 0, 10, 10);
        let other = bounds(20, 20, 10, 10);
        let saved = Clip::Unbounded.intersect(first);
        let empty = saved.intersect(other);
        assert_eq!(empty.apply(first), None);
        assert_eq!(empty.intersect(first).apply(first), None);
        assert_eq!(saved.apply(first), Some(first));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 矩形clipの順序と反復によらず、塗りは共通領域だけへ縮み、領域を増やさない。
        #[test]
        fn nested_rectangles_are_commutative_and_idempotent(
            ax in -100_i16..=100,ay in -100_i16..=100,aw in 1_u8..=50,ah in 1_u8..=50,
            bx in -100_i16..=100,by in -100_i16..=100,bw in 1_u8..=50,bh in 1_u8..=50,
        ) {
            let a=bounds(ax,ay,aw,ah);let b=bounds(bx,by,bw,bh);
            let fill=bounds(-150,-150,255,255);
            let once=Clip::Unbounded.intersect(a).intersect(b).apply(fill);
            prop_assert_eq!(once,Clip::Unbounded.intersect(b).intersect(a).apply(fill));
            prop_assert_eq!(once,Clip::Unbounded.intersect(a).intersect(b).intersect(a).apply(fill));
            if let Some(v)=once {
                prop_assert!(v.x0>=a.x0.max(b.x0) && v.x1<=a.x1.min(b.x1));
                prop_assert!(v.y0>=a.y0.max(b.y0) && v.y1<=a.y1.min(b.y1));
            }
        }
    }
}
