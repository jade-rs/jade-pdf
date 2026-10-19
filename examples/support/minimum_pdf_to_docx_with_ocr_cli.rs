/// CLI 解析結果を保持する。
///
/// `ocr` feature を無効にしてビルドした example では、`collect_optional_ocr_lines` が
/// フィールドを参照しないため、未読フィールド警告を抑止する。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CliArgs {
    /// 入力 PDF パス。
    pub input_pdf: String,
    /// 出力 DOCX パス。
    pub output_docx: String,
    /// `--ocr-mock` の値。
    pub ocr_mock: Option<String>,
    /// `--ocr-image` の値。
    pub ocr_image: Option<String>,
    /// `--ocr-parseq-model` の値。
    pub ocr_parseq_model: Option<String>,
    /// `--ocr-charset` の値。
    pub ocr_charset: Option<String>,
    /// `--ocr-parseq-failure-policy` の値。
    pub ocr_parseq_failure_policy: Option<ParseqFailurePolicyArg>,
    /// `--ocr-min-confidence` の値。
    pub ocr_min_confidence: Option<f32>,
    /// `--ocr-from-pdf` が指定されたか。
    pub ocr_from_pdf: bool,
    /// `--ocr-auto` が指定されたか。
    pub ocr_auto: bool,
    /// `--ocr-pdf-dpi` の値（未指定時は 150）。
    pub ocr_pdf_dpi: u32,
    /// `--clean-output` が指定されたか。
    pub clean_output: bool,
    /// `--ocr-table-json` のパス（Phase 1: 外部パイプライン JSON）。
    pub ocr_table_json: Option<String>,
    /// `--ocr-table-geometry-json` のパス（Phase 2: セル + テキスト bbox JSON）。
    pub ocr_table_geometry_json: Option<String>,
    /// `--ocr-table-pipeline` の実行ファイルパス。
    pub ocr_table_pipeline: Option<String>,
}

#[derive(Debug, Default)]
struct ParsedOptionalArgs {
    ocr_mock: Option<String>,
    ocr_image: Option<String>,
    ocr_parseq_model: Option<String>,
    ocr_charset: Option<String>,
    ocr_parseq_failure_policy: Option<String>,
    ocr_min_confidence: Option<String>,
    ocr_from_pdf: bool,
    ocr_auto: bool,
    clean_output: bool,
    ocr_pdf_dpi: Option<String>,
    ocr_table_json: Option<String>,
    ocr_table_geometry_json: Option<String>,
    ocr_table_pipeline: Option<String>,
}

/// PARSeq 失敗時ポリシーを表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseqFailurePolicyArg {
    /// 認識エラー時に fail-fast 扱いにする。
    FailFast,
    /// 認識エラー時に診断用 band label を返す。本文には使用しない。
    FallbackToBandLabel,
}

impl ParseqFailurePolicyArg {
    fn parse(value: &str) -> Result<Self, CliParseAction> {
        match value {
            "fail-fast" => Ok(Self::FailFast),
            "fallback" => Ok(Self::FallbackToBandLabel),
            _ => Err(CliParseAction::ShowError(
                ERR_PARSEQ_FAILURE_POLICY_MUST_BE_ALLOWED_VALUE.to_string(),
            )),
        }
    }
}

/// CLI 解析時の制御フローを表す。
#[derive(Debug)]
pub enum CliParseAction {
    /// ヘルプ表示で終了する。
    ShowHelp,
    /// エラーメッセージを表示して終了する。
    ShowError(String),
}

/// `cargo run` 形式の usage 行（引数エラー時に標準エラーへ出す）。
pub const CLI_USAGE_LINE: &str = "usage: cargo run --example minimum_pdf_to_docx_with_ocr -- <input.pdf> <output.docx> [options]";
/// 引数エラー時に標準エラーへ続けて出すヒント行。
pub const CLI_HINT_USE_HELP_FOR_FULL_OPTIONS: &str = "hint: use --help for full options";

/// usage を標準エラーへ出力する。
pub fn print_usage_to_stderr() {
    eprintln!("{CLI_USAGE_LINE}");
}

/// 詳細ヘルプを標準出力へ出力する。
pub fn print_help() {
    print!("{HELP_TEXT}");
}

