//! 白い細い塗りを罫線から除外する前に、所属ページの色変換と合成を検証する。
use super::stroked_rectangles::{drawing_operand, joined, resources};
use crate::{
    decode::DecodeBudget,
    metadata, metadata_names,
    resource_resolver::ResourceResolver,
    syntax::{Lexer, Token},
    text_draws::Operand,
};

/// 未対応の効果を含まない、完全かつ一意なExtGStateだけを許可する。
fn normal_state<'a>(
    resolver: &mut ResourceResolver<'_, 'a>,
    raw: &'a [u8],
    require_stroke: bool,
) -> Option<()> {
    let (state, _) = resolver.resolve(raw)?;
    if !metadata::dictionary_entries_complete(state) {
        return None;
    }
    let keys = [
        "Type", "CA", "ca", "BM", "SMask", "AIS", "OP", "op", "OPM", "SA", "TK", "BG2", "UCR2",
        "SM",
    ];
    let mut seen = 0_u16;
    for (key, raw) in metadata::entries(state)? {
        let index = keys
            .iter()
            .position(|name| metadata_names::resource_name_order(key, name).is_eq())?;
        if seen & (1 << index) != 0 {
            return None;
        }
        seen |= 1 << index;
        let (value, _) = resolver.resolve(raw)?;
        let keyword = || crate::parser::image_draws::resources::keyword(value);
        let number = || {
            keyword()
                .and_then(crate::page_attributes::number_value)
                .filter(|v| v.is_finite())
        };
        let valid = match index {
            0 => metadata::is_name(value, b"ExtGState"),
            1 => {
                number().is_some_and(|n| (0.0..=1.0).contains(&n) && (!require_stroke || n == 1.0))
            }
            2 => number() == Some(1.0),
            3 => metadata::is_name(value, b"Normal"),
            4 => metadata::is_name(value, b"None"),
            5 | 6 | 7 | 9 | 10 => matches!(keyword(), Some(b"true" | b"false")),
            8 => matches!(number(), Some(0.0 | 1.0)),
            11 | 12 => metadata::is_name(value, b"Default"),
            13 => number().is_some_and(|n| (0.0..=1.0).contains(&n)),
            _ => false,
        };
        if !valid {
            return None;
        }
    }
    Some(())
}

/// 完全なResourcesの対象キーを一意にし、Device色の既定置換を拒否する。
fn page_states<'a>(
    budget: &DecodeBudget,
    resolver: &mut ResourceResolver<'_, 'a>,
    resource: &'a [u8],
    require_stroke: bool,
) -> Option<Vec<&'a [u8]>> {
    if !metadata::dictionary_entries_complete(resource) {
        return None;
    }
    let mut seen = 0_u8;
    for (key, _) in metadata::entries(resource)? {
        for (index, name) in ["ColorSpace", "ExtGState"].iter().enumerate() {
            if metadata_names::resource_name_order(key, name).is_eq() {
                if seen & (1 << index) != 0 {
                    return None;
                }
                seen |= 1 << index;
                // 別綴りをmetadata::valueが見落とす場合は白と断定しない。
                if key != name.as_bytes() {
                    return None;
                }
            }
        }
    }
    if !(1..=4)
        .filter(|n| *n != 2)
        .all(|n| resources::native_device(resolver, Some(resource), Some(n)))
    {
        return None;
    }
    let Some(raw) = metadata::value(resource, b"ExtGState") else {
        return Some(Vec::new());
    };
    let (states, _) = resolver.resolve(raw)?;
    if !metadata::dictionary_entries_complete(states) {
        return None;
    }
    let mut names = budget.metadata_vector(metadata::entries(states)?.count())?;
    for (name, raw) in metadata::entries(states)? {
        if budget.is_stopped() || name.contains(&b'#') {
            return None;
        }
        normal_state(resolver, raw, require_stroke)?;
        names.push(name);
    }
    names.sort_unstable();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return None;
    }
    Some(names)
}

