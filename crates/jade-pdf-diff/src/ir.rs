//! PDF から読み取った中間表現。
//!
//! 差分計算と、UI 側でハイライトを重ねるための座標の両方に使う。
//!
//! 座標は **PDF ユーザ空間** (単位 pt、原点はページ左下、y は上向き) のまま
//! 持つ。pdf.js の viewport 変換に素直に渡せる形にしておきたいので、ここで
//! 画面座標へは直さない。

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// テキストブロックの外接矩形。`(x0, y0)` が左下、`(x1, y1)` が右上。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Bounds {
    /// pdf-parser のテキスト要素の `Bounds` は `[x, y, width, height]`。
    ///
    /// Adobe Extract の schema は [left, bottom, right, top] だが、取り込んだ
    /// パーサはテキストについてはこの並びで入れている
    /// (`pdf_operators_text.rs` の `element.Bounds = [x, y, width, height]`)。
    /// `y` は書き出し位置 (ベースライン) なので、上端は `y + height`。
    pub fn from_text_element(values: &[f64]) -> Option<Self> {
        if values.len() < 4 {
            return None;
        }
        let (x, y, width, height) = (values[0], values[1], values[2], values[3]);
        Some(Self {
            x0: x,
            y0: y,
            x1: x + width.abs(),
            y1: y + height.abs(),
        })
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    pub fn center(&self) -> (f64, f64) {
        ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }
}

/// ページ内のテキストブロック 1 つ。
///
/// pdf-parser が返す「意味のあるまとまり」(段落・見出し・表のセルなど) を
/// そのまま 1 ブロックとして扱う。行より粗く、ページより細かい粒度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockIr {
    /// ページ内での通し番号 (1 始まり)。
    pub index: usize,
    pub text: String,
    pub bounds: Bounds,
    /// 文字サイズ (pt)。分かれば。
    pub size: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageIr {
    /// 1 始まりのページ番号。
    pub index: usize,
    /// ページの大きさ (pt)。
    pub width: f64,
    pub height: f64,
    /// 表示時の回転角 (0 / 90 / 180 / 270)。
    pub rotation: u64,
    pub blocks: Vec<BlockIr>,
}

impl PageIr {
    /// ページの全テキスト。対応付けのスコアに使う。
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// ページ番号やヘッダ/フッタを除いた本文だけのテキスト。
    ///
    /// ページを 1 枚挟むと以降のページ番号がすべてずれる。それを込みで
    /// ページの同一性を判定すると、後続ページが軒並み「別のページ」に見えて
    /// しまうので、対応付けの鍵からは外す。
    pub fn body_text(&self) -> String {
        self.blocks
            .iter()
            .filter(|block| !self.is_furniture(block))
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// ページの上端 / 下端にある短い要素か (ページ番号・柱・透かしなど)。
    pub fn is_furniture(&self, block: &BlockIr) -> bool {
        // Pdfiumの寸法は回転後、文字座標は回転前。柱の判定も同じ空間で行う。
        let height = if self.rotation == 90 || self.rotation == 270 {
            self.width
        } else {
            self.height
        };
        if height <= 0.0 || block.text.chars().count() > FURNITURE_MAX_CHARS {
            return false;
        }
        let top = block.bounds.y0 > height * (1.0 - FURNITURE_BAND);
        let bottom = block.bounds.y1 < height * FURNITURE_BAND;
        top || bottom
    }
}

/// ページ端のどこまでを「ヘッダ/フッタ」とみなすか (ページ高さに対する割合)。
const FURNITURE_BAND: f64 = 0.08;
/// ページ端にあっても、これより長ければ本文として扱う。
///
/// 短めに取ってある。本文をうっかりヘッダ扱いすると、対応付けの鍵から
/// 本文が抜けてしまうため。ページ番号や短い柱はこの長さに収まる。
const FURNITURE_MAX_CHARS: usize = 24;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentIr {
    pub pages: Vec<PageIr>,
}

impl DocumentIr {
    /// OCRなど、外部から受け取る中間表現も部分比較や不正座標を許さない。
    pub fn validate(&self, limits: &crate::Limits) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.pages.is_empty() && self.pages.len() <= limits.max_pages,
            "OCR IR exceeds max_pages or has no pages"
        );
        let mut text_bytes = 0u64;
        for (index, page) in self.pages.iter().enumerate() {
            anyhow::ensure!(
                page.index == index + 1,
                "OCR page indices must be continuous"
            );
            anyhow::ensure!(
                page.width.is_finite()
                    && page.height.is_finite()
                    && page.width > 0.0
                    && page.height > 0.0
                    && page.width <= 20_000.0
                    && page.height <= 20_000.0,
                "Invalid OCR page geometry"
            );
            anyhow::ensure!(
                [0, 90, 180, 270].contains(&page.rotation),
                "Invalid OCR page rotation"
            );
            anyhow::ensure!(
                page.blocks.len() <= limits.max_blocks_per_page,
                "OCR IR exceeds max_blocks_per_page"
            );
            for (block_index, block) in page.blocks.iter().enumerate() {
                anyhow::ensure!(
                    block.index == block_index + 1,
                    "OCR block indices must be continuous"
                );
                let b = block.bounds;
                anyhow::ensure!(
                    [b.x0, b.y0, b.x1, b.y1].into_iter().all(f64::is_finite)
                        && b.x1 >= b.x0
                        && b.y1 >= b.y0,
                    "Invalid OCR block bounds"
                );
                anyhow::ensure!(
                    block.size.is_none_or(|size| size.is_finite() && size > 0.0),
                    "Invalid OCR font size"
                );
                text_bytes = text_bytes
                    .checked_add(block.text.len() as u64)
                    .ok_or_else(|| anyhow::anyhow!("OCR text size overflow"))?;
            }
        }
        crate::scan::ensure_size(text_bytes, limits)
    }
}

