use std::{fs, process::Command};

#[allow(dead_code)]
#[path = "../examples/support/minimum_pdf_to_docx_with_ocr_cli.rs"]
mod cli;
mod common;

use cli::{
    CLI_HINT_USE_HELP_FOR_FULL_OPTIONS, CLI_USAGE_LINE, ERR_MIN_CONFIDENCE_MUST_BE_FLOAT,
    ERR_MIN_CONFIDENCE_OUT_OF_RANGE, ERR_OCR_PDF_DPI_MUST_BE_POSITIVE_INT,
    ERR_OCR_TABLE_PIPELINE_REQUIRES_IMAGE_SOURCE, ERR_OCR_TABLE_SOURCES_CONFLICT,
    ERR_PARSEQ_FAILURE_POLICY_REQUIRES_PATHS, ERR_PARSEQ_OPTIONS_MUST_BE_PAIRED,
    fmt_duplicate_option, fmt_missing_value_for_option, fmt_unknown_option,
};
use common::expected_minimum_ocr_help_output;

fn documented_error_fragments() -> Vec<String> {
    vec![
        CLI_USAGE_LINE.to_string(),
        ERR_PARSEQ_OPTIONS_MUST_BE_PAIRED.to_string(),
        ERR_PARSEQ_FAILURE_POLICY_REQUIRES_PATHS.to_string(),
        ERR_MIN_CONFIDENCE_MUST_BE_FLOAT.to_string(),
        ERR_MIN_CONFIDENCE_OUT_OF_RANGE.to_string(),
        ERR_OCR_PDF_DPI_MUST_BE_POSITIVE_INT.to_string(),
        "does not take a value".to_string(),
        ERR_OCR_TABLE_SOURCES_CONFLICT.to_string(),
        ERR_OCR_TABLE_PIPELINE_REQUIRES_IMAGE_SOURCE.to_string(),
        fmt_missing_value_for_option("--ocr-image"),
        fmt_duplicate_option("--ocr-image"),
        fmt_unknown_option("--unknown-option"),
        CLI_HINT_USE_HELP_FOR_FULL_OPTIONS.to_string(),
    ]
}

fn normalized_command_tokens(input: &str) -> Vec<String> {
    let replaced = input.replace('\\', " ");
    let tokens = replaced
        .split_whitespace()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut normalized = Vec::new();
    let mut idx = 0usize;
    while idx < tokens.len() {
        if tokens[idx] == "--features" && tokens.get(idx + 1).map(String::as_str) == Some("ocr") {
            idx += 2;
            continue;
        }
        normalized.push(tokens[idx].clone());
        idx += 1;
    }
    normalized
}

fn contains_token_subsequence(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut cursor = 0usize;
    for token in haystack {
        if *token == needle[cursor] {
            cursor += 1;
            if cursor == needle.len() {
                return true;
            }
        }
    }
    false
}

fn required_help_option_tokens() -> Vec<String> {
    let mut tokens = Vec::new();
    for line in expected_minimum_ocr_help_output().lines() {
        let trimmed = line.trim_start();
        if let Some(option) = trimmed.strip_prefix("--") {
            if let Some(name) = option.split_whitespace().next() {
                tokens.push(format!("--{name}"));
            }
            continue;
        }
        if trimmed.starts_with("-h, --help") {
            tokens.push("-h".to_string());
            tokens.push("--help".to_string());
        }
    }
    tokens
}

#[test]
fn minimum_ocr_example_doc_comments_are_covered_by_cli_doc_examples() {
    let example_src = fs::read_to_string("examples/minimum_pdf_to_docx_with_ocr.rs")
        .expect("example source should be readable");
    let doc = fs::read_to_string("docs/minimum_pdf_to_docx_with_ocr_cli.md")
        .expect("CLI specification document must be readable");
    let normalized_doc_tokens = normalized_command_tokens(&doc);

    let comment_commands = example_src
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let inner = trimmed.strip_prefix("//! `")?.strip_suffix('`')?;
            if inner.starts_with("cargo run ") {
                Some(inner.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    assert!(
        !comment_commands.is_empty(),
        "example doc comments should include runnable cargo commands"
    );

    for command in comment_commands {
        let normalized_command = normalized_command_tokens(&command);
        assert!(
            contains_token_subsequence(&normalized_doc_tokens, &normalized_command),
            "CLI document should cover command shown in example doc comments: {command}"
        );
    }
}

#[test]
fn minimum_ocr_example_shows_help_and_exits_success() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--example",
            "minimum_pdf_to_docx_with_ocr",
            "--",
            "--help",
        ])
        .output()
        .expect("failed to run help option");

    assert!(
        output.status.success(),
        "help option should exit successfully"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.as_ref(),
        expected_minimum_ocr_help_output(),
        "help output snapshot must stay stable"
    );
}

#[test]
fn minimum_ocr_example_shows_short_help_and_exits_success() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--example",
            "minimum_pdf_to_docx_with_ocr",
            "--",
            "-h",
        ])
        .output()
        .expect("failed to run short help option");

    assert!(
        output.status.success(),
        "short help option should exit successfully"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.as_ref(),
        expected_minimum_ocr_help_output(),
        "short help output snapshot must match long help output"
    );
}

#[test]
fn minimum_ocr_cli_doc_covers_help_contract() {
    let doc = fs::read_to_string("docs/minimum_pdf_to_docx_with_ocr_cli.md")
        .expect("CLI specification document must be readable");
    let help = expected_minimum_ocr_help_output();
    let usage_line = help
        .lines()
        .find(|line| line.trim_start().starts_with("cargo run --example"))
        .expect("help output must contain usage line");

    assert!(
        doc.contains(usage_line.trim()),
        "CLI document should include usage signature"
    );

    for token in required_help_option_tokens() {
        assert!(
            doc.contains(&token),
            "CLI document should include option token in help contract: {token}"
        );
    }
    assert!(
        doc.contains("`--option value` と `--option=value` の両形式"),
        "CLI document should include dual option syntax note"
    );
    assert!(
        doc.contains("--ocr-parseq-model` と `--ocr-charset` は必ず同時指定"),
        "CLI document should include parseq paired-option requirement"
    );
    assert!(
        doc.contains("## 7. 実行例"),
        "CLI document should include usage examples section"
    );
    for fragment in documented_error_fragments() {
        assert!(
            doc.contains(&fragment),
            "CLI document should include representative stderr fragment: {fragment}"
        );
    }
    assert!(
        doc.contains("# OCR mock 行を利用"),
        "CLI document should include successful mock example"
    );
    assert!(
        doc.contains("# parseq 片側指定（失敗）"),
        "CLI document should include failing parseq partial-option example"
    );
    assert!(
        doc.contains("# fail-fast 指定時: image OCR が失敗したら mock にフォールバック"),
        "CLI document should include fail-fast fallback example"
    );
}