/// 配列・文字列内の偽命令を除き、全gsとDevice色のoperandが確認できることを検査する。
fn commands(
    budget: &DecodeBudget,
    content: &[u8],
    states: &[&[u8]],
    collect_white: bool,
) -> Option<Vec<usize>> {
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    let mut args = [Operand::Other; 6];
    let mut count = 0;
    let mut saved = Vec::new();
    let mut white = false;
    let mut positions = Vec::new();
    while let Some((token, span)) = lexer.next_spanned() {
        if budget.is_stopped() {
            return None;
        }
        let Token::Word(word) = token else {
            let Some(value) = drawing_operand(&mut lexer, token, span.start) else {
                return None;
            };
            if count == args.len() {
                return None;
            }
            args[count] = value;
            count += 1;
            continue;
        };
        if let Some(number) = crate::page_attributes::number_value(word) {
            if count == args.len() || !number.is_finite() {
                return None;
            }
            args[count] = Operand::Number(number);
            count += 1;
            continue;
        }
        let operands = &args[..count];
        match (word, operands) {
            (b"gs", [Operand::Name(name)])
                if !name.contains(&b'#') && states.binary_search(name).is_ok() => {}
            (b"gs" | b"BI", _) => return None,
            (b"q", []) => crate::metadata_workspace::try_push(budget, &mut saved, white)?,
            (b"Q", []) if !saved.is_empty() => white = saved.pop()?,
            (b"q" | b"Q", _) => return None,
            (b"g" | b"rg" | b"k", _) => {
                let expected = match word {
                    b"g" => 1,
                    b"rg" => 3,
                    _ => 4,
                };
                if count != expected || !operands.iter().all(|v| matches!(v, Operand::Number(_))) {
                    return None;
                }
                white = operands.iter().all(|v| match v {
                    Operand::Number(n) if word == b"k" => *n <= 0.0,
                    Operand::Number(n) => *n >= 1.0,
                    _ => false,
                });
            }
            (b"cs" | b"sc" | b"scn", _) => white = false,
            (b"Tr", [Operand::Number(value)]) if !collect_white => {
                if !value.is_finite() || !(0.0..=7.0).contains(value) || value.fract() != 0.0 {
                    return None;
                }
            }
            (b"w", [Operand::Number(value)]) if !collect_white => {
                if !value.is_finite() || *value < 0.0 {
                    return None;
                }
            }
            (b"Tr" | b"w", _) if !collect_white => return None,
            (b"f" | b"f*", []) if white && collect_white => {
                crate::metadata_workspace::try_push(budget, &mut positions, lexer.cursor)?
            }
            _ => {}
        }
        count = 0;
    }
    (count == 0 && saved.is_empty()).then_some(positions)
}

/// 全Contents境界と継承Resourceを照合し、不明な描画は従来の罫線経路に残す。
pub(super) fn positions(
    budget: &DecodeBudget,
    source: &[u8],
    page: usize,
    streams: &[(usize, Vec<u8>)],
) -> Option<Vec<usize>> {
    validated(budget, source, page, streams, false, true)
}

/// 非strokeだけでなくstrokeも不透明な所属ページだけを、文字強調の根拠にする。
pub(super) fn normal_text_paint(
    budget: &DecodeBudget,
    source: &[u8],
    page: usize,
    content: &[u8],
) -> bool {
    validated(budget, source, page, &[(page, content)], true, false).is_some()
}

/// 内容とResourceを同じ所属ページで照合する、二種類のpaint検証の共通処理。
fn validated<T: AsRef<[u8]>>(
    budget: &DecodeBudget,
    source: &[u8],
    page: usize,
    streams: &[(usize, T)],
    require_stroke: bool,
    collect_white: bool,
) -> Option<Vec<usize>> {
    let Some(start) = streams.iter().position(|(p, _)| *p == page) else {
        return None;
    };
    let Some((_, content, end)) = joined(budget, streams, start) else {
        return None;
    };
    if streams[end..].iter().any(|(p, _)| *p == page) {
        return None;
    }
    let Some(objects) = crate::object_index::build(budget, source) else {
        return None;
    };
    let mut resolver = ResourceResolver::new(&objects, budget);
    let resource = if let Some(pages) = budget.page_resources(source) {
        pages.get(page).copied().flatten()
    } else {
        budget
            .page_dictionaries(source)
            .get(page)
            .and_then(|raw| resources::inherited(budget, &mut resolver, raw))
            .flatten()
    };
    let Some(resource) = resource else {
        return None;
    };
    let Some(states) = page_states(budget, &mut resolver, resource, require_stroke) else {
        return None;
    };
    commands(budget, &content, &states, collect_white)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 確認済み位置の全slotをinclusiveに予約し、不足時は部分証明を返さない。
    #[test]
    fn white_position_workspace_is_inclusive_and_sticky() {
        let total = 4 * size_of::<usize>() as u64;
        for limit in [total - 1, total] {
            let budget = DecodeBudget::new(&crate::ParseLimits {
                max_total_metadata_workspace_bytes: limit,
                ..Default::default()
            });
            let positions = commands(&budget, b"1 g f f*", &[], true);
            assert_eq!(positions.is_some(), limit == total);
            assert_eq!(budget.is_stopped(), limit != total);
            if limit == total {
                assert!(budget.check().is_ok());
                assert_eq!(positions.unwrap(), [5, 8]);
            }
            assert!(commands(&budget, b"1 g f", &[], true).is_none());
            assert!(budget.is_stopped());
            assert!(budget.check().is_err());
        }
    }

    /// 全体を検査し終わる前の白候補を、未完文字列や不正q/Qで確定しない。
    #[test]
    fn incomplete_or_ambiguous_page_cannot_prove_earlier_white() {
        for tail in [
            "(unfinished",
            "[1 2",
            "<< /A 1",
            "Q",
            "q",
            "/No gs",
            "1 1 g",
            "BI",
        ] {
            assert!(
                commands(
                    &DecodeBudget::default(),
                    format!("1 g f {tail}").as_bytes(),
                    &[],
                    true
                )
                .is_none(),
                "{tail}"
            );
        }
    }
}
