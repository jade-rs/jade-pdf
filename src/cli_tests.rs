//! 通常CLIと内部workerの引数・timeoutを変換処理より先に検証する。
use crate::cli::{Action, DEFAULT_TIMEOUT, SUPERVISOR_FLAG, WORKER_FLAG, parse};
use std::{ffi::OsString, time::Duration};

/// OS引数列を簡潔に作る。
fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

/// cgroup回収は絶対パス一つを明示した独立操作だけ許可する。
#[test]
fn memory_cleanup_requires_exact_absolute_path() {
    let path = std::env::temp_dir().join("jade-pdf2docx-memory-v1-1-2-3");
    let values = vec![
        "--cleanup-memory-cgroup".into(),
        path.clone().into_os_string(),
    ];
    assert!(matches!(parse(&values), Ok(Action::CleanupMemoryGroup(p)) if p == path));
    for values in [
        vec!["--cleanup-memory-cgroup"],
        vec!["--cleanup-memory-cgroup", "relative"],
        vec!["--cleanup-memory-cgroup", ""],
        vec!["--cleanup-memory-cgroup", "/cg", "--help"],
        vec!["--cleanup-memory-cgroup", "/cg", "/other"],
        vec![WORKER_FLAG, "--cleanup-memory-cgroup", "/cg"],
        vec![SUPERVISOR_FLAG, "--cleanup-memory-cgroup", "/cg"],
        vec!["in.pdf", "out.docx", "--cleanup-memory-cgroup", "/cg"],
    ] {
        assert!(parse(&args(&values)).is_err(), "{values:?}");
    }
}

/// メモリ上限は親だけで検証し、子孫にはcgroup所属で引き継ぐ。
#[test]
fn memory_limit_requires_positive_mib_and_absolute_parent() {
    let parent = std::env::temp_dir().join("jade-pdf2docx-cgroup-parent");
    for mib in [1_u64, 64, 2048, (i64::MAX as u64) / (1024 * 1024)] {
        let values = vec![
            "in.pdf".into(),
            "out.docx".into(),
            "--max-memory-mib".into(),
            mib.to_string().into(),
            "--memory-cgroup-parent".into(),
            parent.clone().into_os_string(),
        ];
        let Action::Convert(request) = parse(&values).unwrap() else {
            panic!("expected conversion")
        };
        let memory = request.memory.as_ref().unwrap();
        assert_eq!(memory.bytes, mib * 1024 * 1024);
        assert_eq!(memory.parent, parent);
        let mut child = std::process::Command::new("jade-pdf2docx");
        request.append_conversion_args(&mut child);
        assert_eq!(child.get_args().count(), 0, "子で新しいcgroupを作らない");
        for mode in [WORKER_FLAG, SUPERVISOR_FLAG] {
            let mut internal = vec![OsString::from(mode)];
            internal.extend(values.clone());
            assert!(parse(&internal).is_err());
        }
    }
    for options in [
        vec!["--max-memory-mib", "1"],
        vec!["--memory-cgroup-parent", "/cg"],
        vec!["--max-memory-mib", "0", "--memory-cgroup-parent", "/cg"],
        vec!["--max-memory-mib", "-1", "--memory-cgroup-parent", "/cg"],
        vec!["--max-memory-mib", "+1", "--memory-cgroup-parent", "/cg"],
        vec!["--max-memory-mib", "1.5", "--memory-cgroup-parent", "/cg"],
        vec![
            "--max-memory-mib",
            "18446744073709551615",
            "--memory-cgroup-parent",
            "/cg",
        ],
        vec![
            "--max-memory-mib",
            "8796093022208",
            "--memory-cgroup-parent",
            "/cg",
        ],
        vec![
            "--max-memory-mib",
            "1",
            "--memory-cgroup-parent",
            "relative",
        ],
        vec!["--max-memory-mib", "1", "--memory-cgroup-parent", ""],
        vec![
            "--max-memory-mib",
            "1",
            "--max-memory-mib",
            "2",
            "--memory-cgroup-parent",
            "/cg",
        ],
        vec![
            "--max-memory-mib",
            "1",
            "--memory-cgroup-parent",
            "/cg",
            "--memory-cgroup-parent",
            "/other",
        ],
    ] {
        let mut values = args(&["in.pdf", "out.docx"]);
        values.extend(args(&options));
        assert!(parse(&values).is_err(), "{options:?}");
    }
}

