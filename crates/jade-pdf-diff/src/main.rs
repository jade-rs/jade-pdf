//! 確認用の CLI。
//!
//! ```bash
//! pdf-diff old.pdf new.pdf              # 差分 JSON を標準出力へ (差分があれば終了コード 1)
//! pdf-diff --ir doc.pdf                 # 抽出したページ / ブロックを標準出力へ
//! pdf-diff --render doc.pdf 1 800 out.png  # ページ画像を書き出す
//! pdf-diff --text-layer doc.pdf 1       # テキストレイヤを標準出力へ
//! ```

use std::path::PathBuf;

use pdf_diff::{diff_files, load_document_blocking, DiffOptions, Limits};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    if let ["--ir", path] = args.as_slice() {
        let document = load_document_blocking(&PathBuf::from(path), &Limits::default())?;
        println!("{}", serde_json::to_string_pretty(&document)?);
        return Ok(());
    }

    if let ["--render", path, page, width, out] = args.as_slice() {
        let rendered = pdf_diff::render_page(&PathBuf::from(path), page.parse()?, width.parse()?)?;
        std::fs::write(out, &rendered.png)?;
        println!("{}x{} -> {out}", rendered.width, rendered.height);
        return Ok(());
    }

    if let ["--text-layer", path, page] = args.as_slice() {
        let runs = pdf_diff::text_layer(&PathBuf::from(path), page.parse()?)?;
        println!("{}", serde_json::to_string_pretty(&runs)?);
        return Ok(());
    }

    let [old, new] = args.as_slice() else {
        eprintln!("usage: pdf-diff <old.pdf> <new.pdf>");
        eprintln!("       pdf-diff --ir <doc.pdf>");
        eprintln!("       pdf-diff --render <doc.pdf> <page> <width> <out.png>");
        eprintln!("       pdf-diff --text-layer <doc.pdf> <page>");
        std::process::exit(2);
    };

    let diff = diff_files(
        &PathBuf::from(old),
        &PathBuf::from(new),
        &Limits::default(),
        &DiffOptions::default(),
    )?;
    println!("{}", serde_json::to_string_pretty(&diff)?);
    if diff.has_changes() {
        std::process::exit(1);
    }
    Ok(())
}
