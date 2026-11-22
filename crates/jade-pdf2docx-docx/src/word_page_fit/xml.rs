//! 長さ属性だけを倍率変換し、文字列・ID・比率・描画順を保持する。
use super::*;
use quick_xml::events::{BytesText, attributes::Attribute};
use std::borrow::Cow;

/// section終端までを同じ倍率で変換し、通常サイズsectionには倍率を掛けない。
pub(super) fn scale(input: &[u8], plans: &[Plan], body: bool) -> Result<Vec<u8>, ConvertError> {
    let mut reader = Reader::from_reader(input);
    let mut writer = Writer::new(Vec::with_capacity(input.len()));
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut section = 0usize;
    let mut pending_section = false;
    let mut run_size: Option<i64> = None;
    let mut run_cs_size: Option<i64> = None;
    loop {
        let event = reader.read_event().map_err(error)?;
        let plan = plans
            .get(section)
            .ok_or_else(|| error("missing section plan"))?;
        match event {
            Event::Start(ref e) => {
                let name = e.name().as_ref().to_vec();
                if name == b"w:rPr" {
                    run_size = None;
                    run_cs_size = None;
                }
                writer
                    .write_event(Event::Start(attributes(
                        e,
                        plan,
                        stack.last().map(Vec::as_slice),
                    )?))
                    .map_err(error)?;
                if name == b"w:sectPr" && body {
                    references(&mut writer, plan)?;
                }
                stack.push(name);
            }
            Event::Empty(ref e) => {
                let name = e.name();
                if body && matches!(name.as_ref(), b"w:headerReference" | b"w:footerReference") {
                    continue;
                }
                if name.as_ref() == b"w:sz" {
                    run_size = Some(attr(e, b"w:val")?.parse().map_err(error)?);
                }
                if name.as_ref() == b"w:szCs" {
                    run_cs_size = Some(attr(e, b"w:val")?.parse().map_err(error)?);
                }
                if name.as_ref() == b"w:rPr" && plan.longest > MAX_TWIPS {
                    writer
                        .write_event(Event::Start(e.to_owned()))
                        .map_err(error)?;
                    sizes(&mut writer, plan, None, None)?;
                    writer.write_event(Event::End(e.to_end())).map_err(error)?;
                } else {
                    writer
                        .write_event(Event::Empty(attributes(
                            e,
                            plan,
                            stack.last().map(Vec::as_slice),
                        )?))
                        .map_err(error)?;
                }
            }
            Event::End(ref e) => {
                if e.name().as_ref() == b"w:rPr" && plan.longest > MAX_TWIPS {
                    sizes(&mut writer, plan, run_size, run_cs_size)?;
                }
                writer.write_event(event.clone()).map_err(error)?;
                if stack.pop().as_deref() != Some(e.name().as_ref()) {
                    return Err(error("unbalanced OOXML"));
                }
                if body && e.name().as_ref() == b"w:sectPr" {
                    pending_section = true;
                }
                if body
                    && pending_section
                    && (e.name().as_ref() == b"w:p"
                        || stack.last().is_some_and(|n| n == b"w:document"))
                {
                    section = (section + 1).min(plans.len() - 1);
                    pending_section = false;
                }
            }
            Event::Text(ref text)
                if plan.longest > MAX_TWIPS
                    && stack.last().is_some_and(|name| name == b"wp:posOffset") =>
            {
                let value = std::str::from_utf8(text.as_ref())
                    .map_err(error)?
                    .parse::<i64>()
                    .map_err(error)?;
                writer
                    .write_event(Event::Text(BytesText::new(
                        &plan.length(value, false).to_string(),
                    )))
                    .map_err(error)?;
            }
            Event::Eof => break,
            _ => writer.write_event(event).map_err(error)?,
        }
    }
    Ok(writer.into_inner())
}

/// 全sectionに実効参照を明示し、縮小headerから通常sectionへの継承漏れも防ぐ。
fn references(writer: &mut Writer<Vec<u8>>, plan: &Plan) -> Result<(), ConvertError> {
    for name in ["w:headerReference", "w:footerReference"] {
        for ((tag, kind), id) in &plan.references {
            if tag == name {
                empty(writer, tag, &[("w:type", kind), ("r:id", id)])?;
            }
        }
    }
    Ok(())
}

/// 明示サイズのないrunに、既定11ptへ同じ倍率を掛けたサイズを設定する。
fn sizes(
    writer: &mut Writer<Vec<u8>>,
    plan: &Plan,
    size: Option<i64>,
    cs: Option<i64>,
) -> Result<(), ConvertError> {
    let inherited = size
        .or(cs)
        .unwrap_or(crate::DEFAULT_DOCX_FONT_SIZE_HALF_POINTS as i64);
    let value = plan.length(inherited, true).to_string();
    if size.is_none() {
        empty(writer, "w:sz", &[("w:val", &value)])?;
    }
    if cs.is_none() {
        empty(writer, "w:szCs", &[("w:val", &value)])?;
    }
    Ok(())
}

