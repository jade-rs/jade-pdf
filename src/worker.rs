//! 変換workerの停止・回収と、正常終了後の成果物公開。
use crate::cli::{Request, WORKER_FLAG};
use jade_pdf2docx_docx::{ParseCancellation, StagedDocxOutput};
use std::{
    fmt, io,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// worker起動・監督・公開の失敗。
#[derive(Debug)]
pub(crate) enum WorkerError {
    /// 停止signal受付の登録失敗。
    InterruptSetup(ctrlc::Error),
    /// 利用者の停止要求または親pipeの切断による停止。
    Interrupted,
    /// 子プロセス操作のI/O失敗。
    Io(io::Error),
    /// 出力準備または公開の失敗。
    Output(jade_pdf2docx_docx::ConvertError),
    /// 実行時間上限を観測した失敗。
    Timeout {
        /// 設定された上限。
        timeout: Duration,
    },
    /// カーネルが変換群の合算メモリ上限でOOMを発生させた失敗。
    MemoryLimit {
        /// 指定されたカーネル上限のbyte数。
        bytes: u64,
    },
    /// 親が出力を公開する前に監督が成功終了したprotocol違反。
    UnpublishedOutput,
    /// workerが非ゼロで終了した失敗。
    Failed {
        /// 子プロセスの終了状態。
        status: ExitStatus,
    },
}

impl fmt::Display for WorkerError {
    /// 操作元の失敗理由とtimeoutを表示する。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InterruptSetup(error) => write!(f, "interrupt handler: {error}"),
            Self::Interrupted => write!(f, "conversion interrupted"),
            Self::Io(error) => write!(f, "worker I/O: {error}"),
            Self::Output(error) => error.fmt(f),
            Self::Timeout { timeout } => {
                write!(f, "conversion timed out after {} ms", timeout.as_millis())
            }
            Self::MemoryLimit { bytes } => {
                write!(f, "conversion memory limit exceeded ({bytes} bytes)")
            }
            Self::UnpublishedOutput => {
                write!(
                    f,
                    "supervisor exited successfully before output publication"
                )
            }
            Self::Failed { status } => write!(f, "worker failed: {status}"),
        }
    }
}

impl From<io::Error> for WorkerError {
    /// OS操作の失敗を保持する。
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<jade_pdf2docx_docx::ConvertError> for WorkerError {
    /// 出力の失敗を保持する。
    fn from(error: jade_pdf2docx_docx::ConvertError) -> Self {
        match error {
            jade_pdf2docx_docx::ConvertError::Parse(jade_pdf2docx_docx::ParseError::Cancelled) => {
                Self::Interrupted
            }
            error => Self::Output(error),
        }
    }
}

impl From<ctrlc::Error> for WorkerError {
    /// 停止受付を登録できない場合はworker起動前に失敗する。
    fn from(error: ctrlc::Error) -> Self {
        Self::InterruptSetup(error)
    }
}

/// すべての早期returnで子プロセスを停止・回収する所有権guard。
pub(crate) struct ChildGuard {
    /// 所有するworker。
    pub(crate) child: Child,
    /// 終了状態を回収済みか。
    pub(crate) reaped: bool,
}

impl ChildGuard {
    /// 起動したchildを直ちにguardの所有下へ置く。
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        Ok(Self {
            child: command.spawn()?,
            reaped: false,
        })
    }

    /// 終了済みなら回収し、実行中ならkill後にwaitする。
    fn terminate_and_reap(&mut self) -> io::Result<()> {
        if self.child.try_wait()?.is_none() {
            if let Err(error) = self.child.kill() {
                // killとの競合で既に終了した場合だけ回収済みとして扱う。
                if self.child.try_wait()?.is_none() {
                    return Err(error);
                }
            }
            self.child.wait()?;
        }
        self.reaped = true;
        Ok(())
    }
}

impl Drop for ChildGuard {
    /// 一時領域の破棄に先立って残ったworkerを終了する。
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// 停止要求とdeadlineを先に確認し、正常終了を期限内に観測したworkerだけを成功にする。
/// UnixのSIGINT・SIGTERM・SIGHUP終了は親の受付threadの観測順序に依存せず中断とする。
pub(crate) fn wait_worker(
    child: &mut ChildGuard,
    timeout: Duration,
    mut elapsed: impl FnMut() -> Duration,
    cancellation: &ParseCancellation,
) -> Result<(), WorkerError> {
    loop {
        let elapsed = elapsed();
        if cancellation.is_cancelled() {
            child.terminate_and_reap()?;
            return Err(WorkerError::Interrupted);
        }
        if elapsed >= timeout {
            child.terminate_and_reap()?;
            return Err(WorkerError::Timeout { timeout });
        }
        if let Some(status) = child.child.try_wait()? {
            child.reaped = true;
            if status.success() {
                return Ok(());
            }
            if interrupted_by_signal(&status) {
                return Err(WorkerError::Interrupted);
            }
            return Err(WorkerError::Failed { status });
        }
        std::thread::sleep((timeout - elapsed).min(Duration::from_millis(10)));
    }
}

/// 親のleaseを持つ監督プロセスに変換と一時領域の回収を委ねる。
pub(crate) fn run_isolated(
    executable: &Path,
    request: &Request,
    cancellation: &ParseCancellation,
) -> Result<(), WorkerError> {
    crate::supervisor::run(executable, request, cancellation)
}

/// 監督の一時領域で変換・回収し、同期済み候補の公開を親へ委ねる。
/// 親を使わない呼出では従来どおり正常終了後だけ出力を置換する。
pub(crate) fn run_conversion(
    executable: &Path,
    request: &Request,
    cancellation: &ParseCancellation,
) -> Result<(), WorkerError> {
    check_interruption(cancellation)?;
    let staged = StagedDocxOutput::new(&request.input, &request.output)?;
    let mut command = Command::new(executable);
    command
        .arg(WORKER_FLAG)
        .arg(&request.input)
        .arg(staged.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    request.append_conversion_args(&mut command);
    let started = Instant::now();
    check_interruption(cancellation)?;
    let mut child = ChildGuard::spawn(&mut command)?;
    wait_worker(
        &mut child,
        request.timeout,
        || started.elapsed(),
        cancellation,
    )?;
    let commit_space = crate::commit_space::request(cancellation)?;
    if let Some(directory) = &commit_space {
        staged.prepare_in_directory_with_cancellation(directory.path(), cancellation)?;
        crate::commit_space::prepared(directory.path(), cancellation)?;
    } else {
        staged.commit_with_cancellation(cancellation)?;
    }
    Ok(())
}

/// 保存完了後に監督を止め、終了通知に依存する公開結果の競合を再現する。
#[cfg(test)]
pub(crate) fn publication_checkpoint() {
    if let Some(path) = std::env::var_os("JADE_PDF2DOCX_TEST_PUBLICATION_CHECKPOINT") {
        std::fs::write(&path, b"").unwrap();
        while !Path::new(&path).with_extension("continue").exists() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// worker起動前に停止を観測して、副作用を開始しない。
pub(crate) fn check_interruption(cancellation: &ParseCancellation) -> Result<(), WorkerError> {
    if cancellation.is_cancelled() {
        Err(WorkerError::Interrupted)
    } else {
        Ok(())
    }
}

/// Unixの利用者停止signalを、監督と変換workerで共通に判定する。
pub(crate) fn interrupted_by_signal(status: &ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        matches!(status.signal(), Some(1 | 2 | 15))
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        false
    }
}
