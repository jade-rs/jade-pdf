//! 公開SDKで確認できる、不透明で細い矩形塗りだけを罫線の候補にする。
use super::geometry::Line;
use pdfium_render::prelude::Pdfium;
use std::{ffi::c_void, mem::MaybeUninit};

/// 公開APIの出力引数の型を推論し、SDK成功時だけ初期化済みとして読む。
fn slot<T, R>(_: &impl Fn(*mut T) -> R) -> MaybeUninit<T> {
    MaybeUninit::uninit()
}

/// 平坦なページから候補を読む。上限超過やフォームがある場合は部分結果を返さない。
pub(super) fn read(pdfium: &Pdfium, page: *mut c_void, point: f64) -> Option<Vec<Line>> {
    let b = pdfium.bindings();
    let page = page.cast();
    if !point.is_finite() || point <= 0.0 {
        return None;
    }
    let count = b.FPDFPage_CountObjects(page);
    if !(0..=20_000).contains(&count) {
        return None;
    }
    let mut result = Vec::new();
    for index in 0..count {
        let object = b.FPDFPage_GetObject(page, index);
        if object.is_null() {
            return None;
        }
        match b.FPDFPageObj_GetType(object) {
            5 => return None, // フォームの継承状態を部分的に推定しない。
            2 => (),
            _ => continue,
        }
        if b.FPDFPageObj_HasTransparency(object) != 0 {
            continue;
        }
        let marks = b.FPDFPageObj_CountMarks(object);
        if !(0..=1).contains(&marks) {
            continue;
        }
        if marks == 1 {
            let mark = b.FPDFPageObj_GetMark(object, 0);
            let mut name = [0u16; 16];
            let mut length = 0;
            if mark.is_null()
                || b.FPDFPageObjMark_CountParams(mark) != 0
                || b.FPDFPageObjMark_GetName(mark, name.as_mut_ptr().cast(), 32, &mut length) == 0
                || length != 18
                || name[..9] != [65, 114, 116, 105, 102, 97, 99, 116, 0]
            {
                continue;
            }
        }
        let clip = b.FPDFPageObj_GetClipPath(object);
        // 固定SDK8111と自作PDFで検証した「参照なし」だけを受け入れる。
        // count=0でも文字クリップの可能性があり、未確認の状態は通さない。
        if clip.is_null() || b.FPDFClipPath_CountPaths(clip) != -1 {
            continue;
        }
        let (mut fill, mut stroke) = (0, 0);
        let (mut r, mut g, mut blue, mut alpha) = (0, 0, 0, 0);
        if b.FPDFPath_GetDrawMode(object, &mut fill, &mut stroke) == 0
            || fill == 0
            || stroke != 0
            || b.FPDFPageObj_GetFillColor(object, &mut r, &mut g, &mut blue, &mut alpha) == 0
            || alpha != 255
            || r.max(g).max(blue) > 64
        {
            continue;
        }
        let count = b.FPDFPath_CountSegments(object);
        if !(4..=5).contains(&count) {
            continue;
        }
        let get_matrix = |p| b.FPDFPageObj_GetMatrix(object, p);
        let mut matrix = slot(&get_matrix);
        if get_matrix(matrix.as_mut_ptr()) == 0 {
            continue;
        }
        // SAFETY: 成功した公開APIがFS_MATRIXを初期化した。
        let m = unsafe { matrix.assume_init() };
        let mut points = Vec::new();
        let mut closed = false;
        for i in 0..count {
            let segment = b.FPDFPath_GetPathSegment(object, i);
            let (mut x, mut y) = (0.0, 0.0);
            if segment.is_null()
                || b.FPDFPathSegment_GetType(segment) != if i == 0 { 2 } else { 0 }
                || b.FPDFPathSegment_GetPoint(segment, &mut x, &mut y) == 0
                || closed
            {
                points.clear();
                break;
            }
            points.push((
                f64::from(m.a) * f64::from(x) + f64::from(m.c) * f64::from(y) + f64::from(m.e),
                f64::from(m.b) * f64::from(x) + f64::from(m.d) * f64::from(y) + f64::from(m.f),
            ));
            closed = b.FPDFPathSegment_GetClose(segment) != 0;
        }
        if !closed || points.len() != count as usize {
            continue;
        }
        if points.len() == 5 {
            if points.first() != points.last() {
                continue;
            }
            points.pop();
        }
        if points
            .iter()
            .any(|&(x, y)| !x.is_finite() || !y.is_finite())
            || points
                .iter()
                .enumerate()
                .any(|(i, p)| points[..i].contains(p))
        {
            continue;
        }
        let (mut xs, mut ys): (Vec<_>, Vec<_>) = points.iter().copied().unzip();
        xs.sort_by(f64::total_cmp);
        ys.sort_by(f64::total_cmp);
        xs.dedup();
        ys.dedup();
        if xs.len() != 2
            || ys.len() != 2
            || points.iter().enumerate().any(|(i, &p)| {
                let next = points[(i + 1) % 4];
                (p.0 == next.0) == (p.1 == next.1)
            })
        {
            continue;
        }
        let (width, height) = (xs[1] - xs[0], ys[1] - ys[0]);
        let horizontal = width >= 20.0 * point && height <= 2.0 * point;
        let vertical = height >= 20.0 * point && width <= 2.0 * point;
        if horizontal == vertical || width.min(height) <= 0.0 {
            continue;
        }
        if result.len() == 2048 {
            return None;
        }
        result.push(Line {
            horizontal,
            coordinate: if horizontal {
                (ys[0] + ys[1]) / 2.0
            } else {
                (xs[0] + xs[1]) / 2.0
            } / point,
            start: if horizontal { xs[0] } else { ys[0] } / point,
            end: if horizontal { xs[1] } else { ys[1] } / point,
            thickness: width.min(height) / point,
        });
    }
    Some(result)
}
