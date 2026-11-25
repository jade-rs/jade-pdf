//! ルビ（ふりがな）を見つけ、DOCX のルビ（`w:ruby`）として出す。
//!
//! PDF のルビは、親文字の上に小さな仮名を置いただけの別の文字片になる。そのままでは親文字の
//! 行の上に仮名だけの行ができ、本文の読み順が乱れる。lawrenceanum の kanpo-parser と同じく、
//! 親文字の上に接して置かれた小さな仮名を親文字のルビとみなす。
//!
//! 親文字とルビは Unicode の行間注記の記号（U+FFF9 親文字の開始・U+FFFA ルビの開始・
//! U+FFFB 終わり）で本文に埋め込み、DOCX の梱包時に [`apply_to_document_xml`] で
//! `w:ruby` に書き換える。

use super::*;

/// 親文字の開始。
pub(crate) const ANCHOR: char = '\u{FFF9}';
/// ルビの開始。
pub(crate) const SEPARATOR: char = '\u{FFFA}';
/// 注記の終わり。
pub(crate) const TERMINATOR: char = '\u{FFFB}';

/// 親文字に対するルビの大きさの上限の比。
const MAX_SIZE_RATIO: f32 = 0.7;

/// ルビの字（仮名・長音記号）だけの文字列か。
fn is_ruby_text(text: &str) -> bool {
    let body = text.trim();
    !body.is_empty()
        && body
            .chars()
            .all(|ch| ('\u{3041}'..='\u{30FF}').contains(&ch) && ch != '・')
}

/// 漢字を含むか。
fn has_kanji(text: &str) -> bool {
    text.chars().any(|ch| {
        ('\u{4E00}'..='\u{9FFF}').contains(&ch)
            || ('\u{3400}'..='\u{4DBF}').contains(&ch)
            || ch == '々'
    })
}

/// 文字片の大きさ（font size が無ければ矩形の高さ）。
fn size_of(text: &ParsedTextElement) -> f32 {
    let bounds = normalized_bounds(text.bounds);
    if text.font_size > 0.0 {
        text.font_size
    } else {
        bounds.y1 - bounds.y0
    }
}

/// 親文字の字の範囲に振ったルビ（最初の字, 最後の字, ルビ, ルビの左端, ルビの右端, ルビの大きさ）。
type RubySpan = (usize, usize, String, f32, f32, f32);

/// ルビの候補の文字片か（仮名だけで、すぐ下に大きな漢字の文字片がある）。
fn is_ruby_over(ruby: &ParsedTextElement, base: &ParsedTextElement) -> bool {
    let (ruby_bounds, bounds) = (
        normalized_bounds(ruby.bounds),
        normalized_bounds(base.bounds),
    );
    let base_size = size_of(base);
    ruby.page == base.page
        && size_of(ruby) <= MAX_SIZE_RATIO * base_size
        && has_kanji(&base.text)
        && ruby_bounds.y0 >= bounds.y1 - 0.3 * base_size
        && ruby_bounds.y0 <= bounds.y1 + 0.5 * base_size
        && ruby_bounds.x1 >= bounds.x0 - 0.5 * base_size
        && ruby_bounds.x0 <= bounds.x1 + 0.5 * base_size
}

/// 文字片の各字の中心の x 座標（字の幅を等しいとみなす）。
fn char_centers(text: &ParsedTextElement) -> Vec<(char, f32)> {
    let bounds = normalized_bounds(text.bounds);
    let chars = text.text.trim().chars().collect::<Vec<_>>();
    let advance = (bounds.x1 - bounds.x0) / chars.len().max(1) as f32;
    chars
        .into_iter()
        .enumerate()
        .map(|(index, ch)| (ch, bounds.x0 + advance * (index as f32 + 0.5)))
        .collect()
}

