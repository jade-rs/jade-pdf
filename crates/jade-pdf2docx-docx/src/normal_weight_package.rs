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
    let mut normal = false;
    for event in &events {
        if let Event::Empty(tag) = event
            && tag.name().as_ref() == b"w:b"
        {
            let value = tag.try_get_attribute(b"w:val").map_err(error)?;
            normal |= value.is_some_and(|a| matches!(a.value.as_ref(), b"false" | b"0" | b"off"));
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 属性順・run順・識別文字列に依らず、同じrPrだけの通常体を補正する。
        #[test]
        fn normal_reset_is_local_and_idempotent(reverse in any::<bool>(), digits in 0_u32..100000) {
            let properties=if reverse {"<w:bCs /><w:b w:val=\"false\" />"} else {"<w:b w:val=\"false\" /><w:bCs />"};
            let input=format!("<w:document><w:r><w:rPr>{properties}<w:position w:val=\"12\"/></w:rPr><w:t>{digits}&lt;w:bCs /&gt;</w:t></w:r><w:r><w:rPr><w:b/><w:bCs /></w:rPr><w:t>bold</w:t></w:r></w:document>");
            let mut bytes=input.into_bytes();rewrite(&mut bytes).unwrap();
            let output=String::from_utf8(bytes.clone()).unwrap();
            prop_assert_eq!(output.matches("<w:bCs w:val=\"false\"").count(),1);
            prop_assert!(output.contains("<w:b/><w:bCs />"));
            prop_assert!(output.contains("<w:position w:val=\"12\"/>"));
            let expected_text=format!("{digits}&lt;w:bCs /&gt;");
            prop_assert!(output.contains(&expected_text));
            let once=bytes.clone();rewrite(&mut bytes).unwrap();prop_assert_eq!(bytes,once);
        }
        /// 任意の入力はpanicせず、失敗した場合は元のXMLを変更しない。
        #[test]
        fn malformed_parts_fail_without_partial_mutation(bytes in proptest::collection::vec(any::<u8>(),0..512)) {
            let mut actual=bytes.clone();
            if rewrite(&mut actual).is_err() {prop_assert_eq!(actual,bytes);}
        }
    }

    /// 隣のparagraphや表の属性にある太字を、別のrPrの通常体から解除しない。
    #[test]
    fn explicit_normal_does_not_leak_to_another_property_scope() {
        let mut bytes=b"<w:d><w:rPr><w:b w:val=\"false\" /></w:rPr><w:rPr><w:bCs /></w:rPr><w:tcPr><w:bCs /></w:tcPr></w:d>".to_vec();
        let before = bytes.clone();
        rewrite(&mut bytes).unwrap();
        assert_eq!(bytes, before);
    }

    /// 途中で切れたrPrの補正は公開せず、梱包を失敗させる。
    #[test]
    fn incomplete_properties_preserve_the_original_bytes() {
        let mut bytes = b"<w:d><w:rPr><w:b w:val=\"false\"/><w:bCs/>".to_vec();
        let before = bytes.clone();
        assert!(rewrite(&mut bytes).is_err());
        assert_eq!(bytes, before);
    }
}
