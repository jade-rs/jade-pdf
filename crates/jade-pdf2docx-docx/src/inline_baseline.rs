//! 同じ行の小さい文字について、確認済みのbaseline差だけを半point位置へ保存する。
use super::{ConvertError, ParsedTextElement};
use docx_rs::XMLDocx;
use quick_xml::{
    Reader, Writer,
    events::{BytesStart, Event},
};

/// 梱包前だけ使うrun style名。最終OOXMLではw:positionへ置換する。
const MARKER: &str = "JadeBaseline_";

/// 最大fontのbaselineが一意な行だけ、周囲の通常文字の基準を返す。
fn reference(row: &[&ParsedTextElement]) -> Option<(f32, f32)> {
    let main = row
        .iter()
        .filter(|text| {
            text.font_size.is_finite()
                && text.font_size > 0.
                && text.baseline_y.is_some_and(|y| y.is_finite())
        })
        .max_by(|a, b| a.font_size.total_cmp(&b.font_size))?;
    let y = main.baseline_y?;
    if row.iter().any(|text| {
        text.font_size >= main.font_size * 0.95
            && text
                .baseline_y
                .is_none_or(|other| !other.is_finite() || (other - y).abs() > 0.25)
    }) {
        return None;
    }
    Some((main.font_size, y))
}

/// 小さい文字だけに元の相対位置を付け、未知の基線・極端な差・font縮小の推測を避ける。
fn offset(text: &ParsedTextElement, reference: Option<(f32, f32)>) -> Option<i32> {
    let (size, baseline) = reference?;
    let y = text.baseline_y.filter(|y| y.is_finite())?;
    let ratio = text.font_size / size;
    let delta = y - baseline;
    let half_points = (f64::from(delta) * 2.).round();
    (ratio.is_finite()
        && (0.5..=0.85).contains(&ratio)
        && delta.abs() >= 0.25
        && delta.abs() <= size * 0.6
        && half_points.is_finite()
        && (-31680. ..=31680.).contains(&half_points)
        && half_points != 0.)
        .then_some(half_points as i32)
}

/// 行全体が曖昧な場合は左右直近の通常文字を照合し、二回の走査で各runの位置を求める。
pub(super) fn offsets(row: &[&ParsedTextElement]) -> Vec<Option<i32>> {
    if let Some(baseline) = reference(row) {
        return row
            .iter()
            .map(|text| offset(text, Some(baseline)))
            .collect();
    }
    let mut result = vec![None; row.len()];
    if row
        .iter()
        .any(|text| !text.font_size.is_finite() || text.font_size <= 0.)
    {
        return result;
    }
    let size = row.iter().map(|text| text.font_size).fold(0., f32::max);
    let dominant = |text: &ParsedTextElement| text.font_size >= size * 0.95;
    let mut previous = None;
    let left = row
        .iter()
        .map(|text| {
            let anchor = previous;
            if dominant(text) {
                previous = Some(*text);
            }
            anchor
        })
        .collect::<Vec<_>>();
    let mut next = None;
    for (index, text) in row.iter().enumerate().rev() {
        result[index] = offset(text, neighbouring_reference(size, left[index], next));
        if dominant(text) {
            next = Some(*text);
        }
    }
    result
}

/// 隣接する通常文字が双方とも既知かつ同じ基線の場合だけ、局所的な基準を返す。
fn neighbouring_reference(
    size: f32,
    left: Option<&ParsedTextElement>,
    right: Option<&ParsedTextElement>,
) -> Option<(f32, f32)> {
    let anchor = left.or(right)?;
    let baseline = anchor.baseline_y.filter(|y| y.is_finite())?;
    if let (Some(_), Some(right)) = (left, right) {
        let other = right.baseline_y.filter(|y| y.is_finite())?;
        if (baseline - other).abs() > 0.25 {
            return None;
        }
    }
    Some((size, baseline))
}

/// privateな測定値だけから、梱包時の変換対象を表すstyle名を作る。
pub(super) fn marker(offset: i32) -> String {
    format!("{MARKER}{offset}")
}

/// 本文と各header/footerへ同じ位置指定を適用し、その後のページ縮小にも委ねる。
pub(super) fn apply(package: &mut XMLDocx) -> Result<(), ConvertError> {
    for part in std::iter::once(&mut package.document)
        .chain(package.headers.iter_mut())
        .chain(package.footers.iter_mut())
    {
        replace(part)?;
    }
    Ok(())
}

/// 検査・XML構築の失敗を、部分DOCXを公開しない梱包エラーへ変える。
fn error(message: impl std::fmt::Display) -> ConvertError {
    ConvertError::Pack(format!("inline baseline: {message}"))
}

/// 自前生成したrPr内のmarkerだけを変換し、文字列・font size・他のstyleは保持する。
fn replace(part: &mut Vec<u8>) -> Result<(), ConvertError> {
    if !part
        .windows(MARKER.len())
        .any(|bytes| bytes == MARKER.as_bytes())
    {
        return Ok(());
    }
    let mut reader = Reader::from_reader(part.as_slice());
    let mut writer = Writer::new(Vec::with_capacity(part.len()));
    let mut parents: Vec<Vec<u8>> = Vec::new();
    loop {
        let event = reader.read_event().map_err(error)?;
        match event {
            Event::Empty(ref element) if element.name().as_ref() == b"w:rStyle" => {
                let value = element.try_get_attribute(b"w:val").map_err(error)?;
                if let Some(value) = value
                    && let Some(value) = value.value.strip_prefix(MARKER.as_bytes())
                {
                    if parents.last().map(Vec::as_slice) != Some(b"w:rPr") {
                        return Err(error("marker outside run properties"));
                    }
                    let offset: i32 = std::str::from_utf8(value)
                        .map_err(error)?
                        .parse()
                        .map_err(error)?;
                    if offset == 0 || !(-31680..=31680).contains(&offset) {
                        return Err(error("invalid baseline offset"));
                    }
                    let value = offset.to_string();
                    let mut position = BytesStart::new("w:position");
                    position.push_attribute(("w:val", value.as_str()));
                    writer.write_event(Event::Empty(position)).map_err(error)?;
                    continue;
                }
            }
            Event::Start(ref element) => parents.push(element.name().as_ref().to_vec()),
            Event::End(ref element) => {
                if parents.pop().as_deref() != Some(element.name().as_ref()) {
                    return Err(error("unbalanced run XML"));
                }
            }
            Event::Eof => {
                if !parents.is_empty() {
                    return Err(error("unclosed run XML"));
                }
                break;
            }
            _ => {}
        }
        writer.write_event(event).map_err(error)?;
    }
    *part = writer.into_inner();
    Ok(())
}
