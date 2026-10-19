//! 保存要求の遅延所有と通知の入力境界を検証する。
use super::*;

/// 通知済みの領域は親切断の停止より先に引き受け、親のDrop欠落時も回収する。
#[test]
fn published_grant_is_received_even_after_parent_cancellation() {
    let base = tempfile::tempdir().unwrap();
    let directory =
        Workspace::new_registered_commit_in(base.path(), base.path(), |_| Ok(())).unwrap();
    fs::write(
        base.path().join(GRANT),
        directory.path().file_name().unwrap().as_encoded_bytes(),
    )
    .unwrap();
    let cancellation = ParseCancellation::new();
    cancellation.cancel();
    let recovery = wait_for_grant(base.path(), &cancellation).unwrap();
    drop(recovery);
    assert!(!directory.path().exists());
}

/// worker成功前は出力親を作らず、通知前に親の回収所有権を確立する。
#[test]
fn commit_owner_creates_only_after_request_and_retains_cleanup() {
    let base = tempfile::tempdir().unwrap();
    let output = base.path().join("new/output.docx");
    let mut owner = CommitOwner::new(base.path(), &output);
    owner.poll().unwrap();
    assert!(!output.parent().unwrap().exists());
    fs::write(base.path().join(REQUEST), b"").unwrap();
    owner.poll().unwrap();
    let grant = fs::read(base.path().join(GRANT)).unwrap();
    let directory = base.path().join(validate_basename(&grant).unwrap());
    fs::write(directory.join("partial"), b"partial").unwrap();
    owner.poll().unwrap();
    assert_eq!(fs::read_dir(output.parent().unwrap()).unwrap().count(), 0);
    drop(owner);
    assert!(!directory.exists());
}

/// 通知のbasenameとmarkerを確認後、親が失われても補助所有者が領域を回収する。
#[test]
fn supervisor_can_recover_commit_directory_without_parent_drop() {
    let base = tempfile::tempdir().unwrap();
    let parent = Workspace::new_commit_in(base.path()).unwrap();
    let directory = parent.path().to_owned();
    let recovery = Workspace::recover_commit(&directory).unwrap();
    fs::write(directory.join("partial"), b"partial").unwrap();
    drop(recovery);
    assert!(!directory.exists());
    drop(parent);
}

