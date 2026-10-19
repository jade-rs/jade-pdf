/// 完全な非空RGB画像の全成分が255であることを確認した白紙の証明。
/// フィールドは非公開とし、空の認識結果だけから証明を作れない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBlankPage {
    /// 検証済み画像の横幅。
    width: usize,
    /// 検証済み画像の高さ。
    height: usize,
}

impl VerifiedBlankPage {
    /// 完全なRGB8画像が全白なら証明を返す。寸法overflowと余剰・不足も拒否する。
    pub fn from_rgb8(rgb: &[u8], width: usize, height: usize) -> Option<Self> {
        let length = width.checked_mul(height)?.checked_mul(3)?;
        (length > 0 && rgb.len() == length && rgb.iter().all(|value| *value == 255))
            .then_some(Self { width, height })
    }
}

/// 元の入力画像を左上原点とする画素座標付き認識行。終端画素は含まない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionedOcrLine {
    /// 前後の空白も保持した認識本文。
    pub text: String,
    /// 切り出し前の入力画像上の[x0,y0,x1,y1]。
    pub bounds: [usize; 4],
}

/// ページの認識本文と、画像を検証した白紙を区別する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrPageContent {
    /// 空行を含まない認識本文。文書へ保存する際にも検証する。
    Text(Vec<String>),
    /// 元画像上の行位置を保持した本文。文書へ保存する際にも座標を検証する。
    PositionedText(Vec<PositionedOcrLine>),
    /// 全白の画像として検証されたページ。本文容量を消費しない。
    Blank(VerifiedBlankPage),
}
