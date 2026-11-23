//! 自分で生成したDOCX runの指定を読み、未埋め込みsourceのfamily参照と内部spaceを区別する。
use crate::ConvertError;
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};

/// 内部spaceに必ず指定する属性だけを、現在のrunについて保持する。
#[derive(Default)]
struct Signature {
    /// 数字suffixを持つ内部候補family。
    family: Option<String>,
    /// font sizeが1ptか。
    one_point: bool,
    /// 明示的な字間指定があるか。
    spacing: bool,
    /// 水平倍率が100percentか。
    full_width: bool,
    /// run本文がU+0020一つか。
    space: bool,
}

/// sourceからXMLを受け取らない生成packageの、所有space runが選ぶfamilyだけを返す。
pub(crate) fn read(document: &[u8]) -> Result<Option<String>, ConvertError> {
    let mut reader = Reader::from_reader(document);
    let mut run = None::<Signature>;
    let mut in_text = false;
    loop {
        match reader
            .read_event()
            .map_err(|e| ConvertError::Pack(e.to_string()))?
        {
            Event::Start(ref e) if e.name().as_ref() == b"w:r" => run = Some(Signature::default()),
            Event::Start(ref e) if e.name().as_ref() == b"w:t" => in_text = true,
            Event::Empty(ref e) => {
                if let Some(run) = run.as_mut() {
                    match e.name().as_ref() {
                        b"w:rFonts" => {
                            if let Some(name) = attribute(e, b"w:ascii")?
                                && name.len() < 63
                                && name.starts_with("JadeLayoutSpace-")
                                && name[16..].parse::<usize>().is_ok()
                            {
                                run.family = Some(name);
                            }
                        }
                        b"w:sz" => run.one_point = attribute(e, b"w:val")?.as_deref() == Some("2"),
                        b"w:spacing" => {
                            run.spacing =
                                attribute(e, b"w:val")?.is_some_and(|v| v.parse::<i32>().is_ok())
                        }
                        b"w:w" => {
                            run.full_width = attribute(e, b"w:val")?.as_deref() == Some("100")
                        }
                        _ => {}
                    }
                }
            }
            Event::Text(ref text) if in_text => {
                if let Some(run) = run.as_mut() {
                    run.space = &text[..] == b" ";
                }
            }
            Event::End(ref e) if e.name().as_ref() == b"w:t" => in_text = false,
            Event::End(ref e) if e.name().as_ref() == b"w:r" => {
                if let Some(run) = run.take()
                    && run.one_point
                    && run.spacing
                    && run.full_width
                    && run.space
                    && run.family.is_some()
                {
                    return Ok(run.family);
                }
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
    }
}

/// serializerが出した少数の属性を境界付きで読み、不正XMLを黙って無視しない。
fn attribute(start: &BytesStart<'_>, key: &[u8]) -> Result<Option<String>, ConvertError> {
    let value = start
        .try_get_attribute(key)
        .map_err(|e| ConvertError::Pack(e.to_string()))?;
    value
        .map(|a| {
            std::str::from_utf8(a.value.as_ref())
                .map(str::to_owned)
                .map_err(|e| ConvertError::Pack(e.to_string()))
        })
        .transpose()
}
