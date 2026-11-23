//! 一要素配列で表された既知のDevice色空間だけを、割当なしで選択する。
use std::ops::Range;

/// 余分な要素や未知の色空間を推測せず、元byte内のDevice名の範囲を返す。
pub(crate) fn device_singleton(bytes: &[u8]) -> Option<Range<usize>> {
    let mut entries = crate::metadata::array_entries(bytes, 1)?;
    let name = entries.next()??;
    if entries.next().is_some()
        || ![b"DeviceRGB".as_slice(), b"DeviceGray", b"DeviceCMYK"]
            .iter()
            .any(|device| crate::metadata::is_name(name, device))
    {
        return None;
    }
    let start = (name.as_ptr() as usize).checked_sub(bytes.as_ptr() as usize)?;
    Some(start..start.checked_add(name.len())?)
}
