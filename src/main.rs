//! 通常の PDF → DOCX 変換コマンド。
mod cli;
#[cfg(test)]
mod cli_tests;
mod commit_space;
#[cfg(test)]
mod commit_space_tests;
mod input_snapshot;
#[cfg(test)]
mod input_snapshot_tests;
mod interrupt;
#[cfg(test)]
mod memory_cleanup_tests;
mod memory_group;
#[cfg(test)]
mod memory_group_tests;
#[cfg(all(test, target_os = "linux"))]
mod memory_parent_death_tests;
#[cfg(test)]
mod memory_supervisor_tests;
mod ocr;
mod process_group;
mod publication_receipt;
mod supervisor;
#[cfg(test)]
mod supervisor_tests;
mod worker;
#[cfg(test)]
mod worker_tests;
mod workspace;
mod workspace_cleanup;
#[cfg(unix)]
mod workspace_identity;
#[cfg(all(test, unix))]
mod workspace_identity_tests;
mod workspace_removal;
mod workspace_reservation;
#[cfg(test)]
mod workspace_startup_tests;
#[cfg(test)]
mod workspace_tests;

use jade_pdf2docx_docx::{
    MinimumPdfToDocxRequest, ParseCancellation, ParseOptions,
    convert_minimum_pdf_to_docx_with_options,
};
use std::{env, ffi::OsString, process::ExitCode};

/// 使用法と監督の起動から保存完了までの待機期限を表示する。
const USAGE: &str = "usage: jade-pdf2docx <input.pdf> <output.docx> [--strict] [--debug-output] [--input-sha256 <64-lowercase-hex> [--publication-receipt <token:SHA-256>]] [--timeout-ms <milliseconds>] [--max-memory-mib <MiB> --memory-cgroup-parent <absolute-path>] [--ocr --ocr-model <model.onnx> --ocr-charset <charset.yaml> [--ocr-dpi <1..600>] [--ocr-direction horizontal|vertical] [--ocr-paper-shading]]\n       jade-pdf2docx --cleanup-workspace <absolute-workspace-path>\n       jade-pdf2docx --cleanup-memory-cgroup <absolute-cgroup-path>\n\nConvert PDF body content to DOCX.\n  --input-sha256  Verify a bounded private PDF snapshot before conversion.\n  --publication-receipt  Bind published DOCX to a durable request (token:SHA-256).\n  --strict        Reject detected quality loss before writing output.\n  --debug-output  Include conversion metadata for debugging.\n  --timeout-ms    Budget for supervisor startup, conversion and commit (default: 300000 ms).\n  --max-memory-mib  Linux cgroup v2 conversion group limit (requires delegated parent).\n  --memory-cgroup-parent  Absolute directory with memory controller delegated and enabled.\n  --ocr           Recognize image-only pages (requires ocr-onnx and pdftoppm).\n  --ocr-model     Local PARSeq ONNX model (required with --ocr).\n  --ocr-charset   Matching charset YAML (required with --ocr).\n  --ocr-dpi       Raster resolution, 1..600 (default: 150).\n  --ocr-direction  Explicit text direction (default: horizontal); vertical columns read right to left.\n  --ocr-paper-shading  Enable paper shadow normalization (default: disabled).\n  --cleanup-workspace  Remove a verified inactive workspace at the exact path.\n  --cleanup-memory-cgroup  Remove one registered, inactive and empty Linux memory cgroup.\n  --help          Show this help.\n  --version       Show version.";

/// worker内で変換し、品質診断を親に引き継ぐ。
fn convert_inline(request: &cli::Request) -> ExitCode {
    let workspace_path = env::var_os(workspace::ENV).map(std::path::PathBuf::from);
    let _workspace_lease = match workspace::worker_lease(workspace_path.as_deref()) {
        Ok(lease) => lease,
        Err(error) => {
            eprintln!("conversion failed: cannot acquire workspace lease: {error}");
            return ExitCode::from(1);
        }
    };
    let options = ParseOptions {
        strict: request.strict,
        ..Default::default()
    };
    if let Err(error) = jade_pdf2docx_docx::validate_distinct_paths(&request.input, &request.output)
    {
        eprintln!("conversion failed: {error}");
        return ExitCode::from(1);
    }
    let snapshot = match request
        .input_sha256
        .as_deref()
        .map(|digest| {
            input_snapshot::Snapshot::capture(
                &request.input,
                options.limits.max_input_bytes,
                digest,
            )
        })
        .transpose()
    {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("conversion failed: {error}");
            return ExitCode::from(1);
        }
    };
    let conversion = MinimumPdfToDocxRequest {
        input_pdf_path: snapshot.as_ref().map_or_else(
            || request.input.clone(),
            |snapshot| snapshot.path().to_path_buf(),
        ),
        output_docx_path: request.output.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: !request.debug,
    };
    let result = if request.ocr.is_some() {
        ocr::convert(request, &conversion, &options)
    } else {
        convert_minimum_pdf_to_docx_with_options(&conversion, &options)
    };
    match result {
        Ok(report) => {
            for issue in &report.quality.issues {
                eprintln!("quality warning: {issue}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("conversion failed: {error}");
            ExitCode::from(1)
        }
    }
}

/// 親の生存を監視し、一時領域を所有して変換workerを回収する。
fn convert_supervised(request: &cli::Request) -> ExitCode {
    let cancellation = ParseCancellation::new();
    let result = (|| -> Result<(), worker::WorkerError> {
        interrupt::install(&cancellation)?;
        // 一時領域を破棄してから、親死亡時に残る描画子孫と監督自身を停止する。
        let _parent_lease = supervisor::watch_parent(&cancellation)?;
        let _workspace = supervisor::start_workspace(&cancellation)?;
        let executable = env::current_exe()?;
        worker::run_conversion(&executable, request, &cancellation)
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(worker::WorkerError::Interrupted) => ExitCode::from(130),
        Err(error) => {
            eprintln!("conversion failed: {error}");
            ExitCode::from(1)
        }
    }
}

/// OS引数を検証し、workerの正常終了と出力公開を成功コードの条件にする。
fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match cli::parse(&args) {
        Ok(cli::Action::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(cli::Action::Version) => {
            println!("jade-pdf2docx {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(cli::Action::CleanupWorkspace(path)) => match workspace_cleanup::cleanup(&path) {
            Ok(()) => {
                println!("removed workspace: {}", path.display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("workspace cleanup failed: {error}");
                ExitCode::from(1)
            }
        },
        Ok(cli::Action::CleanupMemoryGroup(path)) => match memory_group::cleanup(&path) {
            Ok(()) => {
                println!("removed memory cgroup: {}", path.display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("memory cgroup cleanup failed: {error}");
                ExitCode::from(1)
            }
        },
        Ok(cli::Action::Worker(request)) => convert_inline(&request),
        Ok(cli::Action::Supervisor(request)) => convert_supervised(&request),
        Ok(cli::Action::Convert(request)) => {
            let cancellation = ParseCancellation::new();
            let result = interrupt::install(&cancellation)
                .map_err(worker::WorkerError::from)
                .and_then(|()| env::current_exe().map_err(worker::WorkerError::from))
                .and_then(|executable| worker::run_isolated(&executable, &request, &cancellation));
            match result {
                Ok(()) => {
                    println!("output: {}", request.output.display());
                    ExitCode::SUCCESS
                }
                Err(worker::WorkerError::Interrupted) => {
                    eprintln!("conversion interrupted");
                    ExitCode::from(130)
                }
                Err(error) => {
                    eprintln!("conversion failed: {error}");
                    ExitCode::from(1)
                }
            }
        }
        Err(()) => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
