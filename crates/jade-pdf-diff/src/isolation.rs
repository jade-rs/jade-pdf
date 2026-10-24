//! Run the companion CLI in a disposable process before accepting its output.
//! This contains parser failures; it is not a filesystem/network security sandbox.
use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

pub const WORKER_FLAG: &str = "--internal-pdf-worker";

mod client;
#[doc(hidden)]
pub use client::{execute_request, REQUEST_FLAG};
pub use client::{IsolatedPdfClient, IsolatedResponse, StagingStats};

/// Budgets for one child CLI operation, including SDK startup and serialization.
#[derive(Debug, Clone)]
pub struct ProcessLimits {
    pub timeout: Duration,
    /// Linux also enforces this as a virtual-address-space limit. Other platforms
    /// monitor process memory; short-lived overshoot between samples is possible.
    pub max_memory_bytes: u64,
    /// Combined stdout, stderr and an optional rendered PNG, before publication.
    pub max_output_bytes: u64,
}

impl Default for ProcessLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            max_memory_bytes: 2 * 1024 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
        }
    }
}

impl ProcessLimits {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.timeout.is_zero() && self.max_memory_bytes > 0 && self.max_output_bytes > 0,
            "Process resource limits must be positive"
        );
        ensure!(
            self.max_output_bytes <= usize::MAX as u64,
            "Process output limit exceeds platform address range"
        );
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct ExecutionStats {
    pub elapsed_ms: u128,
    /// Maximum observed live memory or OS-reported peak RSS; this is not a
    /// portable hard memory ceiling. macOS also samples physical footprint.
    pub peak_memory_bytes: u64,
    pub output_bytes: u64,
}

#[derive(Debug)]
pub struct IsolatedOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stats: ExecutionStats,
}

/// `executable` must be a trusted `pdf-diff` companion binary. Existing CLI
/// arguments are supported. Failed/limited workers never publish partial stdout
/// or replace an existing render destination. Raw in-process APIs are unaffected.
pub fn run_pdf_worker(
    executable: &Path,
    arguments: &[OsString],
    limits: &ProcessLimits,
) -> Result<IsolatedOutput> {
    limits.validate()?;
    ensure!(
        !arguments.iter().any(|a| a == WORKER_FLAG),
        "Internal worker flag is reserved"
    );
    let mut arguments = arguments.to_vec();
    let destination = if arguments.len() == 5 && arguments[0] == "--render" {
        Some(std::path::PathBuf::from(&arguments[4]))
    } else {
        None
    };
    let image = if let Some(path) = &destination {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let temporary =
            NamedTempFile::new_in(parent).context("Cannot create atomic render destination")?;
        arguments[4] = temporary.path().as_os_str().to_owned();
        Some(temporary)
    } else {
        None
    };
    let mut command = Command::new(executable);
    command
        .arg(WORKER_FLAG)
        .args(&arguments)
        .env("JADE_PDF_PARENT_PID", std::process::id().to_string())
        .stdin(Stdio::null());
    let mut result = supervise(command, limits, image.as_ref())?;
    if let (Some(image), Some(destination)) = (image, destination) {
        if result.exit_code != 0 {
            return Ok(result);
        }
        let mut bytes = [0u8; 24];
        std::fs::File::open(image.path())?.read_exact(&mut bytes)?;
        ensure!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "PDF worker returned an invalid PNG"
        );
        let width = u32::from_be_bytes(bytes[16..20].try_into()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into()?);
        ensure!(width > 0 && height > 0, "PDF worker returned an empty PNG");
        result.stdout = format!("{width}x{height} -> {}\n", destination.display()).into_bytes();
        result.stats.output_bytes = result.stdout.len() as u64
            + result.stderr.len() as u64
            + image.as_file().metadata()?.len();
        ensure!(
            result.stats.output_bytes <= limits.max_output_bytes,
            "PDF worker exceeded output limit; no result published"
        );
        image
            .persist(&destination)
            .map_err(|e| e.error)
            .context("Cannot publish rendered PNG")?;
    }
    Ok(result)
}

