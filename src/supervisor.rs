//! 親とのpipe leaseを持つ監督プロセスに、変換workerと一時領域を所有させる。
use crate::{
    cli::{Request, SUPERVISOR_FLAG},
    process_group,
    worker::{self, WorkerError},
    workspace,
};
use jade_pdf2docx_docx::ParseCancellation;
use std::{
    io::{self, Read, Write},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// 変換開始前に親の接続と内部protocolの版を確認する固定byte列。
pub(crate) const GREETING: &[u8] = b"JADE-PDF2DOCX-LEASE-1\n";

/// 親が持つ監督プロセスとpipe書き込み側の所有権。
struct SupervisorChild {
    /// stdinを親の生存leaseとして使う子プロセス。
    child: Child,
    /// 終了状態を回収済みか。
    reaped: bool,
    /// 親が変換要求の処理を開始した時刻。
    started: Instant,
    /// 起動と保存待ちを含む監督の待機期限。
    timeout: Duration,
    /// 子孫の所属と停止・回収を所有するメモリ制限。
    memory: Option<crate::memory_group::MemoryGroup>,
}

impl Drop for SupervisorChild {
    /// pipeで停止を伝え、期限を超えて応答しない監督は停止して回収する。
    fn drop(&mut self) {
        self.child.stdin.take();
        if !self.reaped {
            if let Some(memory) = &self.memory {
                let _ = memory.kill();
            }
            loop {
                if self.started.elapsed() >= self.timeout {
                    if process_group::terminate_and_reap(&mut self.child).is_err() {
                        let _ = self.child.kill();
                        let _ = self.child.wait();
                    }
                    break;
                }
                match process_group::try_wait(&mut self.child) {
                    Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                    Ok(Some(_)) => break,
                    Err(_) => {
                        // 終了観察が失敗した場合も、実行中の監督をwaitだけで待たない。
                        let _ = self.child.kill();
                        let _ = self.child.wait();
                        break;
                    }
                }
            }
        }
    }
}

/// 起動・変換・保存を親で監督し、期限超過時は子の停止と回収を待つ。
pub(crate) fn run(
    executable: &Path,
    request: &Request,
    cancellation: &ParseCancellation,
) -> Result<(), WorkerError> {
    let started = Instant::now();
    worker::check_interruption(cancellation)?;
    // 対応外OS・未委譲・controller不足は一時領域とworkerを作る前に拒否する。
    let memory = request
        .memory
        .as_ref()
        .map(|options| crate::memory_group::MemoryGroup::new(&options.parent, options.bytes))
        .transpose()?;
    // 子の回収guardより先に作り、子の停止後にだけdirectoryを削除する。
    let reservation = crate::workspace_reservation::Reservation::new(&request.output)?;
    let mut workspace = None;
    // 保存領域も子の停止を待ってから回収するため、子guardより先に宣言する。
    let mut commit_owner = None;
    let mut command = Command::new(executable);
    command
        .env_remove(crate::memory_group::PARENT_GROUP_ENV)
        .arg(SUPERVISOR_FLAG)
        .arg(&request.input)
        .arg(&request.output)
        .arg("--timeout-ms")
        .arg(request.timeout.as_millis().to_string())
        .env(workspace::ENV, reservation.path())
        .env("TMPDIR", reservation.path())
        .env("TMP", reservation.path())
        .env("TEMP", reservation.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    process_group::configure(&mut command);
    if let Some(memory) = &memory {
        memory.configure(&mut command)?;
    }
    request.append_conversion_args(&mut command);
    worker::check_interruption(cancellation)?;
    if started.elapsed() >= request.timeout {
        return Err(WorkerError::Timeout {
            timeout: request.timeout,
        });
    }
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            if let Some(memory) = &memory
                && memory.exceeded()?
            {
                return Err(WorkerError::MemoryLimit {
                    bytes: request.memory.as_ref().unwrap().bytes,
                });
            }
            return Err(error.into());
        }
    };
    let mut supervisor = SupervisorChild {
        child,
        reaped: false,
        started,
        timeout: request.timeout,
        memory,
    };
    let greeting = supervisor
        .child
        .stdin
        .as_mut()
        .ok_or_else(|| io::Error::other("supervisor lease pipe missing"))?
        .write_all(GREETING);
    if let Err(error) = greeting {
        // exec中のOOMでは、初期pipeが閉じた理由を通信エラーに置き換えない。
        if let Some(memory) = &supervisor.memory
            && memory.exceeded()?
        {
            return Err(WorkerError::MemoryLimit {
                bytes: request.memory.as_ref().unwrap().bytes,
            });
        }
        return Err(error.into());
    }
    loop {
        if let Some(memory) = &supervisor.memory
            && memory.exceeded()?
        {
            memory.kill()?;
            process_group::terminate_and_reap(&mut supervisor.child)?;
            supervisor.reaped = true;
            return Err(WorkerError::MemoryLimit {
                bytes: request.memory.as_ref().unwrap().bytes,
            });
        }
        if cancellation.is_cancelled() {
            supervisor.child.stdin.take();
        }
        if let Some(status) = process_group::try_wait(&mut supervisor.child)? {
            supervisor.reaped = true;
            // 終了観察とOOM発生が競合しても、SIGKILLだけの一般エラーにしない。
            if let Some(memory) = &supervisor.memory
                && memory.exceeded()?
            {
                return Err(WorkerError::MemoryLimit {
                    bytes: request.memory.as_ref().unwrap().bytes,
                });
            }
            return Err(unpublished_exit(status, cancellation.is_cancelled(), None));
        }
        let elapsed = started.elapsed();
        if elapsed >= request.timeout {
            supervisor.child.stdin.take();
            let status = process_group::terminate_and_reap(&mut supervisor.child)?;
            supervisor.reaped = true;
            return Err(unpublished_exit(
                status,
                cancellation.is_cancelled(),
                Some(request.timeout),
            ));
        }
        if !cancellation.is_cancelled() {
            if workspace.is_none()
                && let Some(owned) = reservation.claim()?
            {
                commit_owner = Some(crate::commit_space::CommitOwner::new(
                    owned.path(),
                    &request.output,
                ));
                commit_owner.as_mut().unwrap().publication_receipt =
                    request.publication_receipt.clone();
                workspace = Some(owned);
                // 起動許可より先に親の回収guardを保持し、途中のreturnでもchild停止後に回収する。
                reservation.acknowledge()?;
            }
            let Some(commit_owner) = commit_owner.as_mut() else {
                std::thread::sleep((request.timeout - elapsed).min(Duration::from_millis(10)));
                continue;
            };
            commit_owner.poll()?;
            // 親のI/O待ち中に届いた停止・期限を、置換より前にもう一度確認する。
            if !cancellation.is_cancelled()
                && started.elapsed() < request.timeout
                && commit_owner.publish_prepared(|| {
                    !cancellation.is_cancelled()
                        && started.elapsed() < request.timeout
                        && supervisor
                            .memory
                            .as_ref()
                            .is_none_or(|memory| memory.exceeded().is_ok_and(|exceeded| !exceeded))
                })?
            {
                // 実プロセスの回帰用に、最終置換より後の停止要求を注入する。
                #[cfg(test)]
                if std::env::var_os("JADE_PDF2DOCX_TEST_CANCEL_AFTER_PUBLICATION").is_some() {
                    cancellation.cancel();
                }
                // 置換後は成功。監督を終了通知待ちにせず停止し、公開結果を維持する。
                if let Some(memory) = &supervisor.memory {
                    let _ = memory.kill();
                }
                if process_group::terminate_and_reap(&mut supervisor.child).is_ok() {
                    supervisor.reaped = true;
                }
                return Ok(());
            }
        }
        std::thread::sleep((request.timeout - elapsed).min(Duration::from_millis(10)));
    }
}