/// cgroup委譲先も非UTF-8のOSパスを保持する。
#[cfg(unix)]
#[test]
fn memory_parent_preserves_os_path() {
    use std::os::unix::ffi::OsStringExt;
    let parent = OsString::from_vec(b"/cgroup/jade-pdf2docx-\xff".to_vec());
    let mut values = args(&[
        "in.pdf",
        "out.docx",
        "--max-memory-mib",
        "64",
        "--memory-cgroup-parent",
    ]);
    values.push(parent.clone());
    let Action::Convert(request) = parse(&values).unwrap() else {
        panic!("expected conversion")
    };
    assert_eq!(request.memory.unwrap().parent.as_os_str(), parent);
}

/// OCR指定は親・監督・workerの各modeで受け入れ、片方だけのモデル指定を拒否する。
#[test]
fn ocr_options_are_validated_for_every_process_mode() {
    for mode in [None, Some(SUPERVISOR_FLAG), Some(WORKER_FLAG)] {
        let mut values = mode.into_iter().map(OsString::from).collect::<Vec<_>>();
        values.extend(args(&[
            "input.pdf",
            "output.docx",
            "--ocr",
            "--ocr-model",
            "model.onnx",
            "--ocr-charset",
            "charset.yaml",
            "--ocr-dpi",
            "72",
        ]));
        let request = match parse(&values).unwrap() {
            Action::Convert(request) | Action::Supervisor(request) | Action::Worker(request) => {
                request
            }
            _ => panic!("expected conversion"),
        };
        let ocr = request.ocr.as_ref().unwrap();
        assert_eq!(ocr.model, std::path::Path::new("model.onnx"));
        assert_eq!(ocr.charset, std::path::Path::new("charset.yaml"));
        assert_eq!(ocr.dpi, 72);
        let mut command = std::process::Command::new("jade-pdf2docx");
        request.append_conversion_args(&mut command);
        assert_eq!(
            command.get_args().map(OsString::from).collect::<Vec<_>>(),
            values[values.len() - 7..]
        );
    }
    for options in [
        vec!["--ocr"],
        vec!["--ocr", "--ocr-model", "m"],
        vec!["--ocr-model", "m", "--ocr-charset", "c"],
        vec!["--ocr", "--ocr", "--ocr-model", "m", "--ocr-charset", "c"],
        vec![
            "--ocr",
            "--ocr-model",
            "m",
            "--ocr-model",
            "n",
            "--ocr-charset",
            "c",
        ],
        vec![
            "--ocr",
            "--ocr-model",
            "m",
            "--ocr-charset",
            "c",
            "--ocr-dpi",
            "0",
        ],
        vec![
            "--ocr",
            "--ocr-model",
            "m",
            "--ocr-charset",
            "c",
            "--ocr-dpi",
            "601",
        ],
    ] {
        let mut values = args(&["input.pdf", "output.docx"]);
        values.extend(args(&options));
        assert!(parse(&values).is_err(), "{options:?}");
    }
}

