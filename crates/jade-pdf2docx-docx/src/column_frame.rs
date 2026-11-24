//! 原本の段間にWordの実段境界を収め、負indentによる前段内での文字欠けを防ぐ。
use crate::{
    ConvertError, DEFAULT_DOCX_COLUMN_SPACE_TWIPS, pdf_points_to_indent_twips, pdf_points_to_twips,
};
use quick_xml::{
    Reader, Writer,
    events::{BytesEnd, BytesStart, Event},
};

/// 一つの二段sectionの、丸め済み余白と任意の明示段幅。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Frame {
    /// sectionと同じ左余白（twips）。
    left: u32,
    /// 均等段が原本の字面へ侵入するときだけ保存する左右段幅（twips）。
    widths: Option<[u32; 2]>,
}

impl Frame {
    /// 両段の字面と有限な段間から、原本を横断しないWord段幅を決める。
    pub(super) fn infer(
        width: f32,
        origin: f32,
        left_right: f32,
        right_left: f32,
        font_size: f32,
    ) -> Option<Self> {
        if ![width, origin, left_right, right_left, font_size]
            .iter()
            .all(|v| v.is_finite())
            || origin < 0.
            || left_right <= origin
            || right_left <= left_right
            || right_left >= width
            || font_size <= 0.
        {
            return None;
        }
        let width = u32::try_from(pdf_points_to_twips(width)?).ok()?;
        let left = u32::try_from(pdf_points_to_indent_twips(origin)?).ok()?;
        let gap = DEFAULT_DOCX_COLUMN_SPACE_TWIPS as u32;
        let available = width.checked_sub(left)?.checked_sub(gap)?;
        let equal_end = f64::from(left) + f64::from(available) / 2.;
        let left_end = f64::from(left_right) * 20.;
        let right_start = f64::from(right_left) * 20.;
        if left_end <= equal_end && equal_end < right_start {
            return Some(Self { left, widths: None });
        }
        // 右段の字面を段間へ張り出させても、左段の本文領域へは入れない。
        let padding = (f64::from(font_size) * 20.).min((right_start - left_end) / 2.);
        let first_end = (right_start - f64::from(gap) / 2.)
            .max(left_end + padding)
            .ceil();
        if first_end <= f64::from(left) || first_end >= right_start {
            return None;
        }
        let first = (first_end as u32).checked_sub(left)?;
        let second = available.checked_sub(first)?;
        if first == 0 || second == 0 {
            return None;
        }
        Some(Self {
            left,
            widths: Some([first, second]),
        })
    }

    /// 保存する実段幅と同じ原点を返し、本文のPDF位置からのindentへ使う。
    pub(super) fn origin(&self, width: f32, index: usize) -> Option<f64> {
        if index >= 2 {
            return None;
        }
        if let Some(widths) = self.widths {
            Some(
                f64::from(self.left)
                    + if index == 1 {
                        f64::from(widths[0]) + f64::from(DEFAULT_DOCX_COLUMN_SPACE_TWIPS)
                    } else {
                        0.
                    },
            )
        } else {
            crate::paragraph_anchor::column_origin_twips(width, self.left as f32 / 20., index, 2)
        }
    }
}

/// 必要なsectionだけ段幅を明示し、寸法と本文section数の不一致を拒否する。
pub(super) fn apply(document: &mut Vec<u8>, frames: &[Frame]) -> Result<(), ConvertError> {
    if frames.iter().all(|frame| frame.widths.is_none()) {
        return Ok(());
    }
    let mut reader = Reader::from_reader(document.as_slice());
    let mut writer = Writer::new(Vec::with_capacity(document.len()));
    let mut index = 0;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| ConvertError::Pack(e.to_string()))?;
        match event {
            Event::Empty(ref e) if e.name().as_ref() == b"w:cols" && two_columns(e)? => {
                let frame = frames
                    .get(index)
                    .ok_or_else(|| ConvertError::Pack("missing column frame".into()))?;
                index += 1;
                if let Some(widths) = frame.widths {
                    let mut cols = BytesStart::new("w:cols");
                    // docx-rsの属性を維持し、均等段指定だけを明示段へ切り替える。
                    for attr in e.attributes() {
                        let attr = attr.map_err(|e| ConvertError::Pack(e.to_string()))?;
                        if attr.key.as_ref() != b"w:equalWidth" {
                            cols.push_attribute(attr);
                        }
                    }
                    cols.push_attribute(("w:equalWidth", "0"));
                    write(&mut writer, Event::Start(cols))?;
                    for (column, width) in widths.iter().enumerate() {
                        let mut col = BytesStart::new("w:col");
                        let width = width.to_string();
                        col.push_attribute(("w:w", width.as_str()));
                        let gap = DEFAULT_DOCX_COLUMN_SPACE_TWIPS.to_string();
                        if column == 0 {
                            col.push_attribute(("w:space", gap.as_str()));
                        }
                        write(&mut writer, Event::Empty(col))?;
                    }
                    write(&mut writer, Event::End(BytesEnd::new("w:cols")))?;
                } else {
                    write(&mut writer, event)?;
                }
            }
            Event::Eof => break,
            _ => write(&mut writer, event)?,
        }
    }
    if index != frames.len() {
        return Err(ConvertError::Pack("unmatched column frames".into()));
    }
    *document = writer.into_inner();
    Ok(())
}

/// 出力済みsectionの段数を、属性順に依存せず確認する。
fn two_columns(element: &BytesStart<'_>) -> Result<bool, ConvertError> {
    for attr in element.attributes() {
        let attr = attr.map_err(|e| ConvertError::Pack(e.to_string()))?;
        if attr.key.as_ref() == b"w:num" {
            return Ok(attr.value.as_ref() == b"2");
        }
    }
    Ok(false)
}

/// XML書き込み失敗を通常のDOCX梱包エラーとして返す。
fn write(writer: &mut Writer<Vec<u8>>, event: Event<'_>) -> Result<(), ConvertError> {
    writer
        .write_event(event)
        .map_err(|e| ConvertError::Pack(e.to_string()))
}
