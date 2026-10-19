//! 画像に対応するExtGStateを、所属ページのResourceから解決する。
use crate::{
    decode::DecodeBudget,
    metadata,
    metadata_names::resource_name_order,
    resource_resolver::ResourceResolver,
    syntax::{Lexer, Token},
};

/// 継承Resource・間接辞書・世代を検査し、借用したgs名をページ単位で照合する。
pub(crate) fn visit(
    budget: &DecodeBudget,
    source: &[u8],
    contents: &[(usize, Vec<u8>)],
    draw: impl FnMut(usize, &[u8], [f32; 6], super::ImagePaint) -> Option<()>,
) -> Option<()> {
    // gsを含まない本文はResourceを参照しない。誤検出は余分な解決だけで画素を変えない。
    if !contents
        .iter()
        .any(|(_, content)| content.windows(2).any(|word| word == b"gs"))
    {
        return super::visit_painted(budget, contents, draw);
    }
    let objects = crate::object_index::build(budget, source)?;
    let mut resolver = ResourceResolver::new(&objects, budget);
    let resources = match budget.page_resources(source) {
        Some(resources) => resources,
        None => {
            let pages = budget.page_dictionaries(source);
            let mut resources = budget.metadata_vector(pages.len())?;
            for page in pages {
                let dictionary = metadata::dictionary_iter(page).next();
                resources.push(dictionary.and_then(|dictionary| {
                    resolver
                        .resolve(metadata::value(dictionary, b"Resources").unwrap_or(dictionary))
                        .map(|(resources, _)| resources)
                }));
            }
            resources
        }
    };
    super::visit_resolved(
        budget,
        contents,
        |page, name| {
            let resources = resources.get(page).copied().flatten();
            resolve_state(budget, &mut resolver, resources, name)
        },
        draw,
    )
}

/// ページ内の名前だけを照合し、未解決・欠落・停止をno-opと見なさない。
fn resolve_state<'a>(
    budget: &DecodeBudget,
    resolver: &mut ResourceResolver<'_, 'a>,
    resources: Option<&'a [u8]>,
    name: &[u8],
) -> Option<super::paint::Parameters> {
    let (states, _) = resolver.resolve(metadata::value(resources?, b"ExtGState")?)?;
    if !metadata::dictionary_entries_complete(states) {
        return None;
    }
    let mut selected = None;
    for (key, value) in metadata::entries(states)? {
        if budget.is_stopped() {
            return None;
        }
        if crate::metadata_names::same_resource_name(key, name) && selected.replace(value).is_some()
        {
            return None;
        }
    }
    let (state, _) = resolver.resolve(selected?)?;
    parameters(budget, resolver, state)
}

/// 通常合成の非stroke alphaを読み、未知項目・重複・壊れた辞書を拒否する。
fn parameters<'a>(
    budget: &DecodeBudget,
    resolver: &mut ResourceResolver<'_, 'a>,
    state: &'a [u8],
) -> Option<super::paint::Parameters> {
    if !metadata::dictionary_entries_complete(state) {
        return None;
    }
    let keys = [
        "Type", "OPM", "AIS", "BM", "CA", "OP", "SA", "SMask", "ca", "op", "TK",
    ];
    let mut seen = 0u16;
    let mut opacity = None;
    for (key, value) in metadata::entries(state)? {
        if budget.is_stopped() {
            return None;
        }
        let (value, _) = resolver.resolve(value)?;
        let index = keys
            .iter()
            .position(|name| resource_name_order(key, name).is_eq())?;
        if seen & (1 << index) != 0 {
            return None;
        }
        seen |= 1 << index;
        let valid = match index {
            0 => metadata::is_name(value, b"ExtGState"),
            1 => matches!(
                std::str::from_utf8(keyword(value)?)
                    .ok()?
                    .parse::<i32>()
                    .ok()?,
                0 | 1
            ),
            2 | 5 | 9 => keyword(value)? == b"false",
            3 => metadata::is_name(value, b"Normal"),
            4 => alpha(keyword(value)?).is_some(),
            8 => {
                opacity = Some(alpha(keyword(value)?)?);
                true
            }
            6 => keyword(value)? == b"true",
            7 => metadata::is_name(value, b"None"),
            // TKはtext object内のglyph重なりだけを制御し、画像へは影響しない。
            10 => matches!(keyword(value)?, b"true" | b"false"),
            _ => false,
        };
        if !valid {
            return None;
        }
    }
    Some(super::paint::Parameters { opacity })
}

