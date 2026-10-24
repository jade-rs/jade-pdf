#[path = "support/report_fixture.rs"]
#[allow(dead_code)]
mod report_fixture;
use pdf_diff::isolation::{IsolatedPdfClient, ProcessLimits, REQUEST_FLAG};
use pdf_diff::{DiffOptions, Limits};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
#[path = "support/appearance_fixture.rs"]
mod fixture;

fn binary() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_pdf-diff"))
}
fn report(name: &str) -> Vec<u8> {
    report_fixture::bytes(name == "report-new.pdf")
}
fn empty(path: &Path) {
    assert_eq!(
        std::fs::read_dir(path).unwrap().count(),
        0,
        "Private staging files were retained"
    );
}

#[test]
fn typed_byte_read_text_and_both_render_modes_match_in_process_results() {
    let dir = tempfile::tempdir().unwrap();
    let client = IsolatedPdfClient::new(binary()).with_temporary_directory(dir.path());
    let source = report("report-old.pdf");
    let direct = pdf_diff::load_document_from_bytes(&source, &Limits::default()).unwrap();
    let remote = client.inspect_bytes(&source, &Limits::default()).unwrap();
    assert_eq!(
        serde_json::to_value(&remote.value).unwrap(),
        serde_json::to_value(&direct).unwrap()
    );
    assert_eq!(remote.staging.input_bytes, source.len() as u64);
    assert!(remote.execution.peak_memory_bytes > 0);
    empty(dir.path());
    for page in &direct.pages {
        let local = pdf_diff::text_layer_from_bytes(&source, page.index).unwrap();
        let remote = client.text_layer_bytes(&source, page.index).unwrap();
        assert_eq!(
            serde_json::to_value(local).unwrap(),
            serde_json::to_value(remote.value).unwrap()
        );
        empty(dir.path());
    }
    for dpi in [false, true] {
        let source = fixture::fixture("user_unit_scale", true);
        let local = if dpi {
            pdf_diff::render_page_at_dpi_from_bytes(&source, 1, 72)
        } else {
            pdf_diff::render_page_from_bytes(&source, 1, 400)
        }
        .unwrap();
        let remote = if dpi {
            client.render_bytes_at_dpi(&source, 1, 72)
        } else {
            client.render_bytes(&source, 1, 400)
        }
        .unwrap();
        assert_eq!(remote.value.png, local.png);
        assert_eq!(remote.value.viewport, local.viewport);
        assert_eq!(remote.value.viewport.user_unit, Some(3.0));
        empty(dir.path());
    }
}

#[test]
fn typed_byte_comparison_keeps_names_options_limits_and_review_status() {
    let root = tempfile::tempdir().unwrap();
    let client = IsolatedPdfClient::new(binary()).with_temporary_directory(root.path());
    for name in [
        "user_unit_scale",
        "checkbox_export",
        "form_hybrid_xfa",
        "form_partial_property",
        "form_stream_reencoded",
        "form_deep_property",
        "layout_units",
        "layout_units_edit",
        "space_units",
        "widget_units",
        "widget_units_edit",
    ] {
        let old = fixture::fixture(name, false);
        let new = fixture::fixture(name, true);
        let mut options = DiffOptions::default();
        options.visual.as_mut().unwrap().dpi = 72;
        let direct = pdf_diff::diff_bytes(
            "Old document",
            "New document",
            &old,
            &new,
            &Limits::default(),
            &options,
        )
        .unwrap();
        if name == "form_deep_property" {
            assert_eq!(direct.summary.canonical_form_fields_changed, 1);
            assert!(direct
                .comparison
                .as_ref()
                .unwrap()
                .canonical_form_review_reasons
                .is_empty());
            fn depth(value: &serde_json::Value) -> usize {
                match value {
                    serde_json::Value::Array(values) => {
                        1 + values.iter().map(depth).max().unwrap_or(0)
                    }
                    serde_json::Value::Object(values) => {
                        1 + values.values().map(depth).max().unwrap_or(0)
                    }
                    _ => 0,
                }
            }
            assert!(depth(&serde_json::to_value(&direct).unwrap()) > 128);
        }
        let remote = client
            .diff_bytes(
                "Old document",
                "New document",
                &old,
                &new,
                &Limits::default(),
                &options,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&remote.value).unwrap(),
            serde_json::to_value(&direct).unwrap(),
            "{name}"
        );
        assert_eq!(remote.value.old_file, "Old document");
        assert_eq!(remote.value.new_file, "New document");
        assert!(!serde_json::to_string(&remote.value)
            .unwrap()
            .contains(root.path().to_str().unwrap()));
        assert_eq!(
            remote.exit_code,
            if direct.is_comparison_complete() {
                1
            } else {
                3
            }
        );
        empty(root.path());
    }
    let source = report("report-old.pdf");
    let options = DiffOptions {
        visual: None,
        ..DiffOptions::default()
    };
    let remote = client
        .diff_bytes(
            "same",
            "same",
            &source,
            &source,
            &Limits::default(),
            &options,
        )
        .unwrap();
    assert!(!remote.value.has_changes());
    assert!(remote.value.comparison.unwrap().identical_source);
    empty(root.path());
    let mut modified = source.clone();
    modified.extend_from_slice(b"\n% representation\n");
    let remote = client
        .diff_bytes(
            "old",
            "new",
            &source,
            &modified,
            &Limits::default(),
            &options,
        )
        .unwrap();
    assert_eq!(remote.exit_code, 3);
    assert!(!remote.value.is_comparison_complete());
    empty(root.path());
    let source = report("report-new.pdf");
    let count = pdf_diff::load_document_from_bytes(&source, &Limits::default())
        .unwrap()
        .pages
        .len();
    assert!(count > 1);
    let limits = Limits {
        max_pages: count - 1,
        ..Limits::default()
    };
    assert!(client.inspect_bytes(&source, &limits).is_err());
    empty(root.path());
}

