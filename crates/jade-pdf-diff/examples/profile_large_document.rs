//! 大きめの PDF 相当の IR で差分計算の時間を測る。
//!
//! ```sh
//! cargo run --release -p structural-pdf-diff --example profile_large_document -- 200
//! ```
//!
//! 引数はページ数 (既定 200)。IR を直接組み立てるので、抽出の時間は含まない。
//! 新しい側は冒頭に 1 文を足して以降を流し直した想定で、どのページの本文も
//! 少しずつずれる。完全一致するページが無いので錨が付かず、ページの対応付けが
//! 総当たりになる。

use std::time::Instant;

use pdf_diff::{diff_documents, BlockIr, Bounds, DiffOptions, DocumentIr, PageIr};

const BLOCKS_PER_PAGE: usize = 12;
const SENTENCES_PER_BLOCK: usize = 12;

fn sentence(index: usize) -> String {
    format!(
        "第{}条の{}項は、契約の当事者が別途合意した事項を定める。",
        index / 7,
        index % 7
    )
}

/// 文の列をページとブロックに詰める。
fn document(sentences: &[String]) -> DocumentIr {
    let per_page = BLOCKS_PER_PAGE * SENTENCES_PER_BLOCK;
    let pages = sentences
        .chunks(per_page)
        .enumerate()
        .map(|(page, sentences)| PageIr {
            index: page + 1,
            width: 595.0,
            height: 842.0,
            rotation: 0,
            blocks: sentences
                .chunks(SENTENCES_PER_BLOCK)
                .enumerate()
                .map(|(block, sentences)| {
                    let y = 760.0 - block as f64 * 56.0;
                    BlockIr {
                        index: block + 1,
                        text: sentences.concat(),
                        bounds: Bounds {
                            x0: 56.0,
                            y0: y,
                            x1: 539.0,
                            y1: y + 48.0,
                        },
                        size: Some(10.5),
                    }
                })
                .collect(),
        })
        .collect();
    DocumentIr { pages }
}

fn main() {
    let pages: usize = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(200);
    let total = pages * BLOCKS_PER_PAGE * SENTENCES_PER_BLOCK;
    let old_sentences: Vec<String> = (0..total).map(sentence).collect();
    let mut new_sentences = old_sentences.clone();
    new_sentences.insert(
        3,
        "本契約は、前条の規定にかかわらず効力を有する。".to_string(),
    );
    new_sentences.pop();

    let old = document(&old_sentences);
    let new = document(&new_sentences);

    let started = Instant::now();
    let diff = diff_documents("old", "new", &old, &new, &DiffOptions::default());
    let elapsed = started.elapsed();
    println!("pages={pages} elapsed={elapsed:.2?}");
    println!("{:?}", diff.summary);
}