/// 親文字の上のルビを見つけ、親文字の文字片にルビを埋め込む。
///
/// ルビの字ごとに、真下の親文字の字を求める（ルビは親文字の上に字間を空けて振り分けて
/// 組むことがあるので、ルビの文字片の区切りには頼らない）。続く親文字に振られたルビの字を
/// 1 つのルビにする。
pub(super) fn attach(parsed: &mut ParsedDocument) {
    let is_ruby = |text: &ParsedTextElement| is_ruby_text(&text.text);
    let ruby_indices = (0..parsed.text_elements.len())
        .filter(|&index| is_ruby(&parsed.text_elements[index]))
        .filter(|&index| {
            parsed
                .text_elements
                .iter()
                .any(|base| is_ruby_over(&parsed.text_elements[index], base))
        })
        .collect::<BTreeSet<_>>();
    if ruby_indices.is_empty() {
        return;
    }
    // ルビの下の親文字の文字片で、同じ行に字間を空けずに続くものを 1 つにまとめる。
    let mut targets = (0..parsed.text_elements.len())
        .filter(|&index| {
            !ruby_indices.contains(&index)
                && ruby_indices.iter().any(|&ruby| {
                    is_ruby_over(&parsed.text_elements[ruby], &parsed.text_elements[index])
                })
        })
        .collect::<Vec<_>>();
    targets.sort_by(|&a, &b| {
        let (a, b) = (&parsed.text_elements[a], &parsed.text_elements[b]);
        a.page.cmp(&b.page).then(
            normalized_bounds(a.bounds)
                .x0
                .total_cmp(&normalized_bounds(b.bounds).x0),
        )
    });
    let mut remove = BTreeSet::new();
    let mut merged_targets: Vec<usize> = Vec::new();
    for index in targets {
        let next = &parsed.text_elements[index];
        let b = normalized_bounds(next.bounds);
        // 同じ行で、字間を空けずに左に続く親文字の文字片を探す。
        let previous = merged_targets.iter().copied().find(|&last| {
            let previous = &parsed.text_elements[last];
            let a = normalized_bounds(previous.bounds);
            let size = size_of(previous);
            previous.page == next.page
                && (a.y0 - b.y0).abs() <= 0.2 * size
                && b.x0 >= a.x0
                && b.x0 - a.x1 <= 0.8 * size
        });
        if let Some(last) = previous {
            let text = next.text.trim().to_string();
            let a = normalized_bounds(parsed.text_elements[last].bounds);
            let element = &mut parsed.text_elements[last];
            element.text = format!("{}{text}", element.text.trim_end());
            element.bounds = ParsedBounds {
                x0: a.x0,
                y0: a.y0.min(b.y0),
                x1: b.x1.max(a.x1),
                y1: a.y1.max(b.y1),
            };
            remove.insert(index);
            continue;
        }
        merged_targets.push(index);
    }
    // 続くルビの文字片（同じ大きさ・同じ高さ）を 1 つのルビにまとめる。
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut ordered = ruby_indices.iter().copied().collect::<Vec<_>>();
    ordered.sort_by(|&a, &b| {
        let (a, b) = (&parsed.text_elements[a], &parsed.text_elements[b]);
        a.page
            .cmp(&b.page)
            .then(
                normalized_bounds(b.bounds)
                    .y0
                    .total_cmp(&normalized_bounds(a.bounds).y0),
            )
            .then(
                normalized_bounds(a.bounds)
                    .x0
                    .total_cmp(&normalized_bounds(b.bounds).x0),
            )
    });
    for index in ordered {
        let joins = groups.last().is_some_and(|group| {
            let (previous, next) = (
                &parsed.text_elements[group[group.len() - 1]],
                &parsed.text_elements[index],
            );
            let (a, b) = (
                normalized_bounds(previous.bounds),
                normalized_bounds(next.bounds),
            );
            let size = size_of(next);
            previous.page == next.page
                && (size_of(previous) - size).abs() <= 0.5
                && (a.y0 - b.y0).abs() <= 0.5
                && b.x0 >= a.x0
                && b.x0 - a.x1 <= 1.5 * size
        });
        if joins {
            groups.last_mut().expect("group").push(index);
        } else {
            groups.push(vec![index]);
        }
    }
    // ルビのまとまりごとに、真下の親文字の字の範囲を求める。
    // 親文字の文字片 → (最初の字, 最後の字, ルビ, ルビの左端, ルビの右端, ルビの大きさ)
    let mut assignments: BTreeMap<usize, Vec<RubySpan>> = BTreeMap::new();
    for group in groups {
        let mut base_of_group = None;
        let mut positions = Vec::new();
        for &ruby in &group {
            let ruby_element = &parsed.text_elements[ruby];
            for (_, center) in char_centers(ruby_element) {
                let parent = merged_targets.iter().copied().find_map(|base| {
                    let element = &parsed.text_elements[base];
                    if !is_ruby_over(ruby_element, element) {
                        return None;
                    }
                    let centers = char_centers(element);
                    let advance = centers
                        .get(1)
                        .map_or(size_of(element), |next| next.1 - centers[0].1);
                    centers
                        .iter()
                        .enumerate()
                        .min_by(|a, b| (a.1.1 - center).abs().total_cmp(&(b.1.1 - center).abs()))
                        .filter(|(_, (_, x))| (x - center).abs() <= advance.abs().max(1.0))
                        .map(|(position, _)| (base, position))
                });
                match parent {
                    Some((base, position)) if base_of_group.is_none_or(|other| other == base) => {
                        base_of_group = Some(base);
                        positions.push(position);
                    }
                    _ => {
                        positions.clear();
                        break;
                    }
                }
            }
            if positions.is_empty() {
                break;
            }
        }
        let (Some(base), Some(first), Some(last)) = (
            base_of_group,
            positions.iter().min().copied(),
            positions.iter().max().copied(),
        ) else {
            continue;
        };
        let reading = group
            .iter()
            .map(|&index| parsed.text_elements[index].text.trim())
            .collect::<String>();
        let left = normalized_bounds(parsed.text_elements[group[0]].bounds).x0;
        let right = normalized_bounds(parsed.text_elements[group[group.len() - 1]].bounds).x1;
        let size = size_of(&parsed.text_elements[group[0]]);
        assignments
            .entry(base)
            .or_default()
            .push((first, last, reading, left, right, size));
        remove.extend(group);
    }
    // 親文字の文字片ごとに、隣り合う字のルビで字間の狭いものを 1 つにまとめて埋め込む。
    for (base, mut marks) in assignments {
        marks.sort_by(|a, b| a.3.total_cmp(&b.3));
        let chars = parsed.text_elements[base]
            .text
            .trim()
            .chars()
            .collect::<Vec<_>>();
        let mut spans: Vec<RubySpan> = Vec::new();
        for mark in marks {
            match spans.last_mut() {
                Some(span)
                    if mark.0 <= span.1 + 1
                        && mark.0 >= span.0
                        && mark.3 - span.4 <= 3.0 * mark.5 =>
                {
                    span.1 = span.1.max(mark.1);
                    span.2.push_str(&mark.2);
                    span.4 = mark.4;
                }
                _ => spans.push(mark),
            }
        }
        let mut text = String::new();
        let mut next = 0;
        for (mut first, mut last, reading, ..) in spans {
            if first < next || last >= chars.len() {
                continue;
            }
            // 親文字の範囲の端の数字・記号（ルビの幅で隣の字までかかったもの）は親文字に含めない。
            while last > first && !has_kanji(&chars[last].to_string()) {
                last -= 1;
            }
            while first < last && !has_kanji(&chars[first].to_string()) {
                first += 1;
            }
            text.extend(&chars[next..first]);
            text.push(ANCHOR);
            text.extend(&chars[first..=last]);
            text.push(SEPARATOR);
            text.push_str(&reading);
            text.push(TERMINATOR);
            next = last + 1;
        }
        text.extend(&chars[next.min(chars.len())..]);
        let trailing = if parsed.text_elements[base]
            .text
            .ends_with(char::is_whitespace)
        {
            " "
        } else {
            ""
        };
        parsed.text_elements[base].text = format!("{text}{trailing}");
    }
    let mut index = 0;
    parsed.text_elements.retain(|_| {
        let keep = !remove.contains(&index);
        index += 1;
        keep
    });
}

