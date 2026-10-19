use std::io::Read;
use std::{fs, process::Command};
mod common;
#[allow(dead_code)]
#[path = "../crates/jade-pdf2docx-parser/tests/support/source_image_pdf.rs"]
mod source_fixture;

/// 実CLIでもCCITTの非identity Decodeをmediaへ保存し、入力を保持する。
#[test]
fn ccitt_source_decode_preserves_media_pixels() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("source.pdf");
    let output = directory.path().join("source.docx");
    let pdf = source_fixture::source_image_pdf_with_bits(
        "/ColorSpace /DeviceGray /Filter /CCITTFaxDecode /DecodeParms << /K 0 /Columns 2 /Rows 1 /EndOfBlock false >> /Decode [.25 .75]",
        &[0x35, 0x43, 0x80],
        "",
        1,
    );
    fs::write(&input, &pdf).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .args([input.as_os_str(), output.as_os_str()])
        .output()
        .unwrap();
    let warning = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "{warning}");
    assert!(warning.contains("ocr_required"), "{warning}");
    assert!(!warning.contains("image_replaced"), "{warning}");
    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    let mut images = 0;
    for index in 0..archive.len() {
        let mut part = archive.by_index(index).unwrap();
        if part.is_dir() || !part.name().starts_with("word/media/") {
            continue;
        }
        let mut png = Vec::new();
        part.read_to_end(&mut png).unwrap();
        assert_eq!(
            source_fixture::scanline(&png),
            [0, 64, 64, 64, 191, 191, 191]
        );
        images += 1;
    }
    assert_eq!(images, 1);
    assert_eq!(fs::read(input).unwrap(), pdf);
}

/// rename前でも完成した所有証拠は回収し、書込途中の領域と隣接出力は保持する。
#[cfg(unix)]
#[test]
fn cleanup_workspace_handles_completed_pending_marker() {
    let base = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(base.path()).unwrap();
    let output = root.join("output.docx");
    fs::write(&output, b"keep").unwrap();
    for complete in [false, true] {
        let path = root.join(format!("jade-pdf2docx-supervised-cli-pending-{complete}"));
        fs::create_dir(&path).unwrap();
        let mut marker = b"JADE-PDF2DOCX-WORKSPACE-3\n".to_vec();
        marker.extend_from_slice(path.as_os_str().as_encoded_bytes());
        if !complete {
            marker.pop();
        }
        fs::write(path.join("workspace-marker-pending"), &marker).unwrap();
        fs::write(path.join("partial"), b"partial").unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
            .arg("--cleanup-workspace")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(result.status.success(), complete, "{result:?}");
        assert_eq!(path.exists(), !complete);
        if !complete {
            assert_eq!(
                fs::read(path.join("workspace-marker-pending")).unwrap(),
                marker
            );
            assert_eq!(fs::read(path.join("partial")).unwrap(), b"partial");
        }
        assert_eq!(fs::read(&output).unwrap(), b"keep");
    }
}

/// 完成済みの残骸だけを明示パスで回収し、隣の入力・出力・foreign領域を保持する。
#[cfg(unix)]
#[test]
fn cleanup_workspace_removes_stale_owned_directory() {
    let base = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(base.path()).unwrap();
    let directory = root.join("jade-pdf2docx-supervised-stale");
    fs::create_dir(&directory).unwrap();
    let mut marker = b"JADE-PDF2DOCX-WORKSPACE-3\n".to_vec();
    marker.extend_from_slice(directory.as_os_str().as_encoded_bytes());
    fs::write(directory.join(".jade-pdf2docx-workspace-owner"), marker).unwrap();
    fs::create_dir(directory.join("unmarked-commit")).unwrap();
    fs::write(directory.join("unmarked-commit/partial"), b"partial").unwrap();
    for name in [
        "input.pdf",
        "output.docx",
        "jade-pdf2docx-supervised-foreign",
    ] {
        fs::write(root.join(name), b"keep").unwrap();
    }
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .arg("--cleanup-workspace")
        .arg(&directory)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!directory.exists());
    for name in [
        "input.pdf",
        "output.docx",
        "jade-pdf2docx-supervised-foreign",
    ] {
        assert_eq!(fs::read(root.join(name)).unwrap(), b"keep");
    }
}

