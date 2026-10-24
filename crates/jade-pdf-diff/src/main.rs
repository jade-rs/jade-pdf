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

fn main() {
    let code = match dispatch() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            2
        }
    };
    std::process::exit(code);
}

fn dispatch() -> anyhow::Result<i32> {
    use pdf_diff::isolation::{run_pdf_worker, ProcessLimits, WORKER_FLAG};
    use std::io::Write;
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if args.as_slice() == [std::ffi::OsString::from(pdf_diff::isolation::REQUEST_FLAG)] {
        pdf_diff::isolation::watch_parent()?;
        return pdf_diff::isolation::execute_request();
    }
    if args.first().is_some_and(|a| a == WORKER_FLAG) {
        pdf_diff::isolation::watch_parent()?;
        return run(&args[1..]);
    }
    let mut limits = ProcessLimits::default();
    while let Some(flag) = args.first().and_then(|a| a.to_str()) {
        let kind = match flag {
            "--timeout-ms" => 0,
            "--max-memory-mb" => 1,
            "--max-output-mb" => 2,
            _ => break,
        };
        let value = args
            .get(1)
            .and_then(|v| v.to_str())
            .ok_or_else(|| anyhow::anyhow!("Missing resource limit value"))?
            .parse::<u64>()?;
        let bytes = || {
            value
                .checked_mul(1024 * 1024)
                .ok_or_else(|| anyhow::anyhow!("Resource limit overflow"))
        };
        match kind {
            0 => limits.timeout = std::time::Duration::from_millis(value),
            1 => limits.max_memory_bytes = bytes()?,
            _ => limits.max_output_bytes = bytes()?,
        }
        args.drain(..2);
    }
    limits.validate()?;
    let output = run_pdf_worker(&std::env::current_exe()?, &args, &limits)?;
    std::io::stdout().write_all(&output.stdout)?;
    std::io::stderr().write_all(&output.stderr)?;
    Ok(output.exit_code)
}

fn run(arguments: &[std::ffi::OsString]) -> anyhow::Result<i32> {
    let strings = arguments
        .iter()
        .map(|a| {
            a.to_str()
                .ok_or_else(|| anyhow::anyhow!("CLI arguments must be valid UTF-8"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let args = strings;

    if args.as_slice() == ["--licenses"] {
        print!(
            "{}\n{}",
            pdf_diff::PDFIUM_LICENSE_NOTICES,
            pdf_diff::ADOBE_GLYPH_LIST_LICENSE
        );
        return Ok(0);
    }

    if let ["--ir", path] = args.as_slice() {
        let document = load_document_blocking(&PathBuf::from(path), &Limits::default())?;
        println!("{}", serde_json::to_string_pretty(&document)?);
        return Ok(0);
    }

    if let ["--render", path, page, width, out] = args.as_slice() {
        let rendered = pdf_diff::render_page(&PathBuf::from(path), page.parse()?, width.parse()?)?;
        std::fs::write(out, &rendered.png)?;
        println!("{}x{} -> {out}", rendered.width, rendered.height);
        return Ok(0);
    }

    if let ["--text-layer", path, page] = args.as_slice() {
        let runs = pdf_diff::text_layer(&PathBuf::from(path), page.parse()?)?;
        println!("{}", serde_json::to_string_pretty(&runs)?);
        return Ok(0);
    }

    let [old, new] = args.as_slice() else {
        eprintln!("usage: pdf-diff <old.pdf> <new.pdf>");
        eprintln!("       pdf-diff --ir <doc.pdf>");
        eprintln!("       pdf-diff --render <doc.pdf> <page> <width> <out.png>");
        eprintln!("       pdf-diff --text-layer <doc.pdf> <page>");
        eprintln!("       pdf-diff --licenses");
        eprintln!("Resource options before the command: --timeout-ms 120000 --max-memory-mb 2048 --max-output-mb 64");
        return Ok(2);
    };

    let diff = diff_files(
        &PathBuf::from(old),
        &PathBuf::from(new),
        &Limits::default(),
        &DiffOptions::default(),
    )?;
    println!("{}", serde_json::to_string_pretty(&diff)?);
    if !diff.is_comparison_complete() {
        eprintln!("Comparison needs review: rendering, geometry, alignment or interactive semantics remains unverified. Review diagnostics and comparison coverage before treating this comparison as complete.");
        return Ok(3);
    }
    if diff.has_changes() {
        return Ok(1);
    }
    Ok(0)
}