/// XML 本文の文字を escape する。
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 注記の記号を、親文字（ルビ）の括弧書きに置き換える（`w:ruby` にできない場合）。
fn plain(text: &str) -> String {
    text.replace(ANCHOR, "")
        .replace(SEPARATOR, "（")
        .replace(TERMINATOR, "）")
}

/// 1 つの run の本文を、注記ごとの（親文字, ルビ）と通常の本文の並びに分ける。
fn split_annotations(text: &str) -> Option<Vec<(String, Option<String>)>> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(ANCHOR) {
        if start > 0 {
            parts.push((rest[..start].to_string(), None));
        }
        let after = &rest[start + ANCHOR.len_utf8()..];
        let separator = after.find(SEPARATOR)?;
        let terminator = after.find(TERMINATOR)?;
        if terminator < separator {
            return None;
        }
        parts.push((
            after[..separator].to_string(),
            Some(after[separator + SEPARATOR.len_utf8()..terminator].to_string()),
        ));
        rest = &after[terminator + TERMINATOR.len_utf8()..];
    }
    if !rest.is_empty() {
        parts.push((rest.to_string(), None));
    }
    Some(parts)
}

/// document.xml の注記の記号を `w:ruby` に書き換える。
pub(crate) fn apply_to_document_xml(document: &mut Vec<u8>) {
    let Ok(xml) = std::str::from_utf8(document) else {
        return;
    };
    if !xml.contains(ANCHOR) {
        return;
    }
    let mut out = String::with_capacity(xml.len() + 1024);
    let mut rest = xml;
    while let Some(start) = rest.find("<w:r>") {
        let Some(length) = rest[start..].find("</w:r>") else {
            break;
        };
        let end = start + length + "</w:r>".len();
        out.push_str(&rest[..start]);
        out.push_str(&rewrite_run(&rest[start..end]));
        rest = &rest[end..];
    }
    out.push_str(rest);
    // ルビを含む段落の行の高さと表の行の高さは、ルビの分だけ伸ばせるようにする。
    let out = loosen_heights(
        &out,
        "<w:p ",
        "</w:p>",
        "w:lineRule=\"exact\"",
        "w:lineRule=\"atLeast\"",
    );
    let out = loosen_heights(
        &out,
        "<w:tr>",
        "</w:tr>",
        "w:hRule=\"exact\"",
        "w:hRule=\"atLeast\"",
    );
    // 書き換えられなかった記号は括弧書きにする。
    *document = plain(&out).into_bytes();
}