/// 実行中の共有leaseは即座に拒否し、markerと変換途中のbyteを保持する。
#[cfg(unix)]
#[test]
fn cleanup_workspace_refuses_live_lease() {
    let base = tempfile::tempdir().unwrap();
    let directory = fs::canonicalize(base.path())
        .unwrap()
        .join("jade-pdf2docx-supervised-live");
    fs::create_dir(&directory).unwrap();
    let mut marker = b"JADE-PDF2DOCX-WORKSPACE-3\n".to_vec();
    marker.extend_from_slice(directory.as_os_str().as_encoded_bytes());
    let marker_path = directory.join(".jade-pdf2docx-workspace-owner");
    fs::write(&marker_path, &marker).unwrap();
    let lease = fs::File::open(&marker_path).unwrap();
    lease.try_lock_shared().unwrap();
    fs::write(directory.join("partial"), b"partial").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .arg("--cleanup-workspace")
        .arg(&directory)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("in use"));
    assert_eq!(fs::read(marker_path).unwrap(), marker);
    assert_eq!(fs::read(directory.join("partial")).unwrap(), b"partial");
}

/// 未完成・旧版・別候補のmarkerを所有証明に使わず、誤指定を安全に拒否する。
#[cfg(unix)]
#[test]
fn cleanup_workspace_preserves_unverifiable_directories() {
    let base = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(base.path()).unwrap();
    for (name, marker) in [
        ("jade-pdf2docx-supervised-unmarked", None),
        (
            "jade-pdf2docx-supervised-old",
            Some(b"JADE-PDF2DOCX-WORKSPACE-2\n".as_slice()),
        ),
        (
            "jade-pdf2docx-supervised-other",
            Some(b"JADE-PDF2DOCX-WORKSPACE-3\n/other".as_slice()),
        ),
        ("foreign", Some(b"JADE-PDF2DOCX-WORKSPACE-3\n".as_slice())),
    ] {
        let directory = root.join(name);
        fs::create_dir(&directory).unwrap();
        if let Some(marker) = marker {
            let mut marker = marker.to_vec();
            if marker.ends_with(b"\n") {
                marker.extend_from_slice(directory.as_os_str().as_encoded_bytes());
            }
            fs::write(directory.join(".jade-pdf2docx-workspace-owner"), marker).unwrap();
        }
        fs::write(directory.join("keep"), b"foreign").unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
            .arg("--cleanup-workspace")
            .arg(&directory)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1), "{name}");
        assert_eq!(fs::read(directory.join("keep")).unwrap(), b"foreign");
    }
}

/// markerとdirectoryのリンク、FIFO、過大通知でリンク先やforeign byteを削除しない。
#[cfg(unix)]
#[test]
fn cleanup_workspace_rejects_links_fifo_and_oversized_marker() {
    use std::os::unix::fs::symlink;
    let base = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(base.path()).unwrap();
    let external = root.join("external");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("keep"), b"foreign").unwrap();
    for kind in ["directory-link", "marker-link", "fifo", "oversized"] {
        let directory = root.join(format!("jade-pdf2docx-supervised-{kind}"));
        if kind == "directory-link" {
            symlink(&external, &directory).unwrap();
        } else {
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("keep"), b"foreign").unwrap();
            let marker_path = directory.join(".jade-pdf2docx-workspace-owner");
            match kind {
                "marker-link" => {
                    symlink(external.join("keep"), &marker_path).unwrap();
                }
                "fifo" => {
                    assert!(
                        Command::new("mkfifo")
                            .arg(&marker_path)
                            .status()
                            .unwrap()
                            .success()
                    );
                }
                _ => {
                    let mut marker = b"JADE-PDF2DOCX-WORKSPACE-3\n".to_vec();
                    marker.extend_from_slice(directory.as_os_str().as_encoded_bytes());
                    marker.push(b'x');
                    fs::write(marker_path, marker).unwrap();
                }
            }
        }
        let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
            .arg("--cleanup-workspace")
            .arg(&directory)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1), "{kind}");
        assert_eq!(fs::read(directory.join("keep")).unwrap(), b"foreign");
        assert_eq!(fs::read(external.join("keep")).unwrap(), b"foreign");
    }
}

