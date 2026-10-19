//! UTF-8置換を一時配列なしで借用走査する。
/// UTF-8 の有効部分と標準の置換文字を、入力の借用だけで列挙する。
pub(crate) struct LossyChunks<'a> {
    /// まだ検査していない入力。
    remaining: &'a [u8],
    /// 有効部分の次に返す置換文字があるか。
    replacement_pending: bool,
}

impl<'a> LossyChunks<'a> {
    /// 一時配列を確保せずに走査を開始する。
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            remaining: bytes,
            replacement_pending: false,
        }
    }
}

impl<'a> Iterator for LossyChunks<'a> {
    type Item = &'a str;

    /// 標準の from_utf8_lossy と同じ不正シーケンス単位で置換する。
    fn next(&mut self) -> Option<Self::Item> {
        if self.replacement_pending {
            self.replacement_pending = false;
            return Some("\u{fffd}");
        }
        if self.remaining.is_empty() {
            return None;
        }
        match std::str::from_utf8(self.remaining) {
            Ok(text) => {
                self.remaining = &[];
                Some(text)
            }
            Err(error) => {
                let valid = error.valid_up_to();
                let invalid = error.error_len().unwrap_or(self.remaining.len() - valid);
                let prefix = &self.remaining[..valid];
                self.remaining = &self.remaining[valid + invalid..];
                if valid == 0 {
                    Some("\u{fffd}")
                } else {
                    self.replacement_pending = true;
                    std::str::from_utf8(prefix).ok()
                }
            }
        }
    }
}