/// `open`〜`close` の要素のうちルビを含むものだけ、`from` を `to` に置き換える。
fn loosen_heights(xml: &str, open: &str, close: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find(open) {
        let Some(length) = rest[start..].find(close) else {
            break;
        };
        let end = start + length + close.len();
        out.push_str(&rest[..start]);
        let element = &rest[start..end];
        if element.contains("<w:ruby>") {
            out.push_str(&element.replace(from, to));
        } else {
            out.push_str(element);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// 和文の字か（ルビの前後の空白を除くかの判定に使う）。
fn is_japanese(ch: char) -> bool {
    matches!(ch as u32, 0x3000..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xFF00..=0xFFEF)
}

/// 注記を含む 1 つの run を、通常の run とルビの run に書き換える。
fn rewrite_run(run: &str) -> String {
    if !run.contains(ANCHOR) || run.matches("<w:t").count() != 1 {
        return run.to_string();
    }
    let properties = run
        .find("<w:rPr>")
        .and_then(|start| {
            run[start..]
                .find("</w:rPr>")
                .map(|end| &run[start..start + end + "</w:rPr>".len()])
        })
        .unwrap_or("");
    let Some(text_start) = run.find("<w:t") else {
        return run.to_string();
    };
    let Some(open_end) = run[text_start..].find('>') else {
        return run.to_string();
    };
    let body_start = text_start + open_end + 1;
    let Some(body_length) = run[body_start..].find("</w:t>") else {
        return run.to_string();
    };
    let body = &run[body_start..body_start + body_length];
    let Some(parts) = split_annotations(body) else {
        return run.to_string();
    };
    // 親文字の大きさ（half-point）。指定が無ければ Word の既定の 10.5pt。
    let base_half_points = properties
        .find("<w:sz w:val=\"")
        .and_then(|start| {
            let value = &properties[start + "<w:sz w:val=\"".len()..];
            value[..value.find('"')?].parse::<usize>().ok()
        })
        .unwrap_or(21);
    let ruby_half_points = (base_half_points / 2).max(2);
    let ruby_properties = if properties.contains("<w:sz ") {
        properties
            .replace(
                &format!("<w:sz w:val=\"{base_half_points}\""),
                &format!("<w:sz w:val=\"{ruby_half_points}\""),
            )
            .replace(
                &format!("<w:szCs w:val=\"{base_half_points}\""),
                &format!("<w:szCs w:val=\"{ruby_half_points}\""),
            )
    } else if properties.is_empty() {
        format!("<w:rPr><w:sz w:val=\"{ruby_half_points}\" /></w:rPr>")
    } else {
        properties.replace(
            "</w:rPr>",
            &format!("<w:sz w:val=\"{ruby_half_points}\" /></w:rPr>"),
        )
    };
    // ルビの付いた親文字と隣の和文の間の空白（ルビの幅で空いた字間）は除く。
    let mut parts = parts;
    for index in 0..parts.len() {
        if parts[index].1.is_some() {
            continue;
        }
        let before_ruby = parts.get(index + 1).is_some_and(|part| part.1.is_some());
        let after_ruby = index > 0 && parts[index - 1].1.is_some();
        let text = &mut parts[index].0;
        if before_ruby
            && text.ends_with(' ')
            && text.trim_end().chars().last().is_some_and(is_japanese)
        {
            text.truncate(text.trim_end().len());
        }
        if after_ruby
            && text.starts_with(' ')
            && text.trim_start().chars().next().is_some_and(is_japanese)
        {
            *text = text.trim_start().to_string();
        }
    }
    let mut out = String::new();
    for (text, reading) in parts {
        if text.is_empty() {
            continue;
        }
        let text = text.replace([ANCHOR, SEPARATOR, TERMINATOR], "");
        match reading {
            None => out.push_str(&format!(
                "<w:r>{properties}<w:t xml:space=\"preserve\">{text}</w:t></w:r>"
            )),
            Some(reading) => out.push_str(&format!(
                "<w:r><w:ruby><w:rubyPr><w:rubyAlign w:val=\"center\" /><w:hps w:val=\"{ruby_half_points}\" /><w:hpsRaise w:val=\"{base_half_points}\" /><w:hpsBaseText w:val=\"{base_half_points}\" /><w:lid w:val=\"ja-JP\" /></w:rubyPr><w:rt><w:r>{ruby_properties}<w:t xml:space=\"preserve\">{}</w:t></w:r></w:rt><w:rubyBase><w:r>{properties}<w:t xml:space=\"preserve\">{text}</w:t></w:r></w:rubyBase></w:ruby></w:r>",
                escape(&reading.replace([ANCHOR, SEPARATOR, TERMINATOR], ""))
            )),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 注記を含む run は、通常の run とルビの run に分かれる。
    #[test]
    fn rewrites_annotations_as_ruby_runs() {
        let mut xml = format!(
            "<w:p><w:r><w:rPr><w:sz w:val=\"24\" /></w:rPr><w:t xml:space=\"preserve\">本日は{ANCHOR}東京{SEPARATOR}とうきょう{TERMINATOR}の</w:t></w:r></w:p>"
        )
        .into_bytes();
        apply_to_document_xml(&mut xml);
        let xml = String::from_utf8(xml).unwrap();
        assert!(xml.contains("<w:ruby>"), "{xml}");
        assert!(xml.contains("<w:rt><w:r><w:rPr><w:sz w:val=\"12\" /></w:rPr><w:t xml:space=\"preserve\">とうきょう</w:t>"), "{xml}");
        assert!(xml.contains("<w:rubyBase><w:r><w:rPr><w:sz w:val=\"24\" /></w:rPr><w:t xml:space=\"preserve\">東京</w:t>"), "{xml}");
        assert!(
            xml.contains(">本日は</w:t>") && xml.contains(">の</w:t>"),
            "{xml}"
        );
        assert!(!xml.contains(ANCHOR));
    }

    /// テスト用の文字片。
    fn text(body: &str, x0: f32, y0: f32, x1: f32, size: f32) -> ParsedTextElement {
        ParsedTextElement {
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds {
                x0,
                y0,
                x1,
                y1: y0 + size,
            },
            font_size: size,
            font_name: None,
            fill_color: None,
            text: body.to_string(),
        }
    }

    /// 字ごとに分かれた親文字と、字間を空けて振り分けたルビを 1 つのルビにまとめる。
    #[test]
    fn attaches_spread_ruby_to_its_base_characters() {
        let mut parsed = ParsedDocument {
            quality: QualityReport::default(),
            path: std::path::PathBuf::new(),
            byte_len: 0,
            header: "%PDF-1.4".into(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 4 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            fills: Vec::new(),
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
            text_elements: vec![
                text("大", 568.9, 159.8, 576.0, 8.0),
                text("分", 576.9, 159.7, 584.2, 8.0),
                text("林", 584.9, 159.8, 592.1, 8.0),
                text("研", 592.6, 159.7, 600.3, 8.0),
                text("おお", 568.8, 167.7, 576.1, 4.0),
                text("いた", 577.0, 167.7, 584.2, 4.0),
                text("りん", 585.4, 167.7, 592.2, 4.0),
                text("けん", 593.0, 167.7, 600.2, 4.0),
                text("号", 620.0, 159.7, 628.0, 8.0),
            ],
        };
        attach(&mut parsed);
        let texts = parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec![
                format!("{ANCHOR}大分林研{SEPARATOR}おおいたりんけん{TERMINATOR}").as_str(),
                "号"
            ]
        );
    }

    /// 書き換えられない記号は括弧書きになる。
    #[test]
    fn leftover_annotations_become_parentheses() {
        let mut xml =
            format!("<w:t>{ANCHOR}東京{SEPARATOR}とうきょう{TERMINATOR}</w:t>").into_bytes();
        apply_to_document_xml(&mut xml);
        assert_eq!(
            String::from_utf8(xml).unwrap(),
            "<w:t>東京（とうきょう）</w:t>"
        );
    }
}
