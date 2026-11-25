//! SFNT内の属性を、フォント名から推測せずDOCXのface選択へ伝える。

/// 単一埋め込みfaceのstyleとsubset制限。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Face {
    /// OS/2またはheadで宣言された太字。
    pub(super) bold: bool,
    /// OS/2またはheadで宣言された斜体・oblique。
    pub(super) italic: bool,
    /// fsTypeがglyph削減を禁止しているか。
    pub(super) no_subsetting: bool,
}

impl Face {
    /// SFNT境界とhead識別子を確認し、OS/2を優先して属性だけを読む。
    pub(super) fn read(bytes: &[u8]) -> Option<Self> {
        if !matches!(bytes.get(..4)?, b"OTTO" | b"\0\x01\0\0" | b"true") {
            return None;
        }
        let head = table(bytes, b"head")??;
        if head.len() < 54
            || head.get(..4)? != b"\0\x01\0\0"
            || head.get(12..16)? != b"\x5f\x0f\x3c\xf5"
        {
            return None;
        }
        let mac = u16_at(head, 44)?;
        let mut result = Self {
            bold: mac & 1 != 0,
            italic: mac & 2 != 0,
            no_subsetting: false,
        };
        if let Some(os2) = table(bytes, b"OS/2")? {
            result.no_subsetting = u16_at(os2, 8)? & 0x100 != 0;
            if os2.len() >= 64 {
                let selection = u16_at(os2, 62)?;
                result.bold = selection & 32 != 0;
                result.italic =
                    selection & 1 != 0 || (u16_at(os2, 0)? >= 4 && selection & 512 != 0);
            }
        }
        Some(result)
    }

    /// 本文のbold/italic指定と一致するOOXML埋め込みslot名。
    pub(super) fn slot(self) -> &'static str {
        match (self.bold, self.italic) {
            (true, true) => "embedBoldItalic",
            (true, false) => "embedBold",
            (false, true) => "embedItalic",
            (false, false) => "embedRegular",
        }
    }
}

/// 宣言されたdirectoryとtable領域を検証し、重複tagや範囲外offsetを拒否する。
fn table<'a>(bytes: &'a [u8], tag: &[u8; 4]) -> Option<Option<&'a [u8]>> {
    let count = usize::from(u16_at(bytes, 4)?);
    let end = 12usize.checked_add(count.checked_mul(16)?)?;
    let directory = bytes.get(12..end)?;
    let mut found = None;
    for entry in directory.chunks_exact(16) {
        let start = usize::try_from(u32::from_be_bytes(entry.get(8..12)?.try_into().ok()?)).ok()?;
        let length =
            usize::try_from(u32::from_be_bytes(entry.get(12..16)?.try_into().ok()?)).ok()?;
        if start < end {
            return None;
        }
        let data = bytes.get(start..start.checked_add(length)?)?;
        if entry.get(..4)? == tag {
            if found.is_some() {
                return None;
            }
            found = Some(data);
        }
    }
    Some(found)
}

/// 指定offsetのbig-endian値を、切り詰めず境界付きで読む。
fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}