/// 親の切断監視後に領域を作り、親が回収権を引き受けるまでworkerを開始しない。
pub(crate) fn start_workspace(
    cancellation: &ParseCancellation,
) -> Result<Option<workspace::Workspace>, WorkerError> {
    let check = || {
        if cancellation.is_cancelled() {
            Err(io::Error::from(io::ErrorKind::Interrupted))
        } else {
            Ok(())
        }
    };
    let result = (|| {
        let workspace = workspace::Workspace::receive(check)?;
        if let Some(workspace) = &workspace {
            workspace::Workspace::wait_for_ack(workspace.path(), check)?;
        }
        Ok(workspace)
    })();
    result.map_err(|error| {
        if cancellation.is_cancelled() {
            WorkerError::Interrupted
        } else {
            WorkerError::Io(error)
        }
    })
}

/// 未公開の終了は成功にせず、親の停止要求と観測済み期限を優先する。
pub(crate) fn unpublished_exit(
    status: ExitStatus,
    cancelled: bool,
    expired: Option<Duration>,
) -> WorkerError {
    if cancelled {
        WorkerError::Interrupted
    } else if let Some(timeout) = expired {
        WorkerError::Timeout { timeout }
    } else if status.code() == Some(130) || worker::interrupted_by_signal(&status) {
        WorkerError::Interrupted
    } else if status.success() {
        WorkerError::UnpublishedOutput
    } else {
        WorkerError::Failed { status }
    }
}

