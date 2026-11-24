//! 本文と実段組の寸法を、梱包まで型付きで一緒に保持する。
use docx_rs::Docx;

/// docx-rsが直接表せない段幅を伴う、梱包前の文書。
pub(super) struct PreparedDocx {
    /// 通常のWord本文と関連部品。
    pub(super) docx: Docx,
    /// 本文XMLに出現する二段section順の実段領域。
    pub(super) column_frames: Vec<crate::column_frame::Frame>,
}

impl PreparedDocx {
    /// 段組を出力した順序のまま、実段幅と本文を束ねる。
    pub(super) fn new(docx: Docx, column_frames: Vec<crate::column_frame::Frame>) -> Self {
        Self {
            docx,
            column_frames,
        }
    }
}

impl From<Docx> for PreparedDocx {
    /// OCRなど通常のWord文書は段幅の補正を要求しない。
    fn from(docx: Docx) -> Self {
        Self::new(docx, Vec::new())
    }
}
