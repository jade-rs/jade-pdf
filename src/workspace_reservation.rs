//! 作成前の外側領域を選び、監督の作成完了後だけ親の回収権を取得する。
use crate::workspace::{self, Workspace};
use std::{
    io,
    path::{Path, PathBuf},
};

/// 排他的なmkdirを監督へ依頼する候補。選択だけではdirectoryを作らない。
pub(crate) struct Reservation {
    /// outputと同じfilesystemの既存祖先の下に選ぶ絶対候補パス。
    path: PathBuf,
}

impl Reservation {
    /// ランダムな候補を選び、既存fileの削除を一切所有しない。
    pub(crate) fn new(output: &Path) -> io::Result<Self> {
        let temporary = tempfile::Builder::new()
            .prefix(workspace::PREFIX)
            .rand_bytes(16)
            .disable_cleanup(true)
            .make_in(workspace::existing_output_ancestor(output)?, |path| {
                Ok(path.to_owned())
            })?;
        let (path, _) = temporary.into_parts();
        Ok(Self { path })
    }

    /// spawn前に環境変数で監督へ渡す候補を返す。
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// 完成したjob固有markerを待ち、未作成なら所有権を取得しない。
    pub(crate) fn claim(&self) -> io::Result<Option<Workspace>> {
        match Workspace::recover_registered(&self.path) {
            Ok(workspace) => Ok(Some(workspace)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// 回収guard取得済みの親が、監督へworker開始を許可する。
    pub(crate) fn acknowledge(&self) -> io::Result<()> {
        Workspace::acknowledge(&self.path)
    }
}

impl Drop for Reservation {
    /// child停止後に完成済みmarkerだけを回収し、衝突したforeign領域は保持する。
    fn drop(&mut self) {
        if let Ok(mut workspace) = Workspace::recover_registered(&self.path) {
            // SIGKILL送信とkernelのFD解放には時間差がある。停止後の最後の回収だけ待つ。
            workspace.cleanup_with_grace(std::time::Duration::from_secs(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Reservation;
    use crate::workspace::{MARKER, Workspace};
    use std::{fs, io};

    /// 候補の選択ではdirectoryも未作成の出力親も変更しない。
    #[test]
    fn reservation_creates_nothing_until_supervisor_owns_directory() {
        let base = tempfile::tempdir().unwrap();
        let reservation = Reservation::new(&base.path().join("missing/output.docx")).unwrap();
        assert!(!reservation.path().exists());
        assert_eq!(
            reservation.path().parent().unwrap(),
            fs::canonicalize(base.path()).unwrap()
        );
        assert!(reservation.claim().unwrap().is_none());
        assert_eq!(fs::read_dir(base.path()).unwrap().count(), 0);
        let helper = Workspace::create_registered(reservation.path(), || Ok(())).unwrap();
        let parent = reservation.claim().unwrap().unwrap();
        reservation.acknowledge().unwrap();
        Workspace::wait_for_ack(helper.path(), || Ok(())).unwrap();
        drop(parent);
        assert!(helper.path().exists());
        let path = helper.path().to_owned();
        drop(helper);
        assert!(!path.exists());
    }

    /// 所有前の停止とmkdir後の停止で、領域を残さず出力親も変更しない。
    #[test]
    fn cancelled_creation_removes_unmarked_directory() {
        let base = tempfile::tempdir().unwrap();
        for stop_at in [1, 2] {
            let reservation = Reservation::new(&base.path().join("missing/output.docx")).unwrap();
            let mut checks = 0;
            let result = Workspace::create_registered(reservation.path(), || {
                checks += 1;
                if checks == stop_at {
                    Err(io::Error::from(io::ErrorKind::Interrupted))
                } else {
                    Ok(())
                }
            });
            assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::Interrupted));
            assert!(!reservation.path().exists());
        }
        assert_eq!(fs::read_dir(base.path()).unwrap().count(), 0);
    }

    /// 衝突したforeign directoryと、別候補から移動されたmarkerを回収しない。
    #[test]
    fn collision_and_renamed_marker_preserve_foreign_directory() {
        let base = tempfile::tempdir().unwrap();
        let first = Reservation::new(&base.path().join("out.docx")).unwrap();
        fs::create_dir(first.path()).unwrap();
        fs::write(first.path().join("keep"), b"foreign").unwrap();
        assert!(Workspace::create_registered(first.path(), || Ok(())).is_err());
        let first_path = first.path().to_owned();
        drop(first);
        assert_eq!(fs::read(first_path.join("keep")).unwrap(), b"foreign");
        let source = Reservation::new(&base.path().join("out.docx")).unwrap();
        let helper = Workspace::create_registered(source.path(), || Ok(())).unwrap();
        let target = Reservation::new(&base.path().join("out.docx")).unwrap();
        fs::rename(helper.path(), target.path()).unwrap();
        assert!(target.claim().is_err());
        let target_path = target.path().to_owned();
        drop(target);
        assert!(target_path.join(MARKER).is_file());
    }

    /// 親が取得前に終了しても、完了したjob固有markerを持つ領域だけは回収する。
    #[test]
    fn reservation_drop_recovers_completed_workspace() {
        let base = tempfile::tempdir().unwrap();
        let reservation = Reservation::new(&base.path().join("out.docx")).unwrap();
        let helper = Workspace::create_registered(reservation.path(), || Ok(())).unwrap();
        drop(reservation);
        assert!(helper.path().exists());
        let path = helper.path().to_owned();
        drop(helper);
        assert!(!path.exists());
    }

    /// SIGKILL送信後も残るworkerのleaseが解放されるまで、最後の回収を短時間待つ。
    #[test]
    fn reservation_drop_waits_for_stopping_worker_lease() {
        let base = tempfile::tempdir().unwrap();
        let reservation = Reservation::new(&base.path().join("out.docx")).unwrap();
        let path = reservation.path().to_owned();
        let helper = Workspace::create_registered(&path, || Ok(())).unwrap();
        let worker = crate::workspace::worker_lease(Some(&path))
            .unwrap()
            .unwrap();
        drop(helper);
        let stopped = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(worker);
        });
        drop(reservation);
        assert!(!path.exists(), "stopping worker left a completed workspace");
        stopped.join().unwrap();
    }

    /// 未完成・過大・directory通知では起動せず、通知前の切断も待機を止める。
    #[test]
    fn acknowledgement_rejects_invalid_content_and_honors_cancellation() {
        let base = tempfile::tempdir().unwrap();
        let reservation = Reservation::new(&base.path().join("out.docx")).unwrap();
        let helper = Workspace::create_registered(reservation.path(), || Ok(())).unwrap();
        let ack = helper.path().join(crate::workspace::ACK);
        for invalid in [
            b"".as_slice(),
            b"JADE-PDF2DOCX-WORKSPACE-ACK-1\nextra",
            b"other",
        ] {
            fs::write(&ack, invalid).unwrap();
            assert!(
                matches!(Workspace::wait_for_ack(helper.path(), || Ok(())), Err(error) if error.kind() == io::ErrorKind::InvalidData)
            );
        }
        fs::remove_file(&ack).unwrap();
        fs::create_dir(&ack).unwrap();
        assert!(Workspace::wait_for_ack(helper.path(), || Ok(())).is_err());
        fs::remove_dir(&ack).unwrap();
        assert!(
            matches!(Workspace::wait_for_ack(helper.path(), || Err(io::ErrorKind::Interrupted.into())), Err(error) if error.kind() == io::ErrorKind::Interrupted)
        );
    }

    /// 起動許可のリンクやFIFOは通常fileとして開かず拒否する。
    #[cfg(unix)]
    #[test]
    fn acknowledgement_rejects_symlink_and_fifo_without_opening() {
        let base = tempfile::tempdir().unwrap();
        let reservation = Reservation::new(&base.path().join("out.docx")).unwrap();
        let helper = Workspace::create_registered(reservation.path(), || Ok(())).unwrap();
        reservation.acknowledge().unwrap();
        let ack = helper.path().join(crate::workspace::ACK);
        let target = helper.path().join("foreign-ack");
        fs::rename(&ack, &target).unwrap();
        std::os::unix::fs::symlink(&target, &ack).unwrap();
        assert!(Workspace::wait_for_ack(helper.path(), || Ok(())).is_err());
        assert!(target.exists());
        fs::remove_file(&ack).unwrap();
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&ack)
                .status()
                .unwrap()
                .success()
        );
        assert!(Workspace::wait_for_ack(helper.path(), || Ok(())).is_err());
    }
}
