//! 元fontの明示weight接尾辞をfamilyから独立して扱う。

/// family名の部分一致を避け、明示されたstyle接尾辞だけをweightとする。
pub(super) fn weight(name: &str) -> Option<bool> {
    let suffix = name.rsplit_once(['-', ','])?.1.to_ascii_lowercase();
    let suffix = suffix.strip_suffix("mt").unwrap_or(&suffix);
    match suffix {
        "bold" | "bolditalic" | "boldoblique" | "semibold" | "demibold" | "extrabold" | "black"
        | "heavy" => Some(true),
        "regular" | "normal" | "roman" | "book" | "light" | "medium" | "italic" | "oblique" => {
            Some(false)
        }
        _ => None,
    }
}