/// `--help` 出力本文。
pub const HELP_TEXT: &str = include_str!("minimum_pdf_to_docx_with_ocr_help.txt");
/// `--ocr-parseq-model` と `--ocr-charset` の同時指定エラーメッセージ。
pub const ERR_PARSEQ_OPTIONS_MUST_BE_PAIRED: &str =
    "--ocr-parseq-model and --ocr-charset must be provided together";
/// `--ocr-parseq-failure-policy` の依存関係エラーメッセージ。
pub const ERR_PARSEQ_FAILURE_POLICY_REQUIRES_PATHS: &str =
    "--ocr-parseq-failure-policy requires --ocr-parseq-model and --ocr-charset";
/// `--ocr-parseq-failure-policy` の許容値エラーメッセージ。
pub const ERR_PARSEQ_FAILURE_POLICY_MUST_BE_ALLOWED_VALUE: &str =
    "--ocr-parseq-failure-policy must be one of: fail-fast, fallback";
/// `--ocr-min-confidence` の浮動小数値要件エラーメッセージ。
pub const ERR_MIN_CONFIDENCE_MUST_BE_FLOAT: &str = "--ocr-min-confidence must be a float";
/// `--ocr-min-confidence` の範囲要件エラーメッセージ。
pub const ERR_MIN_CONFIDENCE_OUT_OF_RANGE: &str = "--ocr-min-confidence must be within [0.0, 1.0]";
/// `--ocr-pdf-dpi` の整数要件エラーメッセージ。
pub const ERR_OCR_PDF_DPI_MUST_BE_POSITIVE_INT: &str =
    "--ocr-pdf-dpi must be a positive integer (dpi for pdftoppm)";
/// `--ocr-table-pipeline` に画像ソースが無いときのエラー。
pub const ERR_OCR_TABLE_PIPELINE_REQUIRES_IMAGE_SOURCE: &str =
    "--ocr-table-pipeline requires --ocr-image or --ocr-from-pdf";
/// 表入力ソース（Phase 1 HTML JSON / Phase 2 幾何 JSON / 外部パイプライン）の同時指定エラー。
pub const ERR_OCR_TABLE_SOURCES_CONFLICT: &str = "OCR table options are mutually exclusive: --ocr-table-json, --ocr-table-geometry-json, --ocr-table-pipeline";
/// 必須位置引数（`<input.pdf> <output.docx>`）欠落のエラーメッセージ。
pub const ERR_MISSING_REQUIRED_POSITIONALS: &str =
    "missing required arguments: <input.pdf> <output.docx>";
/// `missing value for option:` 行のプレフィックス。
pub const ERR_MISSING_VALUE_FOR_OPTION_PREFIX: &str = "missing value for option: ";
/// `duplicate option:` 行のプレフィックス。
pub const ERR_DUPLICATE_OPTION_PREFIX: &str = "duplicate option: ";
/// `unknown option:` 行のプレフィックス。
pub const ERR_UNKNOWN_OPTION_PREFIX: &str = "unknown option: ";
/// `unexpected positional argument:` 行のプレフィックス。
pub const ERR_UNEXPECTED_POSITIONAL_PREFIX: &str = "unexpected positional argument: ";

/// オプション値欠落エラーメッセージを組み立てる。
///
/// `flag` は `--ocr-image` のようなフラグ名。
pub fn fmt_missing_value_for_option(flag: &str) -> String {
    format!("{ERR_MISSING_VALUE_FOR_OPTION_PREFIX}{flag}")
}

/// 同一オプション重複指定エラーメッセージを組み立てる。
pub fn fmt_duplicate_option(flag: &str) -> String {
    format!("{ERR_DUPLICATE_OPTION_PREFIX}{flag}")
}

/// 未知オプションエラーメッセージを組み立てる。
pub fn fmt_unknown_option(flag: &str) -> String {
    format!("{ERR_UNKNOWN_OPTION_PREFIX}{flag}")
}

/// 想定外の位置引数エラーメッセージを組み立てる。
pub fn fmt_unexpected_positional_argument(token: &str) -> String {
    format!("{ERR_UNEXPECTED_POSITIONAL_PREFIX}{token}")
}