/// 縦書きと陰影補正は親・監督・workerを往復して同じ指定を保つ。
#[test]
fn explicit_vertical_ocr_options_roundtrip() {
    let values = args(&[
        "in.pdf",
        "out.docx",
        "--ocr",
        "--ocr-model",
        "m",
        "--ocr-charset",
        "c",
        "--ocr-direction",
        "vertical",
        "--ocr-paper-shading",
    ]);
    let Action::Convert(request) = parse(&values).unwrap() else {
        panic!("conversion required")
    };
    assert!(request.ocr.as_ref().unwrap().vertical);
    assert!(request.ocr.as_ref().unwrap().paper_shading);
    let mut command = std::process::Command::new("jade-pdf2docx");
    request.append_conversion_args(&mut command);
    for mode in [SUPERVISOR_FLAG, WORKER_FLAG] {
        let mut forwarded = args(&[mode, "in.pdf", "out.docx"]);
        forwarded.extend(command.get_args().map(OsString::from));
        let (Action::Supervisor(child) | Action::Worker(child)) = parse(&forwarded).unwrap() else {
            panic!("child required")
        };
        assert!(child.ocr.as_ref().unwrap().vertical);
        assert!(child.ocr.as_ref().unwrap().paper_shading);
    }
}

/// OCRなしのオプション、未知の方向、重複指定は黙って受け入れない。
#[test]
fn invalid_direction_and_shading_options_are_rejected() {
    for extra in [
        vec!["--ocr-direction", "vertical"],
        vec!["--ocr-paper-shading"],
    ] {
        let mut values = args(&["in.pdf", "out.docx"]);
        values.extend(args(&extra));
        assert!(parse(&values).is_err());
    }
    for extra in [
        vec!["--ocr-direction"],
        vec!["--ocr-direction", "auto"],
        vec![
            "--ocr-direction",
            "vertical",
            "--ocr-direction",
            "horizontal",
        ],
        vec!["--ocr-paper-shading", "--ocr-paper-shading"],
    ] {
        let mut values = args(&[
            "in.pdf",
            "out.docx",
            "--ocr",
            "--ocr-model",
            "m",
            "--ocr-charset",
            "c",
        ]);
        values.extend(args(&extra));
        assert!(parse(&values).is_err(), "{extra:?}");
    }
}

/// OCRモデルと文字セットのOSパスを変換せずworkerへ引き継ぐ。
#[cfg(unix)]
#[test]
fn ocr_paths_preserve_non_utf8_and_default_dpi() {
    use std::os::unix::ffi::OsStringExt;
    let model = OsString::from_vec(b"model-\xff.onnx".to_vec());
    let charset = OsString::from_vec(b"charset-\xfe.yaml".to_vec());
    let mut values = args(&["in.pdf", "out.docx", "--ocr", "--ocr-model"]);
    values.extend([model.clone(), "--ocr-charset".into(), charset.clone()]);
    let Action::Convert(request) = parse(&values).unwrap() else {
        panic!("expected conversion")
    };
    let ocr = request.ocr.as_ref().unwrap();
    assert_eq!(ocr.model.as_os_str(), model);
    assert_eq!(ocr.charset.as_os_str(), charset);
    assert_eq!(ocr.dpi, 150);
    let mut command = std::process::Command::new("jade-pdf2docx");
    request.append_conversion_args(&mut command);
    let forwarded = command.get_args().map(OsString::from).collect::<Vec<_>>();
    assert_eq!(forwarded[2], model);
    assert_eq!(forwarded[4], charset);
}

/// 回収は明示絶対パス一件だけを受け入れ、変換flagと内部modeへの混入を拒否する。
#[test]
fn cleanup_requires_exact_absolute_path() {
    let absolute = std::env::temp_dir().join("jade-pdf2docx-supervised-stale");
    let values = [
        OsString::from("--cleanup-workspace"),
        absolute.clone().into_os_string(),
    ];
    assert!(matches!(parse(&values), Ok(Action::CleanupWorkspace(path)) if path == absolute));
    for values in [
        vec!["--cleanup-workspace"],
        vec!["--cleanup-workspace", "relative"],
        vec!["--cleanup-workspace", "/absolute", "--strict"],
        vec![WORKER_FLAG, "--cleanup-workspace", "/absolute"],
        vec![SUPERVISOR_FLAG, "--cleanup-workspace", "/absolute"],
    ] {
        assert!(parse(&args(&values)).is_err());
    }
}