/// 相対脱出・絶対パス・NUL・非ASCII・過大通知を拒否し、bare outputの基点を固定する。
#[test]
fn grant_rejects_paths_and_unbounded_names() {
    for invalid in [
        b"".as_slice(),
        b".jade-pdf2docx-commit-",
        b"../.jade-pdf2docx-commit-owned",
        b"/tmp/.jade-pdf2docx-commit-owned",
        b".jade-pdf2docx-commit-../foreign",
        b".jade-pdf2docx-commit-\\foreign",
        b".jade-pdf2docx-commit-x\0",
        b".jade-pdf2docx-commit-\xff",
        &[b'a'; 129],
    ] {
        assert_eq!(
            validate_basename(invalid).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    assert_eq!(
        validate_basename(b".jade-pdf2docx-commit-aB0123").unwrap(),
        ".jade-pdf2docx-commit-aB0123"
    );
    assert_eq!(output_parent(Path::new("output.docx")), Path::new("."));
}

/// 通知未完了の作成予定を読み、親のDropなしでも所有markerがある領域を回収する。
#[test]
fn cancelled_pending_creation_removes_owned_directory() {
    let workspace = tempfile::tempdir().unwrap();
    let directory = Workspace::new_commit_in(workspace.path()).unwrap();
    let mut marker = b"JADE-PDF2DOCX-COMMIT-1\n".to_vec();
    marker.extend_from_slice(workspace.path().as_os_str().as_encoded_bytes());
    fs::write(directory.path().join(workspace::MARKER), marker).unwrap();
    fs::write(
        workspace.path().join("commit-intent"),
        directory.path().file_name().unwrap().as_encoded_bytes(),
    )
    .unwrap();
    let cancellation = ParseCancellation::new();
    cancellation.cancel();
    assert!(matches!(
        wait_for_grant(workspace.path(), &cancellation),
        Err(WorkerError::Interrupted)
    ));
    assert!(
        !directory.path().exists(),
        "pending commit directory survived parent death"
    );
}

/// 作成予定の名前だけでは、他のjob・未作成・通知不正のdirectoryを削除しない。
#[test]
fn cancelled_pending_creation_preserves_foreign_directory() {
    for kind in ["foreign", "other-job", "absent", "invalid"] {
        let workspace = tempfile::tempdir().unwrap();
        let directory = Workspace::new_commit_in(workspace.path()).unwrap();
        fs::write(directory.path().join("keep"), b"foreign").unwrap();
        if kind == "other-job" {
            fs::write(
                directory.path().join(workspace::MARKER),
                b"JADE-PDF2DOCX-COMMIT-1\nother-job",
            )
            .unwrap();
        }
        let name = match kind {
            "absent" => b".jade-pdf2docx-commit-absent".as_slice(),
            "invalid" => b"../escape".as_slice(),
            _ => directory.path().file_name().unwrap().as_encoded_bytes(),
        };
        fs::write(workspace.path().join("commit-intent"), name).unwrap();
        let cancellation = ParseCancellation::new();
        cancellation.cancel();
        assert!(wait_for_grant(workspace.path(), &cancellation).is_err());
        assert_eq!(fs::read(directory.path().join("keep")).unwrap(), b"foreign");
    }
}

/// 作成予定でもdirectoryやmarkerのリンク先、未識別領域を回収しない。
#[cfg(unix)]
#[test]
fn pending_creation_rejects_unmarked_directory_and_symlinks() {
    use std::os::unix::fs::symlink;
    for kind in ["directory-link", "marker-link", "unmarked"] {
        let workspace = tempfile::tempdir().unwrap();
        let foreign = tempfile::tempdir().unwrap();
        fs::write(foreign.path().join("keep"), b"foreign").unwrap();
        let directory = workspace.path().join(".jade-pdf2docx-commit-pending");
        if kind == "directory-link" {
            symlink(foreign.path(), &directory).unwrap();
        } else {
            fs::create_dir(&directory).unwrap();
            if kind == "marker-link" {
                let mut marker = b"JADE-PDF2DOCX-COMMIT-1\n".to_vec();
                marker.extend_from_slice(workspace.path().as_os_str().as_encoded_bytes());
                fs::write(foreign.path().join("marker"), marker).unwrap();
                symlink(
                    foreign.path().join("marker"),
                    directory.join(workspace::MARKER),
                )
                .unwrap();
            }
        }
        fs::write(
            workspace.path().join(INTENT),
            b".jade-pdf2docx-commit-pending",
        )
        .unwrap();
        assert!(recover_pending(workspace.path()).is_err());
        assert!(directory.exists());
        assert_eq!(fs::read(foreign.path().join("keep")).unwrap(), b"foreign");
    }
}

/// コピー完了通知だけでは公開せず、親の最後の停止確認を通った候補だけ置換する。
#[test]
fn parent_publication_obeys_final_check_and_preserves_completed_result() {
    let base = tempfile::tempdir().unwrap();
    let output = base.path().join("output.docx");
    fs::write(&output, b"existing").unwrap();
    let mut owner = CommitOwner::new(base.path(), &output);
    assert!(!owner.publish_prepared(|| true).unwrap());
    fs::write(base.path().join(REQUEST), b"").unwrap();
    owner.poll().unwrap();
    let directory = owner.directory.as_ref().unwrap().path().to_owned();
    fs::write(directory.join(CANDIDATE), b"completed").unwrap();
    assert!(!owner.publish_prepared(|| true).unwrap());
    fs::write(directory.join(PREPARED), PREPARED_CONTENT).unwrap();
    assert!(!owner.publish_prepared(|| false).unwrap());
    assert_eq!(fs::read(&output).unwrap(), b"existing");
    assert_eq!(fs::read(directory.join(CANDIDATE)).unwrap(), b"completed");
    assert!(owner.publish_prepared(|| true).unwrap());
    drop(owner);
    assert!(!directory.exists());
    assert_eq!(fs::read(&output).unwrap(), b"completed");
}

/// 公開に失敗した場合は既存出力を保持し、候補は親のguardで回収する。
#[test]
fn parent_publication_error_retains_destination_and_cleans_candidate() {
    let base = tempfile::tempdir().unwrap();
    let output = base.path().join("output.docx");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("existing"), b"existing").unwrap();
    let mut owner = CommitOwner::new(base.path(), &output);
    fs::write(base.path().join(REQUEST), b"").unwrap();
    owner.poll().unwrap();
    let directory = owner.directory.as_ref().unwrap().path().to_owned();
    fs::write(directory.join(CANDIDATE), b"completed").unwrap();
    fs::write(directory.join(PREPARED), PREPARED_CONTENT).unwrap();
    assert!(owner.publish_prepared(|| true).is_err());
    assert_eq!(fs::read(output.join("existing")).unwrap(), b"existing");
    drop(owner);
    assert!(!directory.exists());
}

/// 不完全・過大な準備通知は候補や既存出力を公開しない。
#[test]
fn malformed_prepared_notices_do_not_publish() {
    let base = tempfile::tempdir().unwrap();
    let output = base.path().join("output.docx");
    fs::write(&output, b"existing").unwrap();
    let mut owner = CommitOwner::new(base.path(), &output);
    fs::write(base.path().join(REQUEST), b"").unwrap();
    owner.poll().unwrap();
    let directory = owner.directory.as_ref().unwrap().path();
    fs::write(directory.join(CANDIDATE), b"completed").unwrap();
    for content in [
        b"".as_slice(),
        b"JADE-PDF2DOCX-PREPARED-1",
        b"JADE-PDF2DOCX-PREPARED-1\nextra",
        &[b'x'; 8192],
    ] {
        fs::write(directory.join(PREPARED), content).unwrap();
        assert_eq!(
            owner.publish_prepared(|| true).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read(&output).unwrap(), b"existing");
    }
}

/// 通知と候補のリンク・directory・FIFOを公開対象にしない。
#[cfg(unix)]
#[test]
fn prepared_publication_rejects_nonregular_entries() {
    use std::os::unix::fs::symlink;
    let base = tempfile::tempdir().unwrap();
    let output = base.path().join("output.docx");
    fs::write(&output, b"existing").unwrap();
    let mut owner = CommitOwner::new(base.path(), &output);
    fs::write(base.path().join(REQUEST), b"").unwrap();
    owner.poll().unwrap();
    let directory = owner.directory.as_ref().unwrap().path();
    let notice = directory.join(PREPARED);
    let candidate = directory.join(CANDIDATE);
    let foreign = base.path().join("foreign");
    fs::write(&foreign, PREPARED_CONTENT).unwrap();
    symlink(&foreign, &notice).unwrap();
    assert_eq!(
        owner.publish_prepared(|| true).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    fs::remove_file(&notice).unwrap();
    fs::write(&notice, PREPARED_CONTENT).unwrap();
    symlink(&foreign, &candidate).unwrap();
    assert_eq!(
        owner.publish_prepared(|| true).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    fs::remove_file(&candidate).unwrap();
    fs::create_dir(&candidate).unwrap();
    assert_eq!(
        owner.publish_prepared(|| true).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    fs::remove_dir(&candidate).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&candidate)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        owner.publish_prepared(|| true).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(fs::read(&output).unwrap(), b"existing");
    assert_eq!(fs::read(&foreign).unwrap(), PREPARED_CONTENT);
}

/// 準備通知後に親leaseが切れた監督は候補を回収し、最終出力を公開しない。
#[test]
fn parent_disconnect_after_preparation_cleans_unpublished_candidate() {
    let base = tempfile::tempdir().unwrap();
    let output = base.path().join("output.docx");
    fs::write(&output, b"existing").unwrap();
    let workspace =
        Workspace::new_registered_commit_in(base.path(), base.path(), |_| Ok(())).unwrap();
    let directory = workspace.path().to_owned();
    fs::write(directory.join(CANDIDATE), b"completed").unwrap();
    let cancellation = ParseCancellation::new();
    let observed = cancellation.clone();
    let runner = std::thread::spawn(move || {
        let result = prepared(workspace.path(), &observed);
        drop(workspace);
        result
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !directory.join(PREPARED).exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let announced = directory.join(PREPARED).exists();
    cancellation.cancel();
    assert!(matches!(
        runner.join().unwrap(),
        Err(WorkerError::Interrupted)
    ));
    assert!(
        announced,
        "supervisor never announced its prepared candidate"
    );
    assert!(!directory.exists());
    assert_eq!(fs::read(output).unwrap(), b"existing");
}