/// 引数ベクタを CLI オプションへ変換する。
pub fn parse_cli_args(args: &[String]) -> Result<CliArgs, CliParseAction> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Err(CliParseAction::ShowHelp);
    }
    if args.len() < 3 {
        return Err(CliParseAction::ShowError(
            ERR_MISSING_REQUIRED_POSITIONALS.to_string(),
        ));
    }

    let extra_args = &args[3..];
    let parsed_optional = parse_optional_args(extra_args)?;

    if parsed_optional.ocr_parseq_model.is_some() ^ parsed_optional.ocr_charset.is_some() {
        return Err(CliParseAction::ShowError(
            ERR_PARSEQ_OPTIONS_MUST_BE_PAIRED.to_string(),
        ));
    }
    if parsed_optional.ocr_parseq_failure_policy.is_some()
        && (parsed_optional.ocr_parseq_model.is_none() || parsed_optional.ocr_charset.is_none())
    {
        return Err(CliParseAction::ShowError(
            ERR_PARSEQ_FAILURE_POLICY_REQUIRES_PATHS.to_string(),
        ));
    }
    let ocr_table_source_count = [
        parsed_optional.ocr_table_json.is_some(),
        parsed_optional.ocr_table_geometry_json.is_some(),
        parsed_optional.ocr_table_pipeline.is_some(),
    ]
    .iter()
    .filter(|&&x| x)
    .count();
    if ocr_table_source_count > 1 {
        return Err(CliParseAction::ShowError(
            ERR_OCR_TABLE_SOURCES_CONFLICT.to_string(),
        ));
    }
    if parsed_optional.ocr_table_pipeline.is_some()
        && parsed_optional.ocr_image.is_none()
        && !parsed_optional.ocr_from_pdf
    {
        return Err(CliParseAction::ShowError(
            ERR_OCR_TABLE_PIPELINE_REQUIRES_IMAGE_SOURCE.to_string(),
        ));
    }

    let parsed_failure_policy =
        if let Some(value) = parsed_optional.ocr_parseq_failure_policy.as_deref() {
            Some(ParseqFailurePolicyArg::parse(value)?)
        } else {
            None
        };

    let parsed_min_confidence = if let Some(value) = parsed_optional.ocr_min_confidence.as_deref() {
        let parsed = value
            .parse::<f32>()
            .map_err(|_| CliParseAction::ShowError(ERR_MIN_CONFIDENCE_MUST_BE_FLOAT.to_string()))?;
        if !(0.0..=1.0).contains(&parsed) {
            return Err(CliParseAction::ShowError(
                ERR_MIN_CONFIDENCE_OUT_OF_RANGE.to_string(),
            ));
        }
        Some(parsed)
    } else {
        None
    };

    let ocr_pdf_dpi = if let Some(value) = parsed_optional.ocr_pdf_dpi.as_deref() {
        let parsed = value.parse::<u32>().map_err(|_| {
            CliParseAction::ShowError(ERR_OCR_PDF_DPI_MUST_BE_POSITIVE_INT.to_string())
        })?;
        if parsed == 0 {
            return Err(CliParseAction::ShowError(
                ERR_OCR_PDF_DPI_MUST_BE_POSITIVE_INT.to_string(),
            ));
        }
        parsed
    } else {
        150
    };

    Ok(CliArgs {
        input_pdf: args[1].clone(),
        output_docx: args[2].clone(),
        ocr_mock: parsed_optional.ocr_mock,
        ocr_image: parsed_optional.ocr_image,
        ocr_parseq_model: parsed_optional.ocr_parseq_model,
        ocr_charset: parsed_optional.ocr_charset,
        ocr_parseq_failure_policy: parsed_failure_policy,
        ocr_min_confidence: parsed_min_confidence,
        ocr_from_pdf: parsed_optional.ocr_from_pdf,
        ocr_auto: parsed_optional.ocr_auto,
        ocr_pdf_dpi,
        clean_output: parsed_optional.clean_output,
        ocr_table_json: parsed_optional.ocr_table_json,
        ocr_table_geometry_json: parsed_optional.ocr_table_geometry_json,
        ocr_table_pipeline: parsed_optional.ocr_table_pipeline,
    })
}