/// 親の初期通信を確認し、pipe切断を別threadで協調停止へ伝える。
pub(crate) fn watch_parent(cancellation: &ParseCancellation) -> io::Result<ParentLease> {
    let memory = crate::memory_group::ParentDeath::from_env()?;
    let mut reader = io::stdin();
    validate_lease(&mut reader)?;
    let cancellation = cancellation.clone();
    let lost = ParseCancellation::new();
    let observed = lost.clone();
    std::thread::Builder::new()
        .name("jade-pdf2docx-parent-lease".into())
        .spawn(move || {
            // 親の消滅を先に記録し、変換threadの終了との競合で回収を省略しない。
            observe_parent(reader, &observed);
            cancellation.cancel();
        })?;
    Ok(ParentLease { lost, memory })
}

/// 親が消滅した場合に監督自身の終了で専用群の子孫も停止する所有権。
/// 一時領域のguardより先に作り、その回収後まで保持すること。
#[must_use = "親leaseのguardは一時領域の回収後まで保持してください"]
pub(crate) struct ParentLease {
    /// EOF・protocol違反・監視不能を停止要求とは別に記録する。
    lost: ParseCancellation,
    /// 自分が所属すると検証した専用cgroupの停止権。
    memory: Option<crate::memory_group::ParentDeath>,
}

impl Drop for ParentLease {
    /// 通常のguardが片付いた後、自分を含む専用群の残存プロセスを停止する。
    fn drop(&mut self) {
        if self.lost.is_cancelled() {
            if let Some(memory) = &self.memory
                && let Err(error) = memory.terminate()
            {
                eprintln!("parent death memory group termination failed: {error}");
            }
            crate::process_group::terminate_own_group();
        }
    }
}

/// 不完全・異なる初期通信ではworkerと一時領域を作らない。
pub(crate) fn validate_lease(reader: &mut impl Read) -> io::Result<()> {
    let mut bytes = [0; GREETING.len()];
    reader.read_exact(&mut bytes)?;
    if bytes != GREETING {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid supervisor lease greeting",
        ));
    }
    Ok(())
}

/// EOF・追加データ・監視不能で停止し、読み込み割り込みだけ再試行する。
pub(crate) fn observe_parent(mut reader: impl Read, cancellation: &ParseCancellation) {
    let mut byte = [0];
    loop {
        match reader.read(&mut byte) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            _ => {
                cancellation.cancel();
                return;
            }
        }
    }
}