/// 既定timeoutとオプションの全順序で正しい要求を保持する。
#[test]
fn conversion_options_accept_every_order() {
    let Action::Convert(default) = parse(&args(&["input.pdf", "output.docx"])).unwrap() else {
        panic!()
    };
    assert_eq!(default.timeout, DEFAULT_TIMEOUT);
    assert_eq!(DEFAULT_TIMEOUT, Duration::from_millis(300000));
    assert!(!default.strict && !default.debug);
    for options in [
        ["--strict", "--debug-output", "--timeout-ms", "123"],
        ["--debug-output", "--strict", "--timeout-ms", "123"],
        ["--strict", "--timeout-ms", "123", "--debug-output"],
        ["--debug-output", "--timeout-ms", "123", "--strict"],
        ["--timeout-ms", "123", "--strict", "--debug-output"],
        ["--timeout-ms", "123", "--debug-output", "--strict"],
    ] {
        let mut values = args(&["input.pdf", "output.docx"]);
        values.extend(args(&options));
        let Action::Convert(request) = parse(&values).unwrap() else {
            panic!()
        };
        assert_eq!(request.timeout, Duration::from_millis(123));
        assert!(request.strict && request.debug);
        assert_eq!(request.input, std::path::Path::new("input.pdf"));
        assert_eq!(request.output, std::path::Path::new("output.docx"));
    }
}

/// 不正値・重複・未知flagを明示拒否する。
#[test]
fn invalid_conversion_options_are_rejected() {
    for options in [
        vec!["--timeout-ms"],
        vec!["--timeout-ms", "0"],
        vec!["--timeout-ms", "-1"],
        vec!["--timeout-ms", "+1"],
        vec!["--timeout-ms", "1.5"],
        vec!["--timeout-ms", ""],
        vec!["--timeout-ms", "18446744073709551616"],
        vec!["--timeout-ms", "1", "--timeout-ms", "2"],
        vec!["--strict", "--strict"],
        vec!["--debug-output", "--debug-output"],
        vec!["--unknown"],
    ] {
        let mut values = args(&["input.pdf", "output.docx"]);
        values.extend(args(&options));
        assert!(parse(&values).is_err(), "{options:?}");
    }
    for values in [
        vec![],
        vec!["input.pdf"],
        vec!["--strict", "output.docx"],
        vec!["input.pdf", "--help"],
        vec!["--help", "extra"],
        vec![WORKER_FLAG, WORKER_FLAG, "output.docx"],
    ] {
        assert!(parse(&args(&values)).is_err(), "{values:?}");
    }
}

/// 内部workerは再度隔離せず、変換flagだけを保持する。
#[test]
fn internal_worker_is_a_distinct_action() {
    let Action::Worker(request) = parse(&args(&[
        WORKER_FLAG,
        "input.pdf",
        "staged.docx",
        "--strict",
        "--debug-output",
    ]))
    .unwrap() else {
        panic!()
    };
    assert!(request.strict && request.debug);
    assert_eq!(request.output, std::path::Path::new("staged.docx"));
    assert!(
        parse(&args(&[
            WORKER_FLAG,
            "input.pdf",
            "staged.docx",
            "--timeout-ms",
            "1"
        ]))
        .is_err()
    );
    assert!(matches!(parse(&args(&["-h"])), Ok(Action::Help)));
    assert!(matches!(parse(&args(&["--version"])), Ok(Action::Version)));
}

/// 内部監督は最終出力・timeout・変換flagを保持し、再帰起動しない。
#[test]
fn internal_supervisor_preserves_conversion_options() {
    for timeout in ["1", "300000", "18446744073709551615"] {
        let Action::Supervisor(request) = parse(&args(&[
            SUPERVISOR_FLAG,
            "input.pdf",
            "final.docx",
            "--strict",
            "--timeout-ms",
            timeout,
            "--debug-output",
        ]))
        .unwrap() else {
            panic!()
        };
        assert!(request.strict && request.debug);
        assert_eq!(
            request.timeout,
            Duration::from_millis(timeout.parse().unwrap())
        );
        assert_eq!(request.output, std::path::Path::new("final.docx"));
    }
    for flags in [
        vec!["--timeout-ms", "0"],
        vec!["--timeout-ms", "1", "--timeout-ms", "2"],
        vec![WORKER_FLAG],
    ] {
        let mut values = args(&[SUPERVISOR_FLAG, "input.pdf", "final.docx"]);
        values.extend(args(&flags));
        assert!(parse(&values).is_err());
    }
}