fn parse_optional_args(extra_args: &[String]) -> Result<ParsedOptionalArgs, CliParseAction> {
    let mut parsed = ParsedOptionalArgs::default();
    let mut idx = 0usize;
    while idx < extra_args.len() {
        let token = &extra_args[idx];
        if !token.starts_with('-') {
            return Err(CliParseAction::ShowError(
                fmt_unexpected_positional_argument(token),
            ));
        }
        let (flag, inline_value) = parse_option_token(token);
        if matches!(flag, "--ocr-from-pdf" | "--ocr-auto" | "--clean-output") {
            if inline_value.is_some() {
                return Err(CliParseAction::ShowError(format!(
                    "{flag} does not take a value"
                )));
            }
            match flag {
                "--ocr-from-pdf" => set_flag_once(&mut parsed.ocr_from_pdf, flag)?,
                "--ocr-auto" => set_flag_once(&mut parsed.ocr_auto, flag)?,
                "--clean-output" => set_flag_once(&mut parsed.clean_output, flag)?,
                _ => {}
            }
            idx += 1;
            continue;
        }
        let (value, consumed) = if let Some(value) = inline_value {
            if value.is_empty() {
                return Err(CliParseAction::ShowError(fmt_missing_value_for_option(
                    flag,
                )));
            }
            (value.to_string(), 1usize)
        } else {
            let value = extra_args
                .get(idx + 1)
                .ok_or_else(|| CliParseAction::ShowError(fmt_missing_value_for_option(flag)))?;
            if value.starts_with('-') {
                return Err(CliParseAction::ShowError(fmt_missing_value_for_option(
                    flag,
                )));
            }
            (value.clone(), 2usize)
        };

        match flag {
            "--ocr-mock" => set_once(&mut parsed.ocr_mock, flag, value.clone())?,
            "--ocr-image" => set_once(&mut parsed.ocr_image, flag, value.clone())?,
            "--ocr-parseq-model" => set_once(&mut parsed.ocr_parseq_model, flag, value.clone())?,
            "--ocr-charset" => set_once(&mut parsed.ocr_charset, flag, value.clone())?,
            "--ocr-parseq-failure-policy" => {
                set_once(&mut parsed.ocr_parseq_failure_policy, flag, value.clone())?
            }
            "--ocr-min-confidence" => {
                set_once(&mut parsed.ocr_min_confidence, flag, value.clone())?
            }
            "--ocr-pdf-dpi" => set_once(&mut parsed.ocr_pdf_dpi, flag, value.clone())?,
            "--ocr-table-json" => set_once(&mut parsed.ocr_table_json, flag, value.clone())?,
            "--ocr-table-geometry-json" => {
                set_once(&mut parsed.ocr_table_geometry_json, flag, value.clone())?
            }
            "--ocr-table-pipeline" => {
                set_once(&mut parsed.ocr_table_pipeline, flag, value.clone())?
            }
            _ => {
                return Err(CliParseAction::ShowError(fmt_unknown_option(flag)));
            }
        }
        idx += consumed;
    }
    Ok(parsed)
}

fn set_flag_once(target: &mut bool, flag: &str) -> Result<(), CliParseAction> {
    if *target {
        return Err(CliParseAction::ShowError(fmt_duplicate_option(flag)));
    }
    *target = true;
    Ok(())
}

fn parse_option_token(token: &str) -> (&str, Option<&str>) {
    if let Some((flag, value)) = token.split_once('=') {
        (flag, Some(value))
    } else {
        (token, None)
    }
}

