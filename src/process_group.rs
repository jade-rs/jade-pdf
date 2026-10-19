//! 監督が終了しても残る子孫を、監督PIDの再利用前に専用process groupで停止する。
use std::{
    io,
    process::{Child, Command, ExitStatus},
};

/// 親を失った監督が、生存中の自分をleaderとする専用群だけを停止する。
/// SIGKILLはこの呼出元にも届くため、一時領域の回収後にだけ呼び出すこと。
pub(crate) fn terminate_own_group() {
    #[cfg(unix)]
    {
        // SAFETY: getpid/getpgrpは引数・参照を持たない現在プロセスの照会。
        let pid = unsafe { libc::getpid() };
        // SAFETY: 同上。専用群でない内部起動では呼出元の群を停止しない。
        if unsafe { libc::getpgrp() } != pid {
            return;
        }
        // SAFETY: leaderはこの実行中の監督自身で、PIDの再利用は起こらない。
        // 親が終了済みでも、同じ資格情報の描画子孫を自分とともに停止できる。
        unsafe { libc::kill(-pid, libc::SIGKILL) };
    }
}

/// 監督のI/O相手を含む専用群を先に停止し、未回収PIDの終了状態を回収する。
pub(crate) fn terminate_and_reap(child: &mut Child) -> io::Result<ExitStatus> {
    if let Some(status) = try_wait(child)? {
        return Ok(status);
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd"
    ))]
    {
        let pid = i32::try_from(child.id())
            .ok()
            .filter(|pid| *pid > 0)
            .ok_or_else(|| io::Error::other("invalid supervisor pid"))?;
        // 監督だけをkillして終了を待つと、子孫が保持するFIFO等のI/Oに依存する。
        // 所有するChildはまだ回収していないため、専用群のPIDは再利用されない。
        kill_group(pid)?;
    }
    if let Err(error) = child.kill() {
        // 停止との競合で自然終了した場合も、群を掃除するtry_waitで回収する。
        return try_wait(child)?.ok_or(error);
    }
    loop {
        if let Some(status) = try_wait(child)? {
            return Ok(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 終了を回収せず観察できるUnixで、監督と子孫を親とは別の群にする。
pub(crate) fn configure(command: &mut Command) {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd"
    ))]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd"
    )))]
    let _ = command;
}

/// 終了観察・残った群の停止・監督の回収をこの順に行い、PID再利用を防ぐ。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
pub(crate) fn try_wait(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    let pid = i32::try_from(child.id())
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| io::Error::other("invalid supervisor pid"))?;
    loop {
        // SAFETY: siginfo_tの全zeroは有効で、waitidへ渡す書き込み領域は呼出し中に生存する。
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: 所有する未回収Childだけを観察する。WNOWAITによりPIDは再利用されない。
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if info.si_signo == 0 {
            return Ok(None);
        }
        kill_group(pid)?;
        return child.wait().map(Some);
    }
}

/// 所有する未回収leaderの専用群へ停止を送り、既に空の群は成功とする。
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
))]
fn kill_group(pid: i32) -> io::Result<()> {
    // SAFETY: 呼出元は未回収Childを所有し、leader PIDの再利用を防いでいる。
    if unsafe { libc::kill(-pid, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // Appleのkillpgはzombieを対象から除外し、対象ゼロをEPERMにする。
    // 監督・workerは同じ資格情報を維持する。資格情報を変更する子孫は保証外。
    let empty_apple_group = cfg!(any(target_os = "macos", target_os = "ios"))
        && error.raw_os_error() == Some(libc::EPERM);
    if error.raw_os_error() == Some(libc::ESRCH) || empty_apple_group {
        Ok(())
    } else {
        Err(error)
    }
}

/// 未対応platformでは通常の子回収を使い、子孫群の回収は保証しない。
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd"
)))]
pub(crate) fn try_wait(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    child.try_wait()
}