/// 非UTF-8の入力・出力パスもworkerまで損失なく保持する。
#[cfg(unix)]
#[test]
fn non_utf8_paths_are_preserved() {
    use std::os::unix::ffi::OsStringExt;
    let input = OsString::from_vec(b"input-\xff.pdf".to_vec());
    let output = OsString::from_vec(b"output-\xfe.docx".to_vec());
    let Action::Convert(request) = parse(&[input.clone(), output.clone()]).unwrap() else {
        panic!()
    };
    assert_eq!(request.input.as_os_str(), input);
    assert_eq!(request.output.as_os_str(), output);
}

/// digestは64桁の小文字hexだけを一度指定し、内部modeにも引き継げる。
#[test]
fn input_digest_is_strict_and_available_in_all_modes() {
    let digest = "0123456789abcdef".repeat(4);
    for prefix in [None, Some(WORKER_FLAG), Some(SUPERVISOR_FLAG)] {
        let mut values = Vec::new();
        if let Some(prefix) = prefix {
            values.push(OsString::from(prefix));
        }
        values.extend(args(&["in.pdf", "out.docx", "--input-sha256", &digest]));
        let request = match parse(&values).unwrap() {
            Action::Convert(request) | Action::Supervisor(request) | Action::Worker(request) => {
                request
            }
            _ => panic!("conversion expected"),
        };
        assert_eq!(request.input_sha256.as_deref(), Some(digest.as_str()));
        let mut command = std::process::Command::new("jade-pdf2docx");
        request.append_conversion_args(&mut command);
        assert_eq!(
            command.get_args().map(OsString::from).collect::<Vec<_>>(),
            args(&["--input-sha256", &digest])
        );
        for invalid in [
            "".to_string(),
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
            format!("{digest}\n"),
        ] {
            let mut invalid_values = values.clone();
            *invalid_values.last_mut().unwrap() = invalid.into();
            assert!(parse(&invalid_values).is_err());
        }
        values.extend(args(&["--input-sha256", &digest]));
        assert!(parse(&values).is_err());
    }
    assert!(parse(&args(&["in.pdf", "out.docx", "--input-sha256"])).is_err());
}

/// 公開照合情報を親と照合済み入力に限定し、型・重複・内部worker注入を拒否する。
#[test]
fn publication_receipt_requires_verified_input_and_parent() {
    let receipt = format!("{}:{}", "a".repeat(32), "b".repeat(64));
    let digest = "b".repeat(64);
    let valid = [
        "in.pdf",
        "out.docx",
        "--publication-receipt",
        &receipt,
        "--input-sha256",
        &digest,
    ];
    assert!(parse(&args(&valid)).is_ok());
    assert!(parse(&args(&valid[..4])).is_err());
    for flag in [super::cli::WORKER_FLAG, super::cli::SUPERVISOR_FLAG] {
        let mut values = args(&[flag]);
        values.extend(args(&valid));
        assert!(parse(&values).is_err());
    }
    for invalid in [
        "".to_string(),
        receipt.to_uppercase(),
        format!("{receipt}x"),
        "a".repeat(97),
    ] {
        assert!(
            parse(&args(&[
                "in.pdf",
                "out.docx",
                "--input-sha256",
                &digest,
                "--publication-receipt",
                &invalid
            ]))
            .is_err()
        );
    }
    let mut duplicate = args(&valid);
    duplicate.extend(args(&["--publication-receipt", &receipt]));
    assert!(parse(&duplicate).is_err());
}