fn set_once(target: &mut Option<String>, flag: &str, value: String) -> Result<(), CliParseAction> {
    if target.is_some() {
        return Err(CliParseAction::ShowError(fmt_duplicate_option(flag)));
    }
    *target = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jade_pdf2docx_docx::automatic_ocr_pdf_page_indices;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn parse_cli_args_returns_help_action_when_help_flag_is_present() {
        let actual = parse_cli_args(&args(&["bin", "--help"]));
        assert!(matches!(actual, Err(CliParseAction::ShowHelp)));
    }

    #[test]
    fn parse_cli_args_rejects_missing_required_positional_arguments() {
        let actual = parse_cli_args(&args(&["bin"]));
        assert!(matches!(
            actual,
            Err(CliParseAction::ShowError(message))
                if message == ERR_MISSING_REQUIRED_POSITIONALS
        ));
    }

    #[test]
    fn parse_cli_args_accepts_equals_and_space_mixed_options() {
        let actual = parse_cli_args(&args(&[
            "bin",
            "in.pdf",
            "out.docx",
            "--ocr-mock=line1|line2",
            "--ocr-min-confidence",
            "0.8",
            "--ocr-parseq-model=missing-model.onnx",
            "--ocr-charset",
            "missing-charset.yaml",
            "--ocr-parseq-failure-policy=fallback",
        ]))
        .expect("parse should succeed");

        assert_eq!(actual.input_pdf, "in.pdf");
        assert_eq!(actual.output_docx, "out.docx");
        assert_eq!(actual.ocr_mock.as_deref(), Some("line1|line2"));
        assert_eq!(actual.ocr_min_confidence, Some(0.8));
        assert_eq!(
            actual.ocr_parseq_failure_policy,
            Some(ParseqFailurePolicyArg::FallbackToBandLabel)
        );
    }

    #[test]
    fn parse_cli_args_rejects_empty_value_in_equals_style() {
        let actual = parse_cli_args(&args(&["bin", "in.pdf", "out.docx", "--ocr-image="]));
        assert!(matches!(
            actual,
            Err(CliParseAction::ShowError(message))
                if message == fmt_missing_value_for_option("--ocr-image")
        ));
    }

    #[test]
    fn parse_cli_args_rejects_parseq_failure_policy_without_parseq_paths() {
        let actual = parse_cli_args(&args(&[
            "bin",
            "in.pdf",
            "out.docx",
            "--ocr-parseq-failure-policy",
            "fallback",
        ]));
        assert!(matches!(
            actual,
            Err(CliParseAction::ShowError(message))
                if message == ERR_PARSEQ_FAILURE_POLICY_REQUIRES_PATHS
        ));
    }

    #[test]
    fn parse_cli_args_rejects_table_pipeline_without_image_source() {
        let actual = parse_cli_args(&args(&[
            "bin",
            "in.pdf",
            "out.docx",
            "--ocr-table-pipeline",
            "/bin/true",
        ]));
        assert!(matches!(
            actual,
            Err(CliParseAction::ShowError(message))
                if message == ERR_OCR_TABLE_PIPELINE_REQUIRES_IMAGE_SOURCE
        ));
    }

    #[test]
    fn parse_cli_args_accepts_table_pipeline_with_ocr_from_pdf() {
        let actual = parse_cli_args(&args(&[
            "bin",
            "in.pdf",
            "out.docx",
            "--ocr-from-pdf",
            "--ocr-table-pipeline",
            "/bin/true",
        ]))
        .expect("parse should succeed");
        assert_eq!(actual.ocr_table_pipeline.as_deref(), Some("/bin/true"));
        assert!(actual.ocr_from_pdf);
        assert!(actual.ocr_image.is_none());
    }

    #[test]
    fn parse_cli_args_rejects_table_json_and_pipeline_together() {
        let actual = parse_cli_args(&args(&[
            "bin",
            "in.pdf",
            "out.docx",
            "--ocr-table-json",
            "a.json",
            "--ocr-table-pipeline",
            "/bin/true",
            "--ocr-image",
            "x.ppm",
        ]));
        assert!(matches!(
            actual,
            Err(CliParseAction::ShowError(message))
                if message == ERR_OCR_TABLE_SOURCES_CONFLICT
        ));
    }

    #[test]
    fn parse_cli_args_rejects_table_geometry_json_with_table_json() {
        let actual = parse_cli_args(&args(&[
            "bin",
            "in.pdf",
            "out.docx",
            "--ocr-table-json",
            "a.json",
            "--ocr-table-geometry-json",
            "b.json",
        ]));
        assert!(matches!(
            actual,
            Err(CliParseAction::ShowError(message))
                if message == ERR_OCR_TABLE_SOURCES_CONFLICT
        ));
    }

    #[test]
    fn parse_cli_args_accepts_ocr_auto_flag() {
        let actual = parse_cli_args(&args(&["bin", "in.pdf", "out.docx", "--ocr-auto"]))
            .expect("parse should succeed");

        assert!(actual.ocr_auto);
        assert!(!actual.ocr_from_pdf);
    }

    #[test]
    fn parse_cli_args_rejects_ocr_auto_value() {
        let actual = parse_cli_args(&args(&["bin", "in.pdf", "out.docx", "--ocr-auto=1"]));

        assert!(matches!(
            actual,
            Err(CliParseAction::ShowError(message)) if message == "--ocr-auto does not take a value"
        ));
    }

    #[test]
    fn auto_ocr_runs_for_image_only_pdf_shape() {
        let parsed = jade_pdf2docx_parser::ParsedDocument {
            quality: Default::default(),
            path: "scan.pdf".into(),
            byte_len: 1,
            header: "%PDF-1.7".to_string(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            embedded_fonts: Vec::new(),
            text_elements: Vec::new(),
            image_elements: vec![jade_pdf2docx_parser::ParsedImageElement {
                page: 0,
                bounds: jade_pdf2docx_parser::ParsedBounds {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 100.0,
                    y1: 100.0,
                },
                png_bytes: vec![0x89, b'P', b'N', b'G'],
                width_px: 100,
                height_px: 100,
                floating: false,
                rotation_degrees: 0,
            }],
            text_preview: String::new(),
        };

        assert_eq!(automatic_ocr_pdf_page_indices(&parsed), vec![0]);
    }

    #[test]
    fn auto_ocr_skips_digital_text_pdf_shape() {
        let parsed = jade_pdf2docx_parser::ParsedDocument {
            quality: Default::default(),
            path: "digital.pdf".into(),
            byte_len: 1,
            header: "%PDF-1.7".to_string(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            embedded_fonts: Vec::new(),
            text_elements: vec![jade_pdf2docx_parser::ParsedTextElement {
                page: 0,
                bounds: jade_pdf2docx_parser::ParsedBounds {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 100.0,
                    y1: 20.0,
                },
                font_size: 12.0,
                font_name: Some("Helvetica".to_string()),
                fill_color: None,
                text: "digital text".to_string(),
            }],
            image_elements: vec![jade_pdf2docx_parser::ParsedImageElement {
                page: 0,
                bounds: jade_pdf2docx_parser::ParsedBounds {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 100.0,
                    y1: 100.0,
                },
                png_bytes: vec![0x89, b'P', b'N', b'G'],
                width_px: 100,
                height_px: 100,
                floating: false,
                rotation_degrees: 0,
            }],
            text_preview: "digital text".to_string(),
        };

        assert!(automatic_ocr_pdf_page_indices(&parsed).is_empty());
    }

    #[test]
    fn auto_ocr_targets_only_pages_without_text_layer() {
        let parsed = jade_pdf2docx_parser::ParsedDocument {
            quality: Default::default(),
            path: "mixed.pdf".into(),
            byte_len: 1,
            header: "%PDF-1.7".to_string(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 3,
            pages: Vec::new(),
            graphics: Vec::new(),
            embedded_fonts: Vec::new(),
            text_elements: vec![
                jade_pdf2docx_parser::ParsedTextElement {
                    page: 0,
                    bounds: jade_pdf2docx_parser::ParsedBounds {
                        x0: 0.0,
                        y0: 0.0,
                        x1: 100.0,
                        y1: 20.0,
                    },
                    font_size: 12.0,
                    font_name: Some("Helvetica".to_string()),
                    fill_color: None,
                    text: "page one text".to_string(),
                },
                jade_pdf2docx_parser::ParsedTextElement {
                    page: 2,
                    bounds: jade_pdf2docx_parser::ParsedBounds {
                        x0: 0.0,
                        y0: 0.0,
                        x1: 100.0,
                        y1: 20.0,
                    },
                    font_size: 12.0,
                    font_name: Some("Helvetica".to_string()),
                    fill_color: None,
                    text: "page three text".to_string(),
                },
            ],
            image_elements: (0..3)
                .map(|page| jade_pdf2docx_parser::ParsedImageElement {
                    page,
                    bounds: jade_pdf2docx_parser::ParsedBounds {
                        x0: 0.0,
                        y0: 0.0,
                        x1: 100.0,
                        y1: 100.0,
                    },
                    png_bytes: vec![0x89, b'P', b'N', b'G'],
                    width_px: 100,
                    height_px: 100,
                    floating: false,
                    rotation_degrees: 0,
                })
                .collect(),
            text_preview: "page one text\npage three text".to_string(),
        };

        assert_eq!(automatic_ocr_pdf_page_indices(&parsed), vec![1]);
    }
}