/// native補助を要しない欠落ページも警告し、strictでは既存出力を保護する。
#[test]
fn missing_geometry_warns_and_strict_preserves_output() {
    let input = common::temp_output_docx_path().with_extension("pdf");
    let output = common::temp_output_docx_path();
    let pdf = b"%PDF-1.7\n1 0 obj << /Type /Page >> endobj\n%%EOF\n";
    fs::write(&input, pdf).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .args([input.as_os_str(), output.as_os_str()])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let warning = String::from_utf8_lossy(&result.stderr);
    assert!(
        warning.contains("quality warning:")
            && warning.contains("missing_page_geometry")
            && warning.contains("page 1"),
        "{warning}"
    );
    fs::write(&output, b"existing output").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .args([input.as_os_str(), output.as_os_str()])
        .arg("--strict")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("missing_page_geometry"));
    assert_eq!(fs::read(&output).unwrap(), b"existing output");
    assert_eq!(fs::read(&input).unwrap(), pdf);
    fs::remove_file(input).unwrap();
    fs::remove_file(output).unwrap();
}

/// 通常バイナリの help を成功終了で表示する。
#[test]
fn binary_help_explains_conversion() {
    let output = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("usage: jade-pdf2docx <input.pdf> <output.docx>")
    );
}

/// 引数不足は使用法を stderr に出し usage error とする。
#[test]
fn missing_arguments_return_usage_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
}

/// 解析失敗は成功扱いせず既存の出力を保持する。
#[test]
fn invalid_input_returns_failure_and_preserves_existing_output() {
    let input = common::temp_output_docx_path().with_extension("pdf");
    let output = common::temp_output_docx_path();
    fs::write(&input, b"not a pdf").unwrap();
    fs::write(&output, b"existing output").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .args([input.as_os_str(), output.as_os_str()])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("conversion failed:"));
    assert_eq!(fs::read(&output).unwrap(), b"existing output");
    fs::remove_file(input).unwrap();
    fs::remove_file(output).unwrap();
}

/// 配布されたバイナリの version を取得できる。
#[test]
fn binary_reports_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        concat!("jade-pdf2docx ", env!("CARGO_PKG_VERSION"))
    );
}