/// 空白・コメント以外の余剰tokenを拒否し、単一keywordを借用する。
fn keyword(value: &[u8]) -> Option<&[u8]> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    let Token::Word(word) = lexer.next()? else {
        return None;
    };
    lexer.next().is_none().then_some(word)
}

/// PDF十進数を字句で0..=1と検証し、丸めで範囲外を受け入れない。
fn alpha(word: &[u8]) -> Option<f64> {
    let unsigned = word
        .strip_prefix(b"+")
        .or_else(|| word.strip_prefix(b"-"))
        .unwrap_or(word);
    let mut parts = unsigned.split(|byte| *byte == b'.');
    let integer = parts.next()?;
    let fraction = parts.next().unwrap_or(b"");
    if parts.next().is_some()
        || integer.is_empty() && fraction.is_empty()
        || !integer.iter().chain(fraction).all(u8::is_ascii_digit)
    {
        return None;
    }
    let zero = integer.iter().chain(fraction).all(|byte| *byte == b'0');
    if word.starts_with(b"-") && !zero {
        return None;
    }
    let leading_zeros = integer.iter().take_while(|byte| **byte == b'0').count();
    let integer = &integer[leading_zeros..];
    if !integer.is_empty() && (integer != b"1" || fraction.iter().any(|byte| *byte != b'0')) {
        return None;
    }
    std::str::from_utf8(word)
        .ok()?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// gsがない本文では、使わないResource索引へ追加容量を予約しない。
    #[test]
    fn unused_state_resources_do_not_consume_metadata_budget() {
        let budget = DecodeBudget::new(&crate::ParseLimits {
            max_total_metadata_workspace_bytes: (4 * std::mem::size_of::<(
                usize,
                super::super::State<'_>,
            )>()) as u64,
            ..Default::default()
        });
        let contents = [(0, b"/Im Do".to_vec())];
        let source = b"%PDF-1.7\n1 0 obj << /Type /Page /Resources << >> >> endobj\n%%EOF";
        assert_eq!(
            visit(&budget, source, &contents, |_, _, _, color| {
                assert_eq!(color, super::super::ImagePaint::default());
                Some(())
            }),
            Some(())
        );
        budget.check().unwrap();
    }

    /// 同名gsでも別ページの辞書を共有せず、ページごとの塗り状態を保持する。
    #[test]
    fn same_state_name_is_resolved_on_its_page_only() {
        let source = b"%PDF-1.7\n1 0 obj << /Type /Page /Resources << /ExtGState << /S << /OPM 1 >> >> >> >> endobj\n2 0 obj << /Type /Page /Resources << /ExtGState << /S << /ca .5 >> >> >> >> endobj\n%%EOF";
        let contents = [
            (0, b"1 0 0 rg /S gs /Im Do".to_vec()),
            (1, b"0 1 0 rg /S gs /Im Do".to_vec()),
        ];
        let budget = DecodeBudget::default();
        let mut colors = Vec::new();
        visit(&budget, source, &contents, |page, _, _, color| {
            colors.push((page, color.color, color.opacity));
            Some(())
        })
        .unwrap();
        assert_eq!(
            colors,
            [
                (0, Some([255, 0, 0]), Some(1.0)),
                (1, Some([0, 255, 0]), Some(0.5))
            ]
        );
        budget.check().unwrap();
    }

    /// caとq保存stackをContents境界で保持し、別ページの既定値とは混同しない。
    #[test]
    fn opacity_and_saved_state_cross_contents_on_their_page_only() {
        let source = b"%PDF-1.7\n1 0 obj << /Type /Page /Resources << /ExtGState << /S << /ca .25 >> >> >> >> endobj\n2 0 obj << /Type /Page >> endobj\n%%EOF";
        let contents = [
            (0, b"q /S gs".to_vec()),
            (1, b"/Im Do".to_vec()),
            (0, b"/Im Do Q /Im Do".to_vec()),
        ];
        let budget = DecodeBudget::default();
        let mut values = Vec::new();
        visit(&budget, source, &contents, |page, _, _, paint| {
            values.push((page, paint.opacity));
            Some(())
        })
        .unwrap();
        assert_eq!(values, [(1, Some(1.0)), (0, Some(0.25)), (0, Some(1.0))]);
        budget.check().unwrap();
    }
}