/// PDF 抽出器由来の、表示上は同じ文字列の揺れを整える。
///
/// PDF は改行位置や字間が描画都合で入るので、連続する空白は 1 つに畳み、
/// 前後は落とす。さらに、任意改行位置を示す soft hyphen を除き、OpenType の
/// 表示用合字と Unicode の合成形式を通常の文字列へ揃える。
pub(crate) fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        match ch {
            '\u{ad}' => {}
            'ﬀ' => out.push_str("ff"),
            'ﬁ' => out.push_str("fi"),
            'ﬂ' => out.push_str("fl"),
            'ﬃ' => out.push_str("ffi"),
            'ﬄ' => out.push_str("ffl"),
            'ﬅ' | 'ﬆ' => out.push_str("st"),
            other => out.push(other),
        }
    }
    out.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_come_from_origin_and_size() {
        // [x, y, width, height] → 左下 (60, 700) から 140x20 の矩形。
        let bounds = Bounds::from_text_element(&[60.0, 700.0, 140.0, 20.0]).unwrap();
        assert_eq!(bounds.x0, 60.0);
        assert_eq!(bounds.y0, 700.0);
        assert_eq!(bounds.x1, 200.0);
        assert_eq!(bounds.y1, 720.0);
        assert_eq!(bounds.width(), 140.0);
        assert_eq!(bounds.height(), 20.0);
    }

    #[test]
    fn bounds_need_four_values() {
        assert!(Bounds::from_text_element(&[1.0, 2.0, 3.0]).is_none());
    }

    fn page_with(blocks: Vec<(f64, f64, &str)>) -> PageIr {
        PageIr {
            index: 1,
            width: 595.0,
            height: 842.0,
            rotation: 0,
            blocks: blocks
                .into_iter()
                .enumerate()
                .map(|(index, (y0, y1, text))| BlockIr {
                    index: index + 1,
                    text: text.to_string(),
                    bounds: Bounds {
                        x0: 60.0,
                        y0,
                        x1: 300.0,
                        y1,
                    },
                    size: Some(11.0),
                })
                .collect(),
        }
    }

    #[test]
    fn furniture_is_short_text_at_the_page_edges() {
        let page = page_with(vec![
            (800.0, 812.0, "社外秘"),                             // ヘッダ
            (400.0, 412.0, "第1条 本文はページの中ほどにある。"), // 本文
            (30.0, 42.0, "- 5 -"),                                // フッタ
        ]);
        assert!(page.is_furniture(&page.blocks[0]));
        assert!(!page.is_furniture(&page.blocks[1]));
        assert!(page.is_furniture(&page.blocks[2]));
    }

    #[test]
    fn long_text_at_the_edge_is_body() {
        let page = page_with(vec![(
            30.0,
            42.0,
            "ページ下端にあるが、本文として扱うべき長さの段落がここに置かれている。",
        )]);
        assert!(!page.is_furniture(&page.blocks[0]));
    }

    #[test]
    fn body_text_leaves_out_the_page_number() {
        let page = page_with(vec![
            (800.0, 812.0, "社外秘"),
            (400.0, 412.0, "第1条 本文"),
            (30.0, 42.0, "- 5 -"),
        ]);
        assert_eq!(page.body_text(), "第1条 本文");
        assert!(page.text().contains("- 5 -"));
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize("  合計 \n  1,200 円 "), "合計 1,200 円");
        assert_eq!(normalize("\u{3000}全角\u{3000}空白"), "全角 空白");
    }

    #[test]
    fn normalize_removes_extraction_only_unicode_differences() {
        assert_eq!(normalize("Cafe\u{301}"), "Café");
        assert_eq!(normalize("of\u{ad}fice"), "office");
        assert_eq!(normalize("oﬃce ﬆaff"), "office staff");
    }
}