#[test]
fn every_byte_worker_failure_removes_private_inputs_and_captures() {
    let root = tempfile::tempdir().unwrap();
    let source = report("report-old.pdf");
    let base = IsolatedPdfClient::new(binary()).with_temporary_directory(root.path());
    assert!(base
        .inspect_bytes(b"not a PDF", &Limits::default())
        .is_err());
    empty(root.path());
    assert!(base.render_bytes(&source, 0, 400).is_err());
    empty(root.path());
    assert!(base.text_layer_bytes(&source, 0).is_err());
    empty(root.path());
    for limits in [
        ProcessLimits {
            timeout: Duration::from_millis(1),
            ..ProcessLimits::default()
        },
        ProcessLimits {
            max_memory_bytes: 1,
            ..ProcessLimits::default()
        },
        ProcessLimits {
            max_output_bytes: 1024,
            ..ProcessLimits::default()
        },
    ] {
        let client = base.clone().with_process_limits(limits);
        assert!(client.inspect_bytes(&source, &Limits::default()).is_err());
        empty(root.path());
        assert!(client.render_bytes(&source, 1, 400).is_err());
        empty(root.path());
    }
    let invalid = IsolatedPdfClient::new(root.path().join("missing-executable"))
        .with_temporary_directory(root.path());
    assert!(invalid.inspect_bytes(&source, &Limits::default()).is_err());
    empty(root.path());
    let limits = Limits {
        max_input_mb: 1,
        ..Limits::default()
    };
    assert!(base
        .inspect_bytes(&vec![0; 1024 * 1024 + 1], &limits)
        .is_err());
    empty(root.path());
    let options = DiffOptions {
        font_size_tolerance: f64::NAN,
        ..DiffOptions::default()
    };
    assert!(base
        .diff_bytes("a", "b", &source, &source, &Limits::default(), &options)
        .is_err());
    empty(root.path());
    assert!(base.inspect_bytes(&source, &Limits::default()).is_ok());
    empty(root.path());
}

#[test]
fn private_protocol_rejects_wrong_versions_oversized_requests_and_unknown_coordinates() {
    let valid = serde_json::json!({"version":99,"operation":{"operation":"inspect","limits":{"max_input_mb":512,"max_pages":5000,"max_blocks_per_page":20000}},"input_lengths":[0]});
    for header in [
        serde_json::to_vec(&valid).unwrap(),
        vec![b' '; 1024 * 1024 + 1],
    ] {
        let mut input = tempfile::NamedTempFile::new().unwrap();
        input
            .write_all(&(header.len() as u32).to_be_bytes())
            .unwrap();
        input.write_all(&header).unwrap();
        let output = Command::new(binary())
            .arg(REQUEST_FLAG)
            .stdin(Stdio::from(input.reopen().unwrap()))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    let header=serde_json::to_vec(&serde_json::json!({"version":2,"operation":{"operation":"inspect","limits":{"max_input_mb":512,"max_pages":5000,"max_blocks_per_page":20000}},"input_lengths":[536870912]})).unwrap();
    let mut input = tempfile::NamedTempFile::new().unwrap();
    input
        .write_all(&(header.len() as u32).to_be_bytes())
        .unwrap();
    input.write_all(&header).unwrap();
    input.write_all(b"tiny").unwrap();
    let output = Command::new(binary())
        .arg(REQUEST_FLAG)
        .stdin(Stdio::from(input.reopen().unwrap()))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Truncated PDF worker input"));
    let source = report("report-old.pdf");
    let diff = pdf_diff::diff_bytes(
        "a",
        "a",
        &source,
        &source,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    let mut json = serde_json::to_value(diff).unwrap();
    json["coordinate_system"] = "screen_pixels".into();
    assert!(serde_json::from_value::<pdf_diff::DocumentDiff>(json).is_err());
}
