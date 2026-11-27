//! 原本の段間にWordの実段境界を収め、負indentによる前段内での文字欠けを防ぐ。
use crate::{
    ConvertError, DEFAULT_DOCX_COLUMN_SPACE_TWIPS, pdf_points_to_indent_twips, pdf_points_to_twips,
};
use quick_xml::{
    Reader, Writer,
    events::{BytesEnd, BytesStart, Event},
};

#[cfg(test)]
#[path = "column_frame_tests.rs"]
mod tests;

/// 一つの二段sectionの、丸め済み余白と任意の明示段幅。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Frame {
    /// sectionと同じ左余白（twips）。
    left: u32,
    /// 均等段が原本の字面へ侵入する場合や、段間の縦罫線を保存する場合の左右段幅（twips）。
    widths: Option<[u32; 2]>,
    /// 左右の実段を隔てる距離（twips）。
    space: u32,
    /// 原本の独立した縦罫線をWordの段区切りとして出力するか。
    separator: bool,
}

impl Frame {
    /// 段組本文だけの左右字面を集め、全幅の見出し・末尾注記を段幅へ混ぜない。
    pub(super) fn for_lines(
        width: f32,
        origin: f32,
        lines: &[crate::StructuredTextLine],
        columns: &[f32],
        column_rule_x: Option<f32>,
    ) -> Option<Self> {
        let mut left_right = f32::NEG_INFINITY;
        let mut right_left = f32::INFINITY;
        let mut font_size = 0.0f32;
        for line in lines {
            if ![line.left_x, line.right_x].iter().all(|v| v.is_finite())
                || line.left_x < origin
                || line.right_x > width
                || line.right_x <= line.left_x
            {
                return None;
            }
            if crate::layout_column_index(line, columns, column_rule_x) == 0 {
                left_right = left_right.max(line.right_x);
            } else {
                right_left = right_left.min(line.left_x);
            }
            font_size = font_size.max(crate::structured_line_max_font_size(line));
        }
        if let Some(x) = column_rule_x {
            return Self::with_separator(width, origin, left_right, right_left, x);
        }
        Self::infer(width, origin, left_right, right_left, font_size)
    }

    /// 原本の縦罫線を実段間の中央に置き、右の段を原本の右側本文の左端から始める。
    fn with_separator(
        width: f32,
        origin: f32,
        left_right: f32,
        right_left: f32,
        x: f32,
    ) -> Option<Self> {
        let twips = |points| u32::try_from(pdf_points_to_twips(points)?).ok();
        let width = twips(width)?;
        let left = twips(origin)?;
        let right_start = twips(right_left)?;
        let center = twips(x)?;
        let half_space = right_start.checked_sub(center)?;
        let left_end = center.checked_sub(half_space)?;
        if half_space == 0
            || left_end <= left
            || right_start >= width
            || f64::from(left_end) <= f64::from(left_right) * 20.
        {
            return None;
        }
        Some(Self {
            left,
            widths: Some([left_end - left, width - right_start]),
            space: half_space.checked_mul(2)?,
            separator: true,
        })
    }

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
            return Some(Self {
                left,
                widths: None,
                space: gap,
                separator: false,
            });
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
            space: gap,
            separator: false,
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
                        f64::from(widths[0]) + f64::from(self.space)
                    } else {
                        0.
                    },
            )
        } else {
            crate::paragraph_anchor::column_origin_twips(width, self.left as f32 / 20., index, 2)
        }
    }

    /// 明示幅の段で、段間の手前までの右端を返す。
    pub(super) fn right_edge_twips(&self, index: usize) -> Option<f64> {
        let widths = self.widths?;
        let width = *widths.get(index)?;
        let offset = if index == 1 {
            f64::from(widths[0]) + f64::from(self.space)
        } else {
            0.
        };
        Some(f64::from(self.left) + offset + f64::from(width))
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
                    if frame.separator {
                        cols.push_attribute(("w:sep", "1"));
                    }
                    write(&mut writer, Event::Start(cols))?;
                    for (column, width) in widths.iter().enumerate() {
                        let mut col = BytesStart::new("w:col");
                        let width = width.to_string();
                        col.push_attribute(("w:w", width.as_str()));
                        let gap = frame.space.to_string();
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
