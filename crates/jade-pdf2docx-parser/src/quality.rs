use std::fmt::{Display, Formatter};

use crate::{ParseLimits, ParsedDocument};

#[cfg(test)]
#[path = "ocr_diagnostic_budget_tests.rs"]
mod diagnostic_budget_tests;

/// 資源上限と品質欠落の扱いを指定する解析設定。
#[derive(Debug, Clone, Default)]
pub struct ParseOptions {
    /// 入力サイズや stream 展開などの資源上限。
    pub limits: ParseLimits,
    /// 品質欠落が報告された場合に解析結果を拒否する。
    pub strict: bool,
    /// 他のthreadと共有する任意の協調キャンセル要求。未指定では停止要求を検査しない。
    pub cancellation: Option<crate::ParseCancellation>,
}

/// 変換で検出した品質欠落。空でも原本との完全一致を保証するものではない。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QualityReport {
    /// 検出順の品質欠落。画像置換/修復はobject、Font/XObjectはpage/object、幾何/inline/OCRはpageごとに記録する。
    pub issues: Vec<QualityIssue>,
}

/// 解析結果に含まれる品質欠落の種類。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualityIssue {
    /// 曲線・複合・非凸または64頂点を超えるpath clipの可視性を検証できない。
    UnsupportedClippingPath {
        /// 0始まりのページ番号。同じページに一度だけ記録する。
        page: usize,
    },
    /// MediaBoxを取得できず、ページ寸法・表示座標を確認できない。
    MissingPageGeometry {
        /// 0始まりの元ページ番号。後続の既知ページを詰めて番号変更しない。
        page: usize,
    },
    /// 文字描画に使った Font の glyph 内容を現行 extractor が再現できない。
    UnsupportedFont {
        /// 0 始まりのページ番号。
        page: usize,
        /// alias 解決後の Font object 番号。直接辞書では未指定。
        object_number: Option<u32>,
        /// 任意長の Subtype 名を複製しない固定理由。
        reason: UnsupportedFontReason,
    },
    /// 描画された XObject の内容を現行 extractor が再現できない。
    UnsupportedXObject {
        /// 0 始まりのページ番号。
        page: usize,
        /// 多段参照を解決した最終 XObject 番号。
        object_number: u32,
        /// 未対応の XObject 種別。任意長の名前は report へ複製しない。
        reason: UnsupportedXObjectReason,
    },
    /// inline image を含む content の残りを現行 extractor が検証できない。
    UnsupportedInlineImage {
        /// 0 始まりのページ番号。同じページに一度記録する。
        page: usize,
    },
    /// 元画像を decode できず、代替の白画像を埋め込んだ。
    ImageReplaced {
        /// 元画像の PDF object 番号。抽出できなければ `None`。
        object_number: Option<u32>,
        /// 画像を置換した原因。
        reason: ImageReplacementReason,
    },
    /// 許容されたCCITT破損行を前の正常行または白行で補完した。
    ImageRepaired {
        /// 元画像のPDF object番号。外部maskの補完も元画像へ集約する。
        object_number: Option<u32>,
        /// 元画像とmaskで補完した破損行の合計。
        damaged_rows: u64,
    },
    /// 可視画像内の本文の完全性を確認できず、OCRまたはrenderの確認が必要。
    OcrRequired {
        /// 0 始まりのページ番号。部分的な文字レイヤだけでは解除しない。
        page: usize,
    },
}

/// 最終 IR の文字と画像の可視範囲により、画像ページの OCR 診断を更新する。
///
/// 候補は実際の Image XObject 描画から記録する。配置を推測した画像には診断を追加しない。
/// 候補集合の容量を共有予算へ加算し、失敗時は既存診断を変更しない。
pub(crate) fn refresh_ocr_diagnostics(
    budget: &crate::decode::DecodeBudget,
    parsed: &mut ParsedDocument,
) -> Result<(), crate::ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let mut visible_image_pages = crate::metadata_workspace::Set::default();
    for page in parsed
        .image_elements
        .iter()
        .filter(|image| bounds_are_visible(parsed, image.page, image.bounds))
        .map(|image| image.page)
    {
        visible_image_pages.insert(budget, page)?;
    }
    parsed.quality.issues.retain(|issue| match issue {
        QualityIssue::OcrRequired { page } => visible_image_pages.contains(page),
        QualityIssue::MissingPageGeometry { .. }
        | QualityIssue::ImageReplaced { .. }
        | QualityIssue::ImageRepaired { .. }
        | QualityIssue::UnsupportedFont { .. }
        | QualityIssue::UnsupportedXObject { .. }
        | QualityIssue::UnsupportedInlineImage { .. }
        | QualityIssue::UnsupportedClippingPath { .. } => true,
    });
    Ok(())
}

/// 有効なページの表示範囲に有限で面積のある矩形が交差するか判定する。
fn bounds_are_visible(parsed: &ParsedDocument, page: usize, bounds: crate::ParsedBounds) -> bool {
    bounds_are_visible_on_pages(&parsed.pages, page, bounds)
}

