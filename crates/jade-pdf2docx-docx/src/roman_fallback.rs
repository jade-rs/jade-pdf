//! 既知のPostScript Roman互換faceにだけ、既存のDOCX代替familyと属性を対応させる。

/// 埋め込まない既知faceの、Word向けの互換familyとstyle。
#[derive(Clone, Copy)]
pub(super) struct Font {
    /// 元のTimes互換字形を解決する既存のWord family。
    pub(super) family: &'static str,
    /// Medi faceが宣言する太字。
    pub(super) bold: bool,
    /// Ital faceが宣言する斜体。
    pub(super) italic: bool,
}

/// 既知四faceの完全一致だけを対応させ、未確認の派生名や部分一致は推測しない。
pub(super) fn known(name: &str) -> Option<Font> {
    let name = match name.split_once('+') {
        Some((prefix, name))
            if prefix.len() == 6 && prefix.bytes().all(|b| b.is_ascii_uppercase()) =>
        {
            name
        }
        Some(_) => return None,
        None => name,
    }
    .trim();
    let (bold, italic) = if name.eq_ignore_ascii_case("NimbusRomNo9L-Regu") {
        (false, false)
    } else if name.eq_ignore_ascii_case("NimbusRomNo9L-Medi") {
        (true, false)
    } else if name.eq_ignore_ascii_case("NimbusRomNo9L-ReguItal") {
        (false, true)
    } else if name.eq_ignore_ascii_case("NimbusRomNo9L-MediItal") {
        (true, true)
    } else {
        return None;
    };
    Some(Font {
        family: "Times New Roman",
        bold,
        italic,
    })
}
