use std::{
    fmt::{Display, Formatter},
    path::PathBuf,
};

/// 最小 PDF 解析結果を表す。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedDocument {
    /// 解析中に検出した画像置換などの品質欠落。
    pub quality: crate::QualityReport,
    /// 解析対象のファイルパス。
    pub path: PathBuf,
    /// 入力ファイルのバイト長。
    pub byte_len: usize,
    /// 入力先頭 8 文字のヘッダ。
    pub header: String,
    /// headerと正式なRoot CatalogのVersionのうち上位の実効PDFバージョン。
    pub version: PdfVersion,
    /// PDF 内で検出したページ数。
    pub page_count: usize,
    /// 寸法を確認できたページ情報。欠落ページは品質診断へ記録し、元ページ番号を維持する。
    pub pages: Vec<ParsedPage>,
    /// PDF content stream から抽出した線分 graphics。
    pub graphics: Vec<ParsedGraphicElement>,
    /// PDF content stream から抽出した座標付きテキスト。
    pub text_elements: Vec<ParsedTextElement>,
    /// PDF 内で検出した画像要素。
    pub image_elements: Vec<ParsedImageElement>,
    /// PDFに埋め込まれ、明示的な埋め込み禁止がない再利用可能なTrueType subset font。
    pub embedded_fonts: Vec<ParsedEmbeddedFont>,
    /// PDFバイト列から抽出した最小テキストプレビュー。
    pub text_preview: String,
}

/// PDF の `FontFile2` stream から抽出した埋め込み TrueType font を表す。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedEmbeddedFont {
    /// PDF の subset prefix を含む BaseFont 名。
    pub font_name: String,
    /// 展開済み TrueType/OpenType font バイト列。
    pub bytes: Vec<u8>,
}

/// PDF 1 ページ分の最小ページ情報を表す。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedPage {
    /// 0 始まりのページ番号。
    pub page_number: usize,
    /// ページ幅（PDF user unit）。
    pub width: f32,
    /// ページ高さ（PDF user unit）。
    pub height: f32,
    /// ページ回転角度。
    pub rotation: i32,
}

/// PDF content stream から抽出した線分 graphics を表す。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedGraphicElement {
    /// 対象ページ番号。
    pub page: usize,
    /// 線分の外接 bounds。
    pub bounds: ParsedBounds,
    /// stroke 幅。filled rectangle 由来の場合は推定幅。
    pub stroke_width: f32,
}

/// PDF content stream から抽出したテキスト要素を表す。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTextElement {
    /// 対象ページ番号。
    pub page: usize,
    /// テキストのおおまかな外接 bounds。
    pub bounds: ParsedBounds,
    /// PDF text state 由来の font size（pt）。
    pub font_size: f32,
    /// PDF font resource から解決した BaseFont 名。
    pub font_name: Option<String>,
    /// PDF fill color から解決した RGB hex 色（例: `FF0000`）。
    pub fill_color: Option<String>,
    /// 抽出した Unicode テキスト。
    pub text: String,
}

/// PDF から抽出した画像要素を表す。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedImageElement {
    /// 対象ページ番号。
    pub page: usize,
    /// 画像のおおまかな配置 bounds。
    pub bounds: ParsedBounds,
    /// DOCX に埋め込む PNG バイト列。
    pub png_bytes: Vec<u8>,
    /// 表示幅（px）。
    pub width_px: u32,
    /// 表示高さ（px）。
    pub height_px: u32,
    /// DOCX で floating picture として配置するか。
    pub floating: bool,
    /// CTM から推定した時計回り回転角度。
    pub rotation_degrees: u16,
}

/// PDF 要素の矩形 bounds を表す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParsedBounds {
    /// 始点または左端の x 座標。
    pub x0: f32,
    /// 始点または下端の y 座標。
    pub y0: f32,
    /// 終点または右端の x 座標。
    pub x1: f32,
    /// 終点または上端の y 座標。
    pub y1: f32,
}

/// PDF 解析時のエラーを表す。
#[derive(Debug)]
pub enum ParseError {
    /// 呼び出し元が解析をキャンセルし、部分結果を破棄した。
    Cancelled,
    /// strict mode が品質欠落を拒否した。
    QualityRejected {
        /// エラー対象ファイルパス。
        path: PathBuf,
        /// 拒否の原因となった品質欠落。
        report: crate::QualityReport,
    },
    /// ファイル I/O 失敗。
    Io(std::io::Error),
    /// ToUnicode の範囲演算が表現可能な値を超えた。
    InvalidToUnicode {
        /// 不正と判断した理由。
        reason: &'static str,
    },
    /// 設定された解析上限を超過した。
    ResourceLimit {
        /// 超過した資源名。
        resource: &'static str,
        /// 許可する最大値。
        limit: u64,
        /// 超過を検出した時点の値。
        actual: u64,
    },
    /// `%PDF-` ヘッダが存在しない。
    InvalidPdfHeader {
        /// エラー対象ファイルパス。
        path: PathBuf,
    },
    /// `%PDF-x.y` バージョン形式が不正。
    InvalidPdfVersion {
        /// エラー対象ファイルパス。
        path: PathBuf,
        /// 取得したヘッダ文字列。
        header: String,
    },
    /// 最小限の PDF object 構造を満たさない。
    InvalidPdfStructure {
        /// エラー対象ファイルパス。
        path: PathBuf,
        /// 不正と判断した理由。
        reason: &'static str,
    },
    /// パスワードを受け取らない入口では暗号化 PDF を処理できない。
    UnsupportedEncryption {
        /// エラー対象ファイルパス。
        path: PathBuf,
    },
    /// Pdfium の初期化または解析に失敗した。
    Pdfium {
        /// エラー対象ファイルパス。
        path: PathBuf,
        /// Pdfium 側から得たエラー文字列。
        message: String,
    },
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Cancelled => write!(f, "pdf parsing cancelled"),
            ParseError::QualityRejected { path, report } => {
                write!(
                    f,
                    "quality rejected: {} ({} issue(s))",
                    path.display(),
                    report.issues.len()
                )?;
                for issue in &report.issues {
                    write!(f, "; {issue}")?;
                }
                Ok(())
            }
            ParseError::Io(err) => write!(f, "io error: {err}"),
            ParseError::InvalidToUnicode { reason } => {
                write!(f, "invalid ToUnicode CMap: {reason}")
            }
            ParseError::ResourceLimit {
                resource,
                limit,
                actual,
            } => {
                write!(
                    f,
                    "resource limit exceeded: {resource} ({actual} > {limit})"
                )
            }
            ParseError::InvalidPdfHeader { path } => {
                write!(f, "invalid pdf header: {}", path.display())
            }
            ParseError::InvalidPdfVersion { path, header } => {
                write!(
                    f,
                    "invalid pdf version header: {} ({header})",
                    path.display()
                )
            }
            ParseError::Pdfium { path, message } => {
                write!(f, "pdfium parse error: {} ({message})", path.display())
            }
            ParseError::InvalidPdfStructure { path, reason } => {
                write!(f, "invalid pdf structure: {} ({reason})", path.display())
            }
            ParseError::UnsupportedEncryption { path } => {
                write!(f, "encrypted pdf is unsupported: {}", path.display())
            }
        }
    }
}

impl std::error::Error for ParseError {}

impl From<std::io::Error> for ParseError {
    fn from(value: std::io::Error) -> Self {
        ParseError::Io(value)
    }
}

/// PDF バージョン（major.minor）を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfVersion {
    /// メジャーバージョン。
    pub major: u8,
    /// マイナーバージョン。
    pub minor: u8,
}