/// ページ幾何だけを参照して可視範囲との交差を判定する。
fn bounds_are_visible_on_pages(
    pages: &[crate::ParsedPage],
    page: usize,
    bounds: crate::ParsedBounds,
) -> bool {
    let Some(page) = pages
        .get(page)
        .filter(|candidate| candidate.page_number == page)
        .or_else(|| pages.iter().find(|candidate| candidate.page_number == page))
    else {
        return false;
    };
    [
        bounds.x0,
        bounds.x1,
        bounds.y0,
        bounds.y1,
        page.width,
        page.height,
    ]
    .iter()
    .all(|value| value.is_finite())
        && bounds.x0.min(bounds.x1).max(0.0) < bounds.x0.max(bounds.x1).min(page.width)
        && bounds.y0.min(bounds.y1).max(0.0) < bounds.y0.max(bounds.y1).min(page.height)
}

/// 有限で面積のある可視文字か判定する。描画色や隠し文字の意味までは保証しない。
pub fn text_is_visible(parsed: &ParsedDocument, text: &crate::ParsedTextElement) -> bool {
    text.text
        .chars()
        .any(|ch| !ch.is_whitespace() && !ch.is_control())
        && bounds_are_visible(parsed, text.page, text.bounds)
}

/// 可視画像の本文を確認する必要があるか判定する。
/// 交差する文字レイヤがあっても、画像本文の完全性は証明できない。
pub fn image_requires_ocr(parsed: &ParsedDocument, image: &crate::ParsedImageElement) -> bool {
    bounds_are_visible(parsed, image.page, image.bounds)
}

/// 文字描画に使った Font を再現できない理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnsupportedFontReason {
    /// Type3 の CharProc と局所 resource による glyph 描画を再現していない。
    Type3,
    /// その他の未対応 Subtype。CIDFont を直接選択する入力も含む。
    Subtype,
}

impl UnsupportedFontReason {
    /// CLI・JSON で利用する安定した理由識別子を返す。
    pub fn code(self) -> &'static str {
        match self {
            Self::Type3 => "type3",
            Self::Subtype => "unsupported_subtype",
        }
    }
}

/// XObject 描画を再現できない理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedXObjectReason {
    /// Form の画像・罫線・clip・独自 Resources を完全には再現していない。
    Form,
    /// その他の Subtype または現行 Image extractor が読めない間接 Subtype。
    Subtype,
}

impl UnsupportedXObjectReason {
    /// CLI・JSON で利用する安定した理由識別子を返す。
    pub fn code(self) -> &'static str {
        match self {
            Self::Form => "form",
            Self::Subtype => "unsupported_subtype",
        }
    }
}

/// 白画像へ置換した原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageReplacementReason {
    /// 旧版の20画像制限による置換理由。互換用であり現在の解析では発行しない。
    ImageCountLimit,
    /// 旧版の非JPEG 400万画素制限による置換理由。互換用であり現在は発行しない。
    RawImagePixelLimit,
    /// 画像の filter に対応した decoder がない。
    UnsupportedFilter,
    /// 対応 decoder が画像を復元できなかった。
    DecodeFailed,
}

impl ImageReplacementReason {
    /// CLI や JSON で原因を区別する安定した識別子を返す。
    pub fn code(self) -> &'static str {
        match self {
            Self::ImageCountLimit => "image_count_limit",
            Self::RawImagePixelLimit => "raw_image_pixel_limit",
            Self::UnsupportedFilter => "unsupported_filter",
            Self::DecodeFailed => "decode_failed",
        }
    }
}

impl Display for QualityIssue {
    /// 欠落理由と object 番号または1始まりのページ番号を表示する。
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPageGeometry { page } => write!(
                f,
                "missing page geometry: page {} (missing_page_geometry; MediaBox is unavailable)",
                page.saturating_add(1)
            ),
            Self::UnsupportedFont {
                page,
                object_number,
                reason,
            } => {
                write!(
                    f,
                    "unsupported font: page {} object ",
                    page.saturating_add(1)
                )?;
                match object_number {
                    Some(number) => write!(f, "{number}")?,
                    None => write!(f, "unknown")?,
                }
                write!(f, " (unsupported_font; {})", reason.code())
            }
            Self::UnsupportedXObject {
                page,
                object_number,
                reason,
            } => write!(
                f,
                "unsupported XObject: page {} object {} ({reason:?}; unsupported_xobject; {})",
                page.saturating_add(1),
                object_number,
                reason.code()
            ),
            Self::UnsupportedClippingPath { page } => write!(
                f,
                "unsupported clipping path: page {} (unsupported_clipping_path; visibility requires review)",
                page.saturating_add(1)
            ),
            Self::UnsupportedInlineImage { page } => write!(
                f,
                "unsupported inline image: page {} (unsupported_inline_image; remaining content requires review)",
                page.saturating_add(1)
            ),
            Self::ImageReplaced {
                object_number,
                reason,
            } => {
                write!(f, "image replaced: object ")?;
                match object_number {
                    Some(number) => write!(f, "{number}")?,
                    None => write!(f, "unknown")?,
                }
                write!(f, " ({})", reason.code())
            }
            Self::ImageRepaired {
                object_number,
                damaged_rows,
            } => {
                write!(f, "image repaired: object ")?;
                match object_number {
                    Some(number) => write!(f, "{number}")?,
                    None => write!(f, "unknown")?,
                }
                write!(f, " (image_repaired; {damaged_rows} damaged CCITT rows)")
            }
            Self::OcrRequired { page } => write!(
                f,
                "unverified image text: page {} (ocr_required; OCR or render review required)",
                page.saturating_add(1)
            ),
        }
    }
}