/// 寸法を表す属性だけを変更し、percent・文字単位・色・ID等には触れない。
fn attributes<'a>(
    element: &'a BytesStart<'_>,
    plan: &Plan,
    parent: Option<&[u8]>,
) -> Result<BytesStart<'a>, ConvertError> {
    if plan.longest <= MAX_TWIPS {
        return Ok(element.borrow());
    }
    let mut output = element.borrow().into_owned();
    output.clear_attributes();
    for attribute in element.attributes() {
        let attribute = attribute.map_err(error)?;
        let key = attribute.key.as_ref();
        if let Some(positive) = length_attribute(element, key, parent)? {
            let value = std::str::from_utf8(&attribute.value)
                .map_err(error)?
                .parse::<i64>()
                .map_err(error)?;
            let scaled = plan.length(value, positive);
            output.push_attribute(Attribute {
                key: attribute.key,
                value: Cow::Owned(scaled.to_string().into_bytes()),
            });
        } else {
            output.push_attribute(attribute);
        }
    }
    Ok(output)
}

/// OOXML単位が物理長さである属性を列挙し、正数の寸法か符号付き位置かを返す。
fn length_attribute(
    e: &BytesStart<'_>,
    key: &[u8],
    parent: Option<&[u8]>,
) -> Result<Option<bool>, ConvertError> {
    let name = e.name();
    let name = name.as_ref();
    if matches!(parent, Some(b"w:tblBorders" | b"w:tcBorders" | b"w:pBdr")) {
        return Ok(matches!(key, b"w:sz" | b"w:space").then_some(true));
    }
    let positive = match name {
        b"w:pgSz" => matches!(key, b"w:w" | b"w:h").then_some(true),
        b"w:pgMar" => matches!(
            key,
            b"w:top"
                | b"w:bottom"
                | b"w:left"
                | b"w:right"
                | b"w:header"
                | b"w:footer"
                | b"w:gutter"
        )
        .then_some(false),
        b"w:framePr" => match key {
            b"w:x" | b"w:y" | b"w:hSpace" | b"w:vSpace" => Some(false),
            b"w:w" | b"w:h" => Some(true),
            _ => None,
        },
        b"w:ind" => matches!(
            key,
            b"w:left" | b"w:right" | b"w:start" | b"w:end" | b"w:firstLine" | b"w:hanging"
        )
        .then_some(false),
        b"w:spacing" => {
            if key == b"w:val" && parent == Some(b"w:rPr") {
                Some(false)
            } else if matches!(key, b"w:before" | b"w:after") {
                Some(false)
            } else if key == b"w:line"
                && e.try_get_attribute(b"w:lineRule")
                    .map_err(error)?
                    .is_some_and(|a| matches!(a.value.as_ref(), b"exact" | b"atLeast"))
            {
                Some(true)
            } else {
                None
            }
        }
        b"w:sz" | b"w:szCs" | b"w:kern" => (key == b"w:val").then_some(true),
        b"w:position" => (key == b"w:val").then_some(false),
        b"w:tab" => (key == b"w:pos").then_some(false),
        b"w:cols" | b"w:col" => matches!(key, b"w:w" | b"w:space").then_some(false),
        b"w:gridCol" => (key == b"w:w").then_some(true),
        b"w:trHeight" => (key == b"w:val").then_some(true),
        b"w:tblW" | b"w:tcW" | b"w:tblInd" | b"w:tblCellSpacing" | b"w:top" | b"w:bottom"
        | b"w:left" | b"w:right" | b"w:start" | b"w:end" => (key == b"w:w"
            && e.try_get_attribute(b"w:type")
                .map_err(error)?
                .is_some_and(|a| a.value.as_ref() == b"dxa"))
        .then_some(false),
        b"w:tblpPr" => matches!(
            key,
            b"w:tblpX"
                | b"w:tblpY"
                | b"w:leftFromText"
                | b"w:rightFromText"
                | b"w:topFromText"
                | b"w:bottomFromText"
        )
        .then_some(false),
        b"wp:extent" | b"a:ext" => matches!(key, b"cx" | b"cy").then_some(true),
        b"a:off" | b"wp:simplePos" => matches!(key, b"x" | b"y").then_some(false),
        b"wp:effectExtent" => matches!(key, b"l" | b"t" | b"r" | b"b").then_some(false),
        b"wp:anchor" | b"wp:inline" => {
            matches!(key, b"distT" | b"distB" | b"distL" | b"distR").then_some(false)
        }
        b"a:ln" => (key == b"w").then_some(true),
        _ if matches!(parent, Some(b"w:tblBorders" | b"w:tcBorders" | b"w:pBdr")) => {
            matches!(key, b"w:sz" | b"w:space").then_some(true)
        }
        _ => None,
    };
    Ok(positive)
}
