#[path = "support/report_fixture.rs"]
mod report_fixture;
use pdf_diff::isolation::{run_pdf_worker, ProcessLimits};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_pdf-diff"))
}
fn fixture(name: &str) -> PathBuf {
    report_fixture::path(name)
}
fn arguments(paths: &[&Path]) -> Vec<OsString> {
    paths.iter().map(|p| p.as_os_str().to_owned()).collect()
}

#[test]
fn isolated_public_api_preserves_comparison_codes_and_reports_execution_stats() {
    let old = fixture("report-old.pdf");
    let new = fixture("report-new.pdf");
    for (right, code) in [(&old, 0), (&new, 1)] {
        let output = run_pdf_worker(
            binary(),
            &arguments(&[&old, right]),
            &ProcessLimits::default(),
        )
        .unwrap();
        assert_eq!(output.exit_code, code);
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert!(output.stats.peak_memory_bytes > 0);
        assert!(output.stats.output_bytes >= output.stdout.len() as u64);
    }
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.pdf");
    std::fs::write(&bad, b"not a pdf").unwrap();
    let result = run_pdf_worker(
        binary(),
        &arguments(&[&old, &bad]),
        &ProcessLimits::default(),
    )
    .unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(result.stdout.is_empty());
    assert!(!result.stderr.is_empty());
}

#[test]
fn limited_render_keeps_existing_destination_and_success_publishes_valid_png() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.png");
    let original = b"previous output must survive";
    std::fs::write(&path, original).unwrap();
    let args = vec![
        "--render".into(),
        fixture("report-old.pdf").into_os_string(),
        "1".into(),
        "400".into(),
        path.clone().into_os_string(),
    ];
    let limits = ProcessLimits {
        max_output_bytes: 24,
        ..ProcessLimits::default()
    };
    assert!(run_pdf_worker(binary(), &args, &limits).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    let mut invalid = args.clone();
    invalid[2] = "0".into();
    let failed = run_pdf_worker(binary(), &invalid, &ProcessLimits::default()).unwrap();
    assert_eq!(failed.exit_code, 2);
    assert!(failed.stdout.is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    let output = run_pdf_worker(binary(), &args, &ProcessLimits::default()).unwrap();
    assert_eq!(output.exit_code, 0);
    assert!(std::fs::read(&path)
        .unwrap()
        .starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(String::from_utf8_lossy(&output.stdout).contains(path.to_str().unwrap()));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn cli_resource_failures_emit_no_stdout_and_the_next_process_recovers() {
    let path = fixture("report-old.pdf");
    for option in ["--timeout-ms", "--max-memory-mb"] {
        let output = Command::new(binary())
            .args([option, "1"])
            .arg(&path)
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    let output = Command::new(binary())
        .arg(&path)
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
    for option in ["--timeout-ms", "--max-memory-mb", "--max-output-mb"] {
        let output = Command::new(binary())
            .args([option, "0"])
            .arg(&path)
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}
