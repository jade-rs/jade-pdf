//! Typed byte operations. Anonymous spools keep input and output data unnamed.
use super::{supervise_in, ExecutionStats, IsolatedOutput, ProcessLimits};
use crate::{DiffOptions, DocumentDiff, DocumentIr, Limits, PageViewport, RenderedPage, TextRun};
use anyhow::{ensure, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;
use tempfile::TempDir;

pub const REQUEST_FLAG: &str = "--internal-pdf-request";
const VERSION: u32 = 2;
const REQUEST_BYTES: usize = 1024 * 1024;

/// The companion executable must be trusted. Parsing runs in its process;
/// bounded input staging and response decoding run in the calling process.
#[derive(Debug, Clone)]
pub struct IsolatedPdfClient {
    executable: PathBuf,
    process_limits: ProcessLimits,
    temporary_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct StagingStats {
    pub input_bytes: u64,
    /// Copying before worker startup, outside the child's execution deadline.
    pub elapsed_ms: u128,
}

pub struct IsolatedResponse<T> {
    pub value: T,
    /// Comparisons preserve 0/1/3. Failed/limited byte operations are errors.
    pub exit_code: i32,
    pub stderr: Vec<u8>,
    pub execution: ExecutionStats,
    pub staging: StagingStats,
}

impl IsolatedPdfClient {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            process_limits: ProcessLimits::default(),
            temporary_directory: None,
        }
    }
    pub fn with_process_limits(mut self, limits: ProcessLimits) -> Self {
        self.process_limits = limits;
        self
    }
    /// Use this root for anonymous input/capture spools. Operation directories
    /// are removed before return; a cleanup failure is reported as an error.
    pub fn with_temporary_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.temporary_directory = Some(directory.into());
        self
    }

    fn stage(&self, inputs: &[&[u8]], limits: &Limits, operation: Operation) -> Result<Staged> {
        self.process_limits.validate()?;
        limits.validate()?;
        for bytes in inputs {
            crate::scan::ensure_size(bytes.len() as u64, limits)?;
        }
        let start = Instant::now();
        let request = serde_json::to_vec(&Request {
            version: VERSION,
            operation,
            input_lengths: inputs.iter().map(|b| b.len() as u64).collect(),
        })?;
        ensure!(
            request.len() <= REQUEST_BYTES,
            "PDF worker request exceeds 1MiB"
        );
        let directory = match &self.temporary_directory {
            Some(path) => tempfile::tempdir_in(path)?,
            None => tempfile::tempdir()?,
        };
        let copy = || -> Result<File> {
            let mut input = super::anonymous_file(Some(directory.path()))?;
            input.write_all(&(request.len() as u32).to_be_bytes())?;
            input.write_all(&request)?;
            for bytes in inputs {
                input.write_all(bytes)?;
            }
            input.seek(SeekFrom::Start(0))?;
            Ok(input)
        };
        let input = match copy() {
            Ok(input) => input,
            Err(error) => return cleanup(directory, Err(error)),
        };
        Ok(Staged {
            directory,
            input,
            stats: StagingStats {
                input_bytes: inputs.iter().map(|b| b.len() as u64).sum(),
                elapsed_ms: start.elapsed().as_millis(),
            },
        })
    }

    pub fn inspect_bytes(
        &self,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<IsolatedResponse<DocumentIr>> {
        let staged = self.stage(
            &[bytes],
            limits,
            Operation::Inspect {
                limits: limits.clone(),
            },
        )?;
        staged.finish(|staged| response(self.request(staged)?, staged.stats))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn diff_bytes(
        &self,
        old_name: &str,
        new_name: &str,
        old: &[u8],
        new: &[u8],
        limits: &Limits,
        options: &DiffOptions,
    ) -> Result<IsolatedResponse<DocumentDiff>> {
        options.validate()?;
        ensure!(
            old_name.len().saturating_add(new_name.len()) <= REQUEST_BYTES / 6,
            "PDF worker names exceed request budget"
        );
        let operation = Operation::Compare {
            old_name: old_name.into(),
            new_name: new_name.into(),
            limits: limits.clone(),
            options: options.clone(),
        };
        let staged = self.stage(&[old, new], limits, operation)?;
        staged.finish(|staged| {
            let result: IsolatedResponse<DocumentDiff> =
                response(self.request(staged)?, staged.stats)?;
            ensure!(
                result.value.schema_version == crate::DIFF_SCHEMA_VERSION,
                "Unsupported worker diff schema"
            );
            let expected = if !result.value.is_comparison_complete() {
                3
            } else {
                i32::from(result.value.has_changes())
            };
            ensure!(
                result.exit_code == expected,
                "Worker comparison status disagrees with its result"
            );
            Ok(result)
        })
    }

    pub fn text_layer_bytes(
        &self,
        bytes: &[u8],
        page: usize,
    ) -> Result<IsolatedResponse<Vec<TextRun>>> {
        crate::render::check_page_number(page)?;
        let staged = self.stage(&[bytes], &Limits::default(), Operation::TextLayer { page })?;
        staged.finish(|staged| response(self.request(staged)?, staged.stats))
    }
    pub fn render_bytes(
        &self,
        bytes: &[u8],
        page: usize,
        width: u32,
    ) -> Result<IsolatedResponse<RenderedPage>> {
        self.render(bytes, page, Scale::Width(width))
    }
    pub fn render_bytes_at_dpi(
        &self,
        bytes: &[u8],
        page: usize,
        dpi: u32,
    ) -> Result<IsolatedResponse<RenderedPage>> {
        self.render(bytes, page, Scale::Dpi(dpi))
    }

    fn render(
        &self,
        bytes: &[u8],
        page: usize,
        scale: Scale,
    ) -> Result<IsolatedResponse<RenderedPage>> {
        match &scale {
            Scale::Width(width) => crate::render::check_render_args(page, *width)?,
            Scale::Dpi(dpi) => crate::render::check_dpi_args(page, *dpi)?,
        };
        let staged = self.stage(
            &[bytes],
            &Limits::default(),
            Operation::Render { page, scale },
        )?;
        staged.finish(|staged| {
            let output = self.request(staged)?;
            let split = output
                .stdout
                .iter()
                .position(|&b| b == b'\n')
                .context("Missing PNG response frame")?;
            ensure!(
                split <= REQUEST_BYTES,
                "PNG metadata exceeds response budget"
            );
            let metadata: RenderMetadata = decode(&output.stdout[..split])?;
            ensure!(
                output.stdout.len() - split - 1 == metadata.png_bytes,
                "Worker PNG length disagrees with metadata"
            );
            let mut png = output.stdout;
            png.drain(..split + 1);
            ensure!(
                png.len() >= 24 && png.starts_with(b"\x89PNG\r\n\x1a\n"),
                "Worker returned invalid PNG"
            );
            ensure!(
                metadata.width > 0
                    && metadata.height > 0
                    && metadata.viewport.width == metadata.width
                    && metadata.viewport.height == metadata.height
                    && u32::from_be_bytes(png[16..20].try_into()?) == metadata.width
                    && u32::from_be_bytes(png[20..24].try_into()?) == metadata.height,
                "Worker PNG dimensions disagree with metadata"
            );
            Ok(IsolatedResponse {
                value: RenderedPage {
                    width: metadata.width,
                    height: metadata.height,
                    viewport: metadata.viewport,
                    png,
                },
                exit_code: output.exit_code,
                stderr: output.stderr,
                execution: output.stats,
                staging: staged.stats,
            })
        })
    }

    fn request(&self, staged: &Staged) -> Result<IsolatedOutput> {
        let mut command = Command::new(&self.executable);
        command
            .arg(REQUEST_FLAG)
            .env("JADE_PDF_PARENT_PID", std::process::id().to_string())
            .stdin(Stdio::from(staged.input.try_clone()?));
        let result = supervise_in(
            command,
            &self.process_limits,
            None,
            Some(staged.directory.path()),
        )?;
        ensure!(
            result.exit_code != 2,
            "PDF byte worker failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(result)
    }
}

struct Staged {
    directory: TempDir,
    input: File,
    stats: StagingStats,
}
impl Staged {
    fn finish<T>(self, operation: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let result = operation(&self);
        let Self {
            directory, input, ..
        } = self;
        drop(input);
        cleanup(directory, result)
    }
}
fn cleanup<T>(directory: TempDir, result: Result<T>) -> Result<T> {
    let path = directory.path().to_owned();
    match directory.close() {
        Ok(()) => result,
        Err(error) => {
            let message = format!(
                "Cannot remove private PDF staging {}: {error}",
                path.display()
            );
            match result {
                Ok(_) => Err(anyhow::anyhow!(message)),
                Err(primary) => Err(primary.context(message)),
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u32,
    operation: Operation,
    input_lengths: Vec<u64>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Inspect {
        limits: Limits,
    },
    Compare {
        old_name: String,
        new_name: String,
        limits: Limits,
        options: DiffOptions,
    },
    TextLayer {
        page: usize,
    },
    Render {
        page: usize,
        scale: Scale,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Scale {
    Width(u32),
    Dpi(u32),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    version: u32,
    value: T,
}
#[derive(Serialize, Deserialize)]
struct RenderMetadata {
    width: u32,
    height: u32,
    viewport: PageViewport,
    png_bytes: usize,
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    // Canonical dictionaries have two wrapper levels per supported PDF level.
    check_nesting(bytes, 256)?;
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    decoder.disable_recursion_limit();
    let envelope =
        Envelope::<T>::deserialize(&mut decoder).context("Invalid PDF worker response")?;
    decoder.end()?;
    ensure!(
        envelope.version == VERSION,
        "Unsupported PDF worker response version"
    );
    Ok(envelope.value)
}
fn response<T: DeserializeOwned>(
    output: IsolatedOutput,
    staging: StagingStats,
) -> Result<IsolatedResponse<T>> {
    Ok(IsolatedResponse {
        value: decode(&output.stdout)?,
        exit_code: output.exit_code,
        stderr: output.stderr,
        execution: output.stats,
        staging,
    })
}

/// Internal companion protocol. PDF parsing/rendering runs only in the worker.
#[doc(hidden)]
pub fn execute_request() -> Result<i32> {
    let mut stdin = std::io::stdin().lock();
    let mut prefix = [0u8; 4];
    stdin.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    ensure!(length <= REQUEST_BYTES, "PDF worker request exceeds 1MiB");
    let mut header = vec![0; length];
    stdin.read_exact(&mut header)?;
    let request: Request = serde_json::from_slice(&header)?;
    ensure!(
        request.version == VERSION,
        "Unsupported PDF worker request version"
    );
    let count = if matches!(request.operation, Operation::Compare { .. }) {
        2
    } else {
        1
    };
    ensure!(
        request.input_lengths.len() == count,
        "Wrong number of PDF worker inputs"
    );
    let limits = match &request.operation {
        Operation::Inspect { limits } | Operation::Compare { limits, .. } => limits.clone(),
        _ => Limits::default(),
    };
    limits.validate()?;
    match &request.operation {
        Operation::Compare { options, .. } => options.validate()?,
        Operation::TextLayer { page } => crate::render::check_page_number(*page)?,
        Operation::Render { page, scale } => match scale {
            Scale::Width(width) => crate::render::check_render_args(*page, *width)?,
            Scale::Dpi(dpi) => crate::render::check_dpi_args(*page, *dpi)?,
        },
        _ => {}
    }
    for &length in &request.input_lengths {
        crate::scan::ensure_size(length, &limits)?;
    }
    let mut inputs = Vec::new();
    for length in request.input_lengths {
        let mut bytes = Vec::new();
        let mut remaining = length;
        let mut chunk = [0u8; 8192];
        while remaining > 0 {
            let needed = remaining.min(chunk.len() as u64) as usize;
            let read = stdin.read(&mut chunk[..needed])?;
            ensure!(read > 0, "Truncated PDF worker input");
            bytes.extend_from_slice(&chunk[..read]);
            remaining -= read as u64;
        }
        inputs.push(bytes);
    }
    ensure!(stdin.read(&mut [0u8; 1])? == 0, "Trailing PDF worker input");
    drop(stdin);
    match request.operation {
        Operation::Inspect { limits } => {
            write_response(crate::load_document_from_bytes(&inputs[0], &limits)?, None)?;
            Ok(0)
        }
        Operation::Compare {
            old_name,
            new_name,
            limits,
            options,
        } => {
            let value = crate::diff_bytes(
                &old_name, &new_name, &inputs[0], &inputs[1], &limits, &options,
            )?;
            let code = if !value.is_comparison_complete() {
                3
            } else {
                i32::from(value.has_changes())
            };
            write_response(value, None)?;
            Ok(code)
        }
        Operation::TextLayer { page } => {
            write_response(crate::text_layer_from_bytes(&inputs[0], page)?, None)?;
            Ok(0)
        }
        Operation::Render { page, scale } => {
            let image = match scale {
                Scale::Width(width) => crate::render_page_from_bytes(&inputs[0], page, width)?,
                Scale::Dpi(dpi) => crate::render_page_at_dpi_from_bytes(&inputs[0], page, dpi)?,
            };
            write_response(
                RenderMetadata {
                    width: image.width,
                    height: image.height,
                    viewport: image.viewport,
                    png_bytes: image.png.len(),
                },
                Some(&image.png),
            )?;
            Ok(0)
        }
    }
}
fn write_response<T: Serialize>(value: T, binary: Option<&[u8]>) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(
        &mut stdout,
        &Envelope {
            version: VERSION,
            value,
        },
    )?;
    stdout.write_all(b"\n")?;
    if let Some(bytes) = binary {
        stdout.write_all(bytes)?;
    }
    stdout.flush()?;
    Ok(())
}
fn check_nesting(bytes: &[u8], maximum: usize) -> Result<()> {
    let mut depth = 0usize;
    let mut string = false;
    let mut escaped = false;
    for &byte in bytes {
        if string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
            }
        } else {
            match byte {
                b'"' => string = true,
                b'{' | b'[' => {
                    depth += 1;
                    ensure!(
                        depth <= maximum,
                        "PDF worker response exceeds JSON nesting budget"
                    );
                }
                b'}' | b']' => {
                    depth = depth.checked_sub(1).context("Unbalanced PDF worker JSON")?
                }
                _ => {}
            }
        }
    }
    ensure!(depth == 0 && !string, "Unbalanced PDF worker JSON");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn live_byte_inputs_have_no_directory_entries_and_are_closed_before_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let client =
            IsolatedPdfClient::new("unused-companion").with_temporary_directory(root.path());
        let staged = client
            .stage(
                &[b"sensitive PDF bytes"],
                &Limits::default(),
                Operation::Inspect {
                    limits: Limits::default(),
                },
            )
            .unwrap();
        assert_eq!(
            std::fs::read_dir(staged.directory.path()).unwrap().count(),
            0
        );
        staged
            .finish(|staged| {
                let mut bytes = Vec::new();
                staged.input.try_clone()?.read_to_end(&mut bytes)?;
                ensure!(
                    bytes.ends_with(b"sensitive PDF bytes"),
                    "Anonymous input was lost"
                );
                Ok(())
            })
            .unwrap();
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn response_depth_guard_ignores_quoted_brackets_and_bounds_recursion() {
        let strings = serde_json::to_vec(&vec!["[{}] \\\""; 512]).unwrap();
        assert!(check_nesting(&strings, 1).is_ok());
        for depth in [128, 256, 257] {
            let bytes = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
            assert_eq!(check_nesting(bytes.as_bytes(), 256).is_ok(), depth <= 256);
        }
        assert!(check_nesting(b"{]}", 256).is_err());
    }
}