struct Worker {
    child: Child,
    reaped: bool,
}

fn anonymous_file(directory: Option<&Path>) -> Result<std::fs::File> {
    let file = match directory {
        Some(path) => tempfile::tempfile_in(path)?,
        None => tempfile::tempfile()?,
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            file.metadata()?.nlink() == 0,
            "Temporary PDF spool could not be unlinked"
        );
    }
    Ok(file)
}

impl Drop for Worker {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl Worker {
    #[cfg(unix)]
    fn poll(&mut self) -> Result<Option<(ExitStatus, u64)>> {
        use std::os::unix::process::ExitStatusExt;
        let mut status = 0;
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        // SAFETY: wait4 targets our exclusively owned child. The kernel writes
        // status/rusage; a successful reap disarms Drop before the PID can recur.
        let result = unsafe {
            libc::wait4(
                self.child.id() as libc::pid_t,
                &mut status,
                libc::WNOHANG,
                usage.as_mut_ptr(),
            )
        };
        if result == 0 {
            return Ok(None);
        }
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ECHILD) {
                self.reaped = true;
            }
            if error.kind() == std::io::ErrorKind::Interrupted {
                return Ok(None);
            }
            return Err(error.into());
        }
        self.reaped = true;
        let usage = unsafe { usage.assume_init() };
        let peak = usage.ru_maxrss.max(0) as u64;
        #[cfg(not(target_os = "macos"))]
        let peak = peak.saturating_mul(1024);
        Ok(Some((ExitStatus::from_raw(status), peak)))
    }

    #[cfg(not(unix))]
    fn poll(&mut self) -> Result<Option<(ExitStatus, u64)>> {
        if let Some(status) = self.child.try_wait()? {
            self.reaped = true;
            Ok(Some((status, memory_usage(&self.child)?)))
        } else {
            Ok(None)
        }
    }
}

fn supervise(
    command: Command,
    limits: &ProcessLimits,
    artifact: Option<&NamedTempFile>,
) -> Result<IsolatedOutput> {
    limits.validate()?;
    supervise_in(command, limits, artifact, None)
}