/// 未知のオプションを変換成功として扱わない。
#[test]
fn unknown_option_returns_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .args(["input.pdf", "output.docx", "--unknown"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

/// 白画像へ置換する PDF は通常モードで警告し、strict では既存出力を保持する。
#[test]
fn image_replacement_warns_and_strict_preserves_output() {
    let input = common::temp_output_docx_path().with_extension("pdf");
    let output = common::temp_output_docx_path();
    fs::write(&input, b"%PDF-1.7\n1 0 obj << /Type /Page >> endobj\n2 0 obj << /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .args([input.as_os_str(), output.as_os_str()])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let warning = String::from_utf8_lossy(&result.stderr);
    assert!(warning.contains("quality warning:"), "{warning}");
    assert!(warning.contains("object 2"), "{warning}");
    assert!(warning.contains("unsupported_filter"), "{warning}");
    fs::write(&output, b"existing output").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .args([input.as_os_str(), output.as_os_str()])
        .args(["--strict", "--debug-output"])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("quality rejected"));
    assert_eq!(fs::read(&output).unwrap(), b"existing output");
    fs::remove_file(input).unwrap();
    fs::remove_file(output).unwrap();
}

/// 起動・変換・保存の監督期限と既定値をhelpで明示する。
#[test]
fn binary_help_explains_worker_timeout() {
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(result.status.success());
    let help = String::from_utf8_lossy(&result.stdout);
    assert!(help.contains("--timeout-ms"), "{help}");
    assert!(help.contains("300000"), "{help}");
    assert!(
        help.contains("supervisor startup, conversion and commit"),
        "{help}"
    );
}

/// 不正なtimeoutはworkerを起動せずusage errorにする。
#[test]
fn invalid_timeouts_are_usage_errors() {
    for options in [
        vec!["--timeout-ms"],
        vec!["--timeout-ms", "0"],
        vec!["--timeout-ms", "-1"],
        vec!["--timeout-ms", "abc"],
        vec!["--timeout-ms", "18446744073709551616"],
        vec!["--timeout-ms", "10", "--timeout-ms", "20"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
            .args(["missing.pdf", "output.docx"])
            .args(options)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&result.stderr).contains("usage:"));
    }
}

/// 読込のopenで停止するFIFOをworker期限で終了し、既存出力と一時領域を保持・回収する。
#[cfg(unix)]
#[test]
fn worker_timeout_terminates_blocked_input_and_preserves_output() {
    use std::os::unix::fs::FileTypeExt;
    let base =
        std::env::temp_dir().join(format!("jade-pdf2docx-timeout-{}", common::unique_suffix()));
    fs::create_dir(&base).unwrap();
    let input = base.join("blocked.pdf");
    let output = base.join("output.docx");
    assert!(
        Command::new("mkfifo")
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    fs::write(&output, b"existing output").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .args([input.as_os_str(), output.as_os_str()])
        .args(["--timeout-ms", "100"])
        .env("TMPDIR", &base)
        .env("TMP", &base)
        .env("TEMP", &base)
        .output()
        .unwrap();
    let preserved = fs::read(&output).unwrap();
    let input_preserved = fs::metadata(&input).unwrap().file_type().is_fifo();
    let entries = fs::read_dir(&base).unwrap().count();
    fs::remove_dir_all(base).unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("conversion timed out after 100 ms"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    assert_eq!(preserved, b"existing output");
    assert!(input_preserved);
    assert_eq!(
        entries, 2,
        "worker workspace or commit temporary file leaked"
    );
}

/// 期限切れworkerは新規出力の親directoryと部分成果物を残さない。
#[cfg(unix)]
#[test]
fn worker_timeout_does_not_create_output_parent() {
    let base = tempfile::tempdir().unwrap();
    let input = base.path().join("blocked.pdf");
    let output = base.path().join("new/output.docx");
    assert!(
        Command::new("mkfifo")
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .args([input.as_os_str(), output.as_os_str()])
        .args(["--timeout-ms", "100"])
        .env("TMPDIR", base.path())
        .env("TMP", base.path())
        .env("TEMP", base.path())
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("conversion timed out after 100 ms"));
    assert!(!output.parent().unwrap().exists());
    assert_eq!(fs::read_dir(base.path()).unwrap().count(), 1);
}

/// 未作成directoryの後の親参照で入力へ戻る出力は、directory作成前に拒否する。
#[test]
fn future_output_alias_preserves_input_before_creating_directories() {
    for hardlink in [false, true] {
        let base = tempfile::tempdir().unwrap();
        let input = base.path().join("input.pdf");
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Page >> endobj\n%%EOF\n";
        fs::write(&input, pdf).unwrap();
        fs::create_dir(base.path().join("parent")).unwrap();
        let name = if hardlink {
            fs::hard_link(&input, base.path().join("alias.docx")).unwrap();
            "alias.docx"
        } else {
            "input.pdf"
        };
        let output = base.path().join("parent/missing/../..").join(name);
        let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
            .args([input.as_os_str(), output.as_os_str()])
            .output()
            .unwrap();
        assert_eq!(
            fs::read(&input).unwrap(),
            pdf,
            "input was replaced through a future output alias"
        );
        assert_eq!(result.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("output path refers to the input PDF")
        );
        if hardlink {
            assert_eq!(fs::read(base.path().join("alias.docx")).unwrap(), pdf);
        }
        assert!(!base.path().join("parent/missing").exists());
    }
}
/// 対応外環境と通常directoryを、cgroupの明示回収でも変更しない。
#[test]
fn cleanup_memory_cgroup_preserves_ordinary_directory() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join("jade-pdf2docx-memory-v1-1-2-3");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep"), b"foreign").unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .arg("--cleanup-memory-cgroup")
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("memory cgroup cleanup failed"),
        "{result:?}"
    );
    assert_eq!(std::fs::read(path.join("keep")).unwrap(), b"foreign");
}
