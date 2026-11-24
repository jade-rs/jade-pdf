//! docx-rs 0.4.22が通常体のbCsを有効として出力する挙動を、同じrPr内だけで補正する。
use super::ConvertError;
use docx_rs::XMLDocx;
use quick_xml::{
    Reader, Writer,
    events::{BytesStart, Event},
};

/// 本文と全header/footerへ同じ通常体の梱包規則を適用する。
pub(super) fn apply(package: &mut XMLDocx) -> Result<(), ConvertError> {
    for part in std::iter::once(&mut package.document)
        .chain(package.headers.iter_mut())
        .chain(package.footers.iter_mut())
    {
        rewrite(part)?;
    }
    Ok(())
}

/// 部分的なXMLの公開を避けるため、構文エラーを梱包エラーとして扱う。
fn error(value: impl std::fmt::Display) -> ConvertError {
    ConvertError::Pack(format!("normal font weight: {value}"))
}

/// 同じrPrでLatinの通常体が明示された場合だけ、bCsも明示的な通常体にする。
fn write_properties(
    events: Vec<Event<'static>>,
    writer: &mut Writer<Vec<u8>>,
) -> Result<bool, ConvertError> {
    let normal = events.iter().any(|event| match event {
        Event::Empty(tag) if tag.name().as_ref() == b"w:b" => tag
            .try_get_attribute(b"w:val")
            .ok()
            .flatten()
            .is_some_and(|a| matches!(a.value.as_ref(), b"false" | b"0" | b"off")),
        _ => false,
    });
    let mut changed = false;
    for event in events {
        if normal
            && let Event::Empty(ref tag) = event
            && tag.name().as_ref() == b"w:bCs"
        {
            let mut reset = BytesStart::new("w:bCs");
            for attribute in tag.attributes() {
                let attribute = attribute.map_err(error)?;
                if attribute.key.as_ref() != b"w:val" {
                    reset.push_attribute(attribute);
                }
            }
            reset.push_attribute(("w:val", "false"));
            writer.write_event(Event::Empty(reset)).map_err(error)?;
            changed = true;
        } else {
            writer.write_event(event).map_err(error)?;
        }
    }
    Ok(changed)
}

/// 完成したrPr単位で検査し、本文中の文字列や隣の太字を変更しない。
fn rewrite(part: &mut Vec<u8>) -> Result<(), ConvertError> {
    if !part.windows(4).any(|bytes| bytes == b"w:b ") {
        return Ok(());
    }
    let mut reader = Reader::from_reader(part.as_slice());
    let mut writer = Writer::new(Vec::with_capacity(part.len()));
    let mut properties: Option<Vec<Event<'static>>> = None;
    let mut changed = false;
    loop {
        let event = reader.read_event().map_err(error)?.into_owned();
        let eof = matches!(event, Event::Eof);
        if eof && properties.is_some() {
            return Err(error("unterminated run properties"));
        }
        if matches!(&event,Event::Start(tag) if tag.name().as_ref()==b"w:rPr") {
            if properties.is_some() {
                return Err(error("nested run properties"));
            }
            properties = Some(vec![event]);
            continue;
        }
        if let Some(buffer) = properties.as_mut() {
            let end = matches!(&event,Event::End(tag) if tag.name().as_ref()==b"w:rPr");
            buffer.push(event);
            if end {
                changed |= write_properties(properties.take().unwrap(), &mut writer)?;
            }
        } else {
            writer.write_event(event).map_err(error)?;
        }
        if eof {
            break;
        }
    }
    if changed {
        *part = writer.into_inner();
    }
    Ok(())
}