fn supervise_in(
    mut command: Command,
    limits: &ProcessLimits,
    artifact: Option<&NamedTempFile>,
    directory: Option<&Path>,
) -> Result<IsolatedOutput> {
    limits.validate()?;
    let temporary = || anonymous_file(directory);
    let mut stdout = temporary()?;
    let mut stderr = temporary()?;
    command
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?));
    configure_os_limits(&mut command, limits)?;
    let started = Instant::now();
    let child = command
        .spawn()
        .context("Cannot start isolated PDF worker")?;
    let mut worker = Worker {
        child,
        reaped: false,
    };
    let mut peak = 0;
    let (status, _) = loop {
        let completed = worker.poll()?;
        if let Some((_, rss)) = &completed {
            peak = peak.max(*rss);
        }
        if started.elapsed() > limits.timeout {
            bail!(
                "PDF worker exceeded wall-clock limit ({} ms); no result published",
                limits.timeout.as_millis()
            );
        }
        let bytes = stdout
            .metadata()?
            .len()
            .saturating_add(stderr.metadata()?.len())
            .saturating_add(
                artifact
                    .map(|f| f.as_file().metadata().map(|m| m.len()))
                    .transpose()?
                    .unwrap_or(0),
            );
        ensure!(
            bytes <= limits.max_output_bytes,
            "PDF worker exceeded output limit ({} bytes); no result published",
            limits.max_output_bytes
        );
        if completed.is_none() {
            match memory_usage(&worker.child) {
                Ok(bytes) => peak = peak.max(bytes),
                Err(error) => {
                    // A process may exit between wait4 and the statistics call.
                    if let Some((status, rss)) = worker.poll()? {
                        peak = peak.max(rss);
                        ensure!(
                            peak <= limits.max_memory_bytes,
                            "PDF worker exceeded memory limit ({} bytes); no result published",
                            limits.max_memory_bytes
                        );
                        break (status, bytes);
                    }
                    return Err(
                        error.context("Cannot monitor PDF worker memory; no result published")
                    );
                }
            }
        }
        ensure!(peak <= limits.max_memory_bytes,"PDF worker exceeded memory limit ({} bytes); observed {peak} bytes; no result published",limits.max_memory_bytes);
        if let Some((status, _)) = completed {
            break (status, bytes);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output_bytes = stdout
        .metadata()?
        .len()
        .saturating_add(stderr.metadata()?.len())
        .saturating_add(
            artifact
                .map(|f| f.as_file().metadata().map(|m| m.len()))
                .transpose()?
                .unwrap_or(0),
        );
    ensure!(
        output_bytes <= limits.max_output_bytes,
        "PDF worker exceeded output limit; no result published"
    );
    ensure!(
        started.elapsed() <= limits.timeout,
        "PDF worker exceeded wall-clock limit; no result published"
    );
    let code = status.code().with_context(|| {
        format!("PDF worker terminated by a signal ({status}); no result published")
    })?;
    ensure!(
        (0..=3).contains(&code),
        "PDF worker exited unexpectedly ({code}); no result published"
    );
    let read = |file: &mut std::fs::File| -> Result<Vec<u8>> {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(limits.max_output_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= limits.max_output_bytes,
            "PDF worker output changed after completion"
        );
        Ok(bytes)
    };
    let stdout = if code == 2 {
        Vec::new()
    } else {
        read(&mut stdout)?
    };
    let stderr = read(&mut stderr)?;
    ensure!(
        started.elapsed() <= limits.timeout,
        "PDF worker exceeded wall-clock limit; no result published"
    );
    Ok(IsolatedOutput {
        exit_code: code,
        stdout,
        stderr,
        stats: ExecutionStats {
            elapsed_ms: started.elapsed().as_millis(),
            peak_memory_bytes: peak,
            output_bytes,
        },
    })
}

#[cfg(unix)]
fn configure_os_limits(command: &mut Command, limits: &ProcessLimits) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let file_limit = limits.max_output_bytes.max(128 * 1024 * 1024);
    let cpu_limit = limits.timeout.as_secs().saturating_add(2);
    #[cfg(target_os = "linux")]
    let memory_limit = limits.max_memory_bytes;
    #[cfg(target_os = "linux")]
    let parent = std::process::id() as libc::pid_t;
    // SAFETY: this post-fork closure only invokes async-signal-safe setrlimit
    // with precomputed scalar values; no allocator, locks, formatting or I/O.
    unsafe {
        command.pre_exec(move || {
            let set = |resource, value| {
                let mut inherited = std::mem::MaybeUninit::<libc::rlimit>::zeroed();
                if libc::getrlimit(resource, inherited.as_mut_ptr()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let inherited = inherited.assume_init();
                let value = (value as libc::rlim_t).min(inherited.rlim_max);
                let limit = libc::rlimit {
                    rlim_cur: value,
                    rlim_max: value,
                };
                if libc::setrlimit(resource, &limit) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            };
            set(libc::RLIMIT_CORE, 0)?;
            set(libc::RLIMIT_FSIZE, file_limit)?;
            set(libc::RLIMIT_CPU, cpu_limit)?;
            #[cfg(target_os = "linux")]
            {
                set(libc::RLIMIT_AS, memory_limit)?;
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    libc::_exit(2);
                }
            }
            Ok(())
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn configure_os_limits(_command: &mut Command, _limits: &ProcessLimits) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn memory_usage(child: &Child) -> Result<u64> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: public libproc ABI writes the version-2 rusage struct. libc's
    // rusage_info_t* is the documented opaque pointer to this storage.
    let code = unsafe {
        libc::proc_pid_rusage(
            child.id() as libc::c_int,
            libc::RUSAGE_INFO_V2,
            info.as_mut_ptr().cast(),
        )
    };
    ensure!(code == 0, "{}", std::io::Error::last_os_error());
    let info = unsafe { info.assume_init() };
    Ok(info.ri_resident_size.max(info.ri_phys_footprint))
}

#[cfg(target_os = "linux")]
fn memory_usage(child: &Child) -> Result<u64> {
    let text = std::fs::read_to_string(format!("/proc/{}/smaps_rollup", child.id()))?;
    let mut rss = None;
    let mut swap = None;
    for line in text.lines() {
        let mut values = line.split_whitespace();
        match values.next() {
            Some("Rss:") => {
                rss = Some(
                    values
                        .next()
                        .context("Missing worker RSS")?
                        .parse::<u64>()?,
                )
            }
            Some("Swap:") => {
                swap = Some(
                    values
                        .next()
                        .context("Missing worker swap")?
                        .parse::<u64>()?,
                )
            }
            _ => {}
        }
    }
    Ok(rss
        .context("No worker RSS")?
        .saturating_add(swap.context("No worker swap")?)
        .saturating_mul(1024))
}

#[cfg(windows)]
fn memory_usage(child: &Child) -> Result<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    let mut info = std::mem::MaybeUninit::<PROCESS_MEMORY_COUNTERS_EX>::zeroed();
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    unsafe {
        (*info.as_mut_ptr()).cb = size;
    }
    let ok = unsafe {
        GetProcessMemoryInfo(
            child.as_raw_handle().cast(),
            info.as_mut_ptr().cast::<PROCESS_MEMORY_COUNTERS>(),
            size,
        )
    };
    ensure!(ok != 0, "{}", std::io::Error::last_os_error());
    let info = unsafe { info.assume_init() };
    Ok(info
        .PrivateUsage
        .max(info.WorkingSetSize)
        .max(info.PeakWorkingSetSize) as u64)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn memory_usage(_child: &Child) -> Result<u64> {
    bail!("PDF process memory monitoring is unsupported on this platform")
}

/// Companion-worker entry point: stop if the supervising process disappears.
/// The hidden worker protocol is for a trusted executable, not an authentication
/// or filesystem/network boundary. Normal callers use `run_pdf_worker`.
pub fn watch_parent() -> Result<()> {
    let Some(parent) = std::env::var_os("JADE_PDF_PARENT_PID") else {
        return Ok(());
    };
    let parent = parent
        .to_str()
        .context("Invalid PDF supervisor PID")?
        .parse::<u32>()?;
    ensure!(parent > 0, "Invalid PDF supervisor PID");
    #[cfg(unix)]
    std::thread::Builder::new()
        .name("pdf-parent-watch".into())
        .stack_size(64 * 1024)
        .spawn(move || loop {
            if unsafe { libc::getppid() } != parent as libc::pid_t {
                unsafe {
                    libc::_exit(2);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        })?;
    #[cfg(windows)]
    {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
        };
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent) };
        ensure!(!handle.is_null(), "Cannot monitor PDF supervisor process");
        let owned = unsafe { OwnedHandle::from_raw_handle(handle.cast()) };
        std::thread::Builder::new()
            .name("pdf-parent-watch".into())
            .stack_size(64 * 1024)
            .spawn(move || {
                let _ = unsafe { WaitForSingleObject(owned.as_raw_handle().cast(), INFINITE) };
                // An exited parent or failed monitoring both stop the worker.
                std::process::exit(2);
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn child(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "isolation::tests::worker_fixture",
                "--nocapture",
            ])
            .env("JADE_ISOLATION_TEST_CHILD", mode);
        command
    }

    #[test]
    #[ignore = "Subprocess-only fault fixture; exercised by isolation supervisor tests"]
    fn worker_fixture() {
        let Ok(mode) = std::env::var("JADE_ISOLATION_TEST_CHILD") else {
            return;
        };
        if let Ok(path) = std::env::var("JADE_ISOLATION_TEST_PID") {
            std::fs::write(path, std::process::id().to_string()).unwrap();
        }
        println!("partial output must remain private");
        std::io::stdout().flush().unwrap();
        match mode.as_str() {
            "parent" => {
                child("watched")
                    .env("JADE_PDF_PARENT_PID", std::process::id().to_string())
                    .spawn()
                    .unwrap()
                    .wait()
                    .unwrap();
            }
            "watched" => {
                watch_parent().unwrap();
                std::thread::sleep(Duration::from_secs(5));
            }
            "sleep" => std::thread::sleep(Duration::from_secs(2)),
            "memory" => {
                let bytes = vec![0x5a; 128 * 1024 * 1024];
                std::hint::black_box(&bytes);
                std::thread::sleep(Duration::from_secs(2));
            }
            "output" => {
                std::io::stdout()
                    .write_all(&vec![b'x'; 2 * 1024 * 1024])
                    .unwrap();
            }
            "crash" => std::process::abort(),
            _ => panic!("unknown fixture"),
        }
    }

    #[test]
    fn deadline_memory_crash_and_output_failures_do_not_return_partial_results() {
        for mode in ["sleep", "memory", "crash", "output"] {
            let mut limits = ProcessLimits::default();
            if mode == "sleep" {
                limits.timeout = Duration::from_millis(100);
            }
            if mode == "memory" {
                limits.max_memory_bytes = 64 * 1024 * 1024;
            }
            if mode == "output" {
                limits.max_output_bytes = 1024;
            }
            let started = Instant::now();
            let error = supervise(child(mode), &limits, None).unwrap_err();
            let message = error.to_string();
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "{mode}: {message}"
            );
            if mode == "sleep" {
                assert!(message.contains("wall-clock limit"), "{message}");
            }
            if mode == "output" {
                assert!(message.contains("output limit"), "{message}");
            }
            if mode == "crash" {
                assert!(
                    message.contains("signal") || message.contains("unexpectedly"),
                    "{message}"
                );
            }
            if mode == "memory" {
                assert!(
                    message.contains("memory limit")
                        || message.contains("signal")
                        || message.contains("Cannot start"),
                    "{message}"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn timed_out_worker_is_killed_and_reaped() {
        let file = NamedTempFile::new().unwrap();
        let mut command = child("sleep");
        command.env("JADE_ISOLATION_TEST_PID", file.path());
        let limits = ProcessLimits {
            timeout: Duration::from_millis(500),
            ..ProcessLimits::default()
        };
        assert!(supervise(command, &limits, None).is_err());
        let pid = std::fs::read_to_string(file.path())
            .unwrap()
            .parse::<libc::pid_t>()
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    fn zero_process_budgets_are_rejected() {
        for limits in [
            ProcessLimits {
                timeout: Duration::ZERO,
                ..ProcessLimits::default()
            },
            ProcessLimits {
                max_memory_bytes: 0,
                ..ProcessLimits::default()
            },
            ProcessLimits {
                max_output_bytes: 0,
                ..ProcessLimits::default()
            },
        ] {
            assert!(limits.validate().is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn worker_stops_if_its_supervisor_is_killed() {
        let file = NamedTempFile::new().unwrap();
        let mut command = child("parent");
        command.env("JADE_ISOLATION_TEST_PID", file.path());
        let child = command.spawn().unwrap();
        let parent_pid = child.id();
        let mut parent = Worker {
            child,
            reaped: false,
        };
        let started = Instant::now();
        let pid = loop {
            if let Ok(pid) = std::fs::read_to_string(file.path()).unwrap().parse::<u32>() {
                if pid != parent_pid {
                    break pid;
                }
            }
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "Worker failed to start"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        parent.child.kill().unwrap();
        parent.child.wait().unwrap();
        parent.reaped = true;
        let stopped = Instant::now();
        loop {
            if unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
                break;
            }
            #[cfg(target_os = "linux")]
            if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
                if status
                    .lines()
                    .any(|line| line.starts_with("State:") && line.contains('Z'))
                {
                    break;
                }
            }
            assert!(
                stopped.elapsed() < Duration::from_secs(2),
                "Orphan worker remained alive"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
