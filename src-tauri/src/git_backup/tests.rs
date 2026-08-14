use super::*;
use crate::app_core::{atomic_write, now_ms};
use crate::operation_plan::StableHash;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

fn test_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("skillmate-backup-test-{}-{}", name, now_ms()))
}

fn managed_connection() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE managed_installations (
                skill_path TEXT PRIMARY KEY,
                assistant TEXT NOT NULL,
                source TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                target_name TEXT NOT NULL,
                scope TEXT NOT NULL DEFAULT 'global',
                install_mode TEXT NOT NULL DEFAULT 'copy',
                project_path TEXT,
                tracking_ref TEXT,
                subdir TEXT,
                resolved_ref TEXT,
                content_hash TEXT,
                installed_at TEXT NOT NULL
             );
             CREATE TABLE managed_roots (
                root_path TEXT PRIMARY KEY,
                scope TEXT NOT NULL DEFAULT 'global',
                project_path TEXT,
                updated_at TEXT NOT NULL
             );",
        )
        .unwrap();
    connection
}

fn register_backup_source(connection: &Connection, source: &Path) {
    connection
        .execute(
            "INSERT INTO managed_installations (
                skill_path, assistant, source, source_kind, target_name, scope,
                install_mode, installed_at
             ) VALUES (?, 'Codex', 'local', 'local', 'managed', 'global', 'copy', 'now')",
            [source.to_string_lossy().to_string()],
        )
        .unwrap();
}

fn initialize_test_git_repo(repo: &Path) {
    fs::create_dir_all(repo).unwrap();
    ensure_git_repo(repo).unwrap();
    ensure_git_identity(repo).unwrap();
}

fn commit_old_backup_snapshot(repo: &Path) {
    fs::create_dir_all(repo.join("assistants")).unwrap();
    fs::write(repo.join("assistants").join(BACKUP_ROOT_MARKER), "managed").unwrap();
    fs::write(repo.join("assistants/old.txt"), "old snapshot").unwrap();
    fs::write(repo.join("skillmate-backup.json"), "old manifest").unwrap();
    stage_backup_snapshot(repo).unwrap();
    run_git_checked(
        repo,
        &["commit", "-m", "old backup"],
        Duration::from_secs(30),
    )
    .unwrap();
}

#[test]
fn snapshot_root_accepts_missing_directory_without_mutation() {
    let base = test_dir("first-run");
    let repo = base.join("repo");

    validate_existing_snapshot_root(&repo).unwrap();

    assert!(!repo.join("assistants").exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn load_normalizes_null_legacy_backup_values() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE git_backup (
                id INTEGER PRIMARY KEY,
                enabled INTEGER,
                remote_url TEXT,
                repo_path TEXT,
                branch TEXT,
                last_sync TEXT
             );
             INSERT INTO git_backup VALUES (1, NULL, NULL, NULL, NULL, NULL);",
        )
        .unwrap();

    let backup = load(&connection).unwrap();

    assert!(!backup.enabled);
    assert_eq!(backup.remote_url, "");
    assert_eq!(backup.repo_path, "");
    assert_eq!(backup.branch, "main");
    assert_eq!(backup.last_sync, "");
}

#[test]
fn snapshot_root_accepts_managed_directory_without_deleting_it() {
    let base = test_dir("managed-replace");
    let repo = base.join("repo");
    let snapshot_root = repo.join("assistants");
    fs::create_dir_all(&snapshot_root).unwrap();
    fs::write(snapshot_root.join(BACKUP_ROOT_MARKER), "managed").unwrap();
    fs::write(snapshot_root.join("old-file"), "old").unwrap();

    validate_existing_snapshot_root(&repo).unwrap();

    assert!(snapshot_root.join(BACKUP_ROOT_MARKER).exists());
    assert!(snapshot_root.join("old-file").exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn snapshot_root_rejects_unmanaged_existing_directory() {
    let base = test_dir("unmanaged-reject");
    let repo = base.join("repo");
    let snapshot_root = repo.join("assistants");
    fs::create_dir_all(&snapshot_root).unwrap();
    fs::write(snapshot_root.join("user-file"), "keep").unwrap();

    let error = validate_existing_snapshot_root(&repo).unwrap_err();

    assert_eq!(
        error,
        "备份仓库中的 assistants 不是 SkillMate 管理目录，已拒绝覆盖"
    );
    assert!(snapshot_root.join("user-file").exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn snapshot_transaction_rolls_back_directory_and_manifest() {
    let base = test_dir("transaction-rollback");
    let repo = base.join("repo");
    let transaction_root = repo.join(".git/skillmate-backup-test");
    let snapshot_root = repo.join("assistants");
    let backup_root = transaction_root.join("previous-assistants");
    let manifest_path = repo.join("skillmate-backup.json");
    initialize_test_git_repo(&repo);
    fs::create_dir_all(&snapshot_root).unwrap();
    fs::create_dir_all(&backup_root).unwrap();
    fs::write(
        snapshot_root.join(BACKUP_ROOT_MARKER),
        transaction_snapshot_marker("test"),
    )
    .unwrap();
    fs::write(backup_root.join(BACKUP_ROOT_MARKER), "managed").unwrap();
    fs::write(snapshot_root.join("new"), "new").unwrap();
    fs::write(backup_root.join("old"), "old").unwrap();
    fs::write(&manifest_path, "new manifest").unwrap();
    atomic_write(
        &transaction_root.join(BACKUP_PREVIOUS_MANIFEST_FILE),
        b"old manifest",
    )
    .unwrap();
    let mut previous_manifest_hash = StableHash::new();
    previous_manifest_hash.update(b"old manifest");
    write_backup_transaction_owner(&transaction_root, "test").unwrap();
    let mut journal = BackupSnapshotJournal {
        version: BACKUP_JOURNAL_VERSION,
        generation: 0,
        state: BackupSnapshotState::Prepared,
        transaction_id: "test".to_string(),
        baseline_branch: Some(current_git_branch(&repo).unwrap()),
        baseline_head: None,
        expected_tree: None,
        expected_commit: None,
        previous_snapshot_marker: Some("managed".to_string()),
        previous_manifest_len: Some(b"old manifest".len() as u64),
        previous_manifest_sha256: Some(previous_manifest_hash.finish()),
        had_snapshot: true,
        had_manifest: true,
    };
    update_backup_snapshot_journal(&transaction_root, &mut journal, |_| {}).unwrap();
    let mut transaction = BackupSnapshotTransaction {
        repo: repo.clone(),
        transaction_root,
        journal,
        finished: false,
    };

    transaction.rollback().unwrap();

    assert!(snapshot_root.join("old").exists());
    assert!(!snapshot_root.join("new").exists());
    assert_eq!(fs::read_to_string(manifest_path).unwrap(), "old manifest");
    fs::remove_dir_all(base).ok();
}

#[test]
fn prepared_snapshot_transaction_recovers_files_manifest_and_index() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("recover-prepared");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "new skill").unwrap();
    register_backup_source(&connection, &source);

    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    let transaction_root = transaction.transaction_root.clone();
    transaction.stage_for_commit().unwrap();
    fs::write(repo.join("private.env"), "TOKEN=private").unwrap();
    run_git_checked(
        &repo,
        &["add", "--", "private.env"],
        Duration::from_secs(10),
    )
    .unwrap();
    let orphan = repo.join(".git/skillmate-backup-orphan");
    fs::create_dir_all(orphan.join("assistants")).unwrap();
    write_backup_transaction_owner(&orphan, "orphan").unwrap();
    fs::write(orphan.join("assistants/partial"), "partial").unwrap();
    std::mem::forget(transaction);

    recover_backup_transactions(&repo).unwrap();

    assert!(repo.join("assistants/old.txt").exists());
    assert_eq!(
        fs::read_to_string(repo.join("skillmate-backup.json")).unwrap(),
        "old manifest"
    );
    assert!(!transaction_root.exists());
    assert!(!orphan.exists());
    assert_eq!(
        git_output(&repo, &["diff", "--cached", "--name-only"]).unwrap(),
        "private.env"
    );
    fs::remove_dir_all(base).ok();
}

#[test]
fn changed_previous_manifest_is_rejected_before_rollback() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("manifest-binding");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "new skill").unwrap();
    register_backup_source(&connection, &source);
    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    let transaction_root = transaction.transaction_root.clone();
    fs::write(
        transaction_root.join(BACKUP_PREVIOUS_MANIFEST_FILE),
        "bad manifest",
    )
    .unwrap();

    let error = transaction.rollback().unwrap_err();

    assert!(error.contains("摘要与事务日志不匹配"));
    assert!(transaction_root.exists());
    assert!(repo.join("assistants/roots").exists());
    assert!(transaction_root.join("previous-assistants").exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn committed_snapshot_transaction_only_cleans_journal() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("recover-committed");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "new skill").unwrap();
    register_backup_source(&connection, &source);

    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    let transaction_root = transaction.transaction_root.clone();
    let copied_skill = repo
        .join("assistants/roots")
        .join(backup_root_id(&source))
        .join("SKILL.md");
    transaction.stage_for_commit().unwrap();
    let expected_tree = transaction.journal.expected_tree.clone().unwrap();
    let commit_transaction_root = transaction.transaction_root.clone();
    commit_backup_snapshot(
        &repo,
        &commit_transaction_root,
        "new backup",
        &mut transaction.journal,
    )
    .unwrap();
    fs::write(repo.join("private.env"), "TOKEN=private").unwrap();
    run_git_checked(
        &repo,
        &["add", "--", "private.env"],
        Duration::from_secs(10),
    )
    .unwrap();
    transaction.mark_committed().unwrap();
    std::mem::forget(transaction);

    recover_backup_transactions(&repo).unwrap();

    assert!(copied_skill.exists());
    assert!(!repo.join("assistants/old.txt").exists());
    assert!(!transaction_root.exists());
    assert_eq!(git_tree(&repo, "HEAD").unwrap(), expected_tree);
    assert_eq!(
        git_output(&repo, &["diff", "--cached", "--name-only"]).unwrap(),
        "private.env"
    );
    fs::remove_dir_all(base).ok();
}

#[test]
fn committed_journal_write_failure_preserves_transaction_directory() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("committed-journal-failure");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "new skill").unwrap();
    register_backup_source(&connection, &source);
    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    transaction.stage_for_commit().unwrap();
    let transaction_root = transaction.transaction_root.clone();
    commit_backup_snapshot(
        &repo,
        &transaction_root,
        "new backup",
        &mut transaction.journal,
    )
    .unwrap();
    fs::remove_file(transaction_root.join(BACKUP_JOURNAL_FILE)).unwrap();
    fs::create_dir(transaction_root.join(BACKUP_JOURNAL_FILE)).unwrap();

    let error = transaction.finish_commit().unwrap_err();

    assert!(error.contains("记录备份提交状态失败"));
    assert!(transaction_root.exists());
    std::mem::forget(transaction);
    fs::remove_dir_all(base).ok();
}

#[test]
fn prepared_marker_window_detects_commit_with_or_without_baseline_head() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    for has_baseline_head in [false, true] {
        let base = test_dir(if has_baseline_head {
            "recover-marker-existing"
        } else {
            "recover-marker-unborn"
        });
        let repo = base.join("repo");
        let source = base.join("source");
        let connection = managed_connection();
        initialize_test_git_repo(&repo);
        if has_baseline_head {
            fs::write(repo.join("README.md"), "baseline").unwrap();
            run_git_checked(&repo, &["add", "--", "README.md"], Duration::from_secs(10))
                .unwrap();
            run_git_checked(
                &repo,
                &["commit", "-m", "baseline"],
                Duration::from_secs(30),
            )
            .unwrap();
        }
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "new skill").unwrap();
        register_backup_source(&connection, &source);

        if !has_baseline_head {
            let mut aborted_transaction = snapshot_assistants(&connection, &repo).unwrap();
            aborted_transaction.stage_for_commit().unwrap();
            std::mem::forget(aborted_transaction);
            recover_backup_transactions(&repo).unwrap();
            assert!(current_git_head(&repo).unwrap().is_none());
            assert!(!repo.join("assistants").exists());
            assert!(!repo.join("skillmate-backup.json").exists());
        }

        let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
        let transaction_root = transaction.transaction_root.clone();
        let copied_skill = repo
            .join("assistants/roots")
            .join(backup_root_id(&source))
            .join("SKILL.md");
        transaction.stage_for_commit().unwrap();
        let commit_transaction_root = transaction.transaction_root.clone();
        commit_backup_snapshot(
            &repo,
            &commit_transaction_root,
            "new backup",
            &mut transaction.journal,
        )
        .unwrap();
        std::mem::forget(transaction);

        recover_backup_transactions(&repo).unwrap();

        assert!(copied_skill.exists());
        assert!(!transaction_root.exists());
        ensure_git_worktree_clean(&repo).unwrap();
        fs::remove_dir_all(base).ok();
    }
}

#[test]
fn prepared_recovery_rejects_different_branch() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("recover-wrong-branch");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "new skill").unwrap();
    register_backup_source(&connection, &source);
    let transaction = snapshot_assistants(&connection, &repo).unwrap();
    let transaction_root = transaction.transaction_root.clone();
    let baseline_branch = transaction.journal.baseline_branch.clone().unwrap();
    run_git_checked(
        &repo,
        &["switch", "-c", "other-branch"],
        Duration::from_secs(10),
    )
    .unwrap();
    std::mem::forget(transaction);

    let error = recover_backup_transactions(&repo).unwrap_err();

    assert!(error.contains(&format!("属于分支 {}", baseline_branch)));
    assert!(transaction_root.exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn missing_main_journal_recovers_valid_atomic_write_artifact() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("journal-artifact");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "new skill").unwrap();
    register_backup_source(&connection, &source);
    let transaction = snapshot_assistants(&connection, &repo).unwrap();
    let transaction_root = transaction.transaction_root.clone();
    fs::rename(
        transaction_root.join(BACKUP_JOURNAL_FILE),
        transaction_root.join(".journal.json.skillmate-old-1"),
    )
    .unwrap();
    std::mem::forget(transaction);

    recover_backup_transactions(&repo).unwrap();

    assert!(repo.join("assistants/old.txt").exists());
    assert!(!transaction_root.exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn journal_artifacts_use_persisted_generation_across_process_sequences() {
    let base = test_dir("journal-generation");
    let transaction_root = base.join("skillmate-backup-generation");
    fs::create_dir_all(&transaction_root).unwrap();
    let stale = BackupSnapshotJournal {
        version: BACKUP_JOURNAL_VERSION,
        generation: 4,
        state: BackupSnapshotState::Prepared,
        transaction_id: "generation".to_string(),
        baseline_branch: Some("main".to_string()),
        baseline_head: None,
        expected_tree: None,
        expected_commit: None,
        previous_snapshot_marker: None,
        previous_manifest_len: None,
        previous_manifest_sha256: None,
        had_snapshot: false,
        had_manifest: false,
    };
    let current = BackupSnapshotJournal {
        generation: 5,
        state: BackupSnapshotState::RolledBack,
        ..stale.clone()
    };
    fs::write(
        transaction_root.join(BACKUP_JOURNAL_FILE),
        serde_json::to_vec(&stale).unwrap(),
    )
    .unwrap();
    fs::write(
        transaction_root.join(".journal.json.skillmate-old-999"),
        serde_json::to_vec(&stale).unwrap(),
    )
    .unwrap();
    fs::write(
        transaction_root.join(".journal.json.skillmate-tmp-7-1"),
        serde_json::to_vec(&current).unwrap(),
    )
    .unwrap();

    let recovered = read_backup_snapshot_journal(&transaction_root)
        .unwrap()
        .unwrap();

    assert_eq!(recovered.generation, 5);
    assert_eq!(recovered.state, BackupSnapshotState::RolledBack);
    fs::remove_dir_all(base).ok();
}

#[test]
fn unknown_prefixed_directory_without_owner_is_preserved() {
    let base = test_dir("unknown-prefix");
    let repo = base.join("repo");
    initialize_test_git_repo(&repo);
    let unknown = repo.join(".git/skillmate-backup-user-data");
    fs::create_dir(&unknown).unwrap();
    fs::write(unknown.join("keep"), "user data").unwrap();

    recover_backup_transactions(&repo).unwrap();

    assert_eq!(
        fs::read_to_string(unknown.join("keep")).unwrap(),
        "user data"
    );
    fs::remove_dir_all(base).ok();
}

#[test]
fn missing_journal_still_binds_owner_to_transaction_directory() {
    let base = test_dir("missing-journal-owner-binding");
    let repo = base.join("repo");
    initialize_test_git_repo(&repo);
    let transaction_root = repo.join(".git/skillmate-backup-actual");
    fs::create_dir(&transaction_root).unwrap();
    write_backup_transaction_owner(&transaction_root, "different").unwrap();
    fs::create_dir(repo.join("assistants")).unwrap();
    fs::write(
        repo.join("assistants").join(BACKUP_ROOT_MARKER),
        transaction_snapshot_marker("actual"),
    )
    .unwrap();

    let error = recover_backup_transactions(&repo).unwrap_err();

    assert!(error.contains("事务目录与日志标识不匹配"));
    assert!(transaction_root.exists());
    assert!(repo.join("assistants").exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn oversized_journal_fails_closed() {
    let base = test_dir("oversized-journal");
    let repo = base.join("repo");
    initialize_test_git_repo(&repo);
    let transaction_root = repo.join(".git/skillmate-backup-large");
    fs::create_dir(&transaction_root).unwrap();
    write_backup_transaction_owner(&transaction_root, "large").unwrap();
    fs::write(
        transaction_root.join(BACKUP_JOURNAL_FILE),
        vec![b'x'; MAX_BACKUP_JOURNAL_BYTES as usize + 1],
    )
    .unwrap();

    let error = recover_backup_transactions(&repo).unwrap_err();

    assert!(error.contains("超过"));
    assert!(transaction_root.exists());
    fs::remove_dir_all(base).ok();
}

#[cfg(unix)]
#[test]
fn unreadable_owner_and_symlinked_journal_fail_closed() {
    use std::os::unix::fs::symlink;

    let base = test_dir("journal-no-follow");
    let repo = base.join("repo");
    initialize_test_git_repo(&repo);
    let owner_root = repo.join(".git/skillmate-backup-owner");
    fs::create_dir(&owner_root).unwrap();
    let outside_owner = base.join("outside-owner");
    fs::write(&outside_owner, "owner").unwrap();
    symlink(&outside_owner, owner_root.join(BACKUP_OWNER_FILE)).unwrap();

    let owner_error = recover_backup_transactions(&repo).unwrap_err();

    assert!(owner_error.contains("所有权标记") && owner_error.contains("不是普通文件"));
    assert!(owner_root.exists());
    fs::remove_dir_all(&owner_root).unwrap();

    let journal_root = repo.join(".git/skillmate-backup-journal");
    fs::create_dir(&journal_root).unwrap();
    write_backup_transaction_owner(&journal_root, "journal").unwrap();
    let outside_journal = base.join("outside-journal");
    fs::write(&outside_journal, "{}").unwrap();
    symlink(&outside_journal, journal_root.join(BACKUP_JOURNAL_FILE)).unwrap();

    let journal_error = recover_backup_transactions(&repo).unwrap_err();

    assert!(journal_error.contains("事务日志") && journal_error.contains("不是普通文件"));
    assert!(journal_root.exists());
    fs::remove_dir_all(base).ok();
}

#[cfg(unix)]
#[test]
fn symlinked_marker_and_existing_manifest_fail_before_snapshot_activation() {
    use std::os::unix::fs::symlink;

    let base = test_dir("snapshot-metadata-no-follow");
    let repo = base.join("repo");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    fs::create_dir(repo.join("assistants")).unwrap();
    let outside_marker = base.join("outside-marker");
    fs::write(&outside_marker, "managed").unwrap();
    symlink(
        &outside_marker,
        repo.join("assistants").join(BACKUP_ROOT_MARKER),
    )
    .unwrap();

    let marker_error = snapshot_assistants(&connection, &repo)
        .err()
        .expect("软连接 marker 应被拒绝");

    assert!(marker_error.contains("管理标记") && marker_error.contains("不是普通文件"));
    fs::remove_dir_all(repo.join("assistants")).unwrap();
    let outside_manifest = base.join("outside-manifest");
    fs::write(&outside_manifest, "keep").unwrap();
    symlink(&outside_manifest, repo.join(BACKUP_SNAPSHOT_PATHS[1])).unwrap();

    let manifest_error = snapshot_assistants(&connection, &repo)
        .err()
        .expect("软连接 manifest 应被拒绝");

    assert!(manifest_error.contains("现有备份 manifest"));
    assert_eq!(fs::read_to_string(outside_manifest).unwrap(), "keep");
    assert!(!repo.join("assistants").exists());
    fs::remove_dir_all(base).ok();
}

#[cfg(unix)]
#[test]
fn git_metadata_symlink_is_rejected_without_touching_target() {
    use std::os::unix::fs::symlink;

    let base = test_dir("git-dir-symlink");
    let repo = base.join("repo");
    let outside = base.join("outside");
    let outside_transaction = outside.join("skillmate-backup-user-data");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&outside_transaction).unwrap();
    fs::write(outside_transaction.join("keep"), "user data").unwrap();
    symlink(&outside, repo.join(".git")).unwrap();

    let error = ensure_git_repo(&repo).unwrap_err();

    assert!(error.contains(".git 必须是仓库内的普通目录"));
    assert!(outside_transaction.join("keep").exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn prepared_recovery_rejects_unmanaged_snapshot_without_deleting_user_files() {
    let base = test_dir("forged-journal");
    let repo = base.join("repo");
    initialize_test_git_repo(&repo);
    fs::create_dir_all(repo.join("assistants")).unwrap();
    fs::write(repo.join("assistants/user-file"), "keep").unwrap();
    fs::write(repo.join("skillmate-backup.json"), "keep manifest").unwrap();
    let transaction_root = repo.join(".git/skillmate-backup-forged");
    fs::create_dir(&transaction_root).unwrap();
    write_backup_transaction_owner(&transaction_root, "forged").unwrap();
    let mut journal = BackupSnapshotJournal {
        version: BACKUP_JOURNAL_VERSION,
        generation: 0,
        state: BackupSnapshotState::Prepared,
        transaction_id: "forged".to_string(),
        baseline_branch: Some(current_git_branch(&repo).unwrap()),
        baseline_head: None,
        expected_tree: None,
        expected_commit: None,
        previous_snapshot_marker: None,
        previous_manifest_len: None,
        previous_manifest_sha256: None,
        had_snapshot: false,
        had_manifest: false,
    };
    update_backup_snapshot_journal(&transaction_root, &mut journal, |_| {}).unwrap();

    let error = recover_backup_transactions(&repo).unwrap_err();

    assert!(error.contains("不是 SkillMate 管理目录"));
    assert_eq!(
        fs::read_to_string(repo.join("assistants/user-file")).unwrap(),
        "keep"
    );
    assert_eq!(
        fs::read_to_string(repo.join("skillmate-backup.json")).unwrap(),
        "keep manifest"
    );
    assert!(transaction_root.exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn rolled_back_journal_cleanup_does_not_require_previous_artifacts() {
    let base = test_dir("rolled-back-cleanup");
    let repo = base.join("repo");
    initialize_test_git_repo(&repo);
    let transaction_root = repo.join(".git/skillmate-backup-rolled-back");
    fs::create_dir(&transaction_root).unwrap();
    write_backup_transaction_owner(&transaction_root, "rolled-back").unwrap();
    let mut journal = BackupSnapshotJournal {
        version: BACKUP_JOURNAL_VERSION,
        generation: 0,
        state: BackupSnapshotState::RolledBack,
        transaction_id: "rolled-back".to_string(),
        baseline_branch: Some(current_git_branch(&repo).unwrap()),
        baseline_head: None,
        expected_tree: None,
        expected_commit: None,
        previous_snapshot_marker: None,
        previous_manifest_len: None,
        previous_manifest_sha256: None,
        had_snapshot: true,
        had_manifest: true,
    };
    update_backup_snapshot_journal(&transaction_root, &mut journal, |_| {}).unwrap();

    recover_backup_transactions(&repo).unwrap();

    assert!(!transaction_root.exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn manifest_cleanup_refuses_matching_directory() {
    let base = test_dir("manifest-directory");
    let repo = base.join("repo");
    let unexpected = repo.join(".skillmate-backup.json.skillmate-tmp-user");
    fs::create_dir_all(unexpected.join("nested")).unwrap();
    fs::write(unexpected.join("nested/keep"), "user data").unwrap();

    let error = cleanup_backup_manifest_artifacts(&repo).unwrap_err();

    assert!(error.contains("已拒绝递归删除"));
    assert!(unexpected.join("nested/keep").exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn marker_window_uses_committed_tree_and_preserves_later_worktree_edits() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("marker-worktree-edit");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "new skill").unwrap();
    register_backup_source(&connection, &source);
    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    let transaction_root = transaction.transaction_root.clone();
    let copied_skill = repo
        .join("assistants/roots")
        .join(backup_root_id(&source))
        .join("SKILL.md");
    transaction.stage_for_commit().unwrap();
    let commit_transaction_root = transaction.transaction_root.clone();
    commit_backup_snapshot(
        &repo,
        &commit_transaction_root,
        "new backup",
        &mut transaction.journal,
    )
    .unwrap();
    fs::write(&copied_skill, "user edit after crash").unwrap();
    std::mem::forget(transaction);

    recover_backup_transactions(&repo).unwrap();

    assert_eq!(
        fs::read_to_string(copied_skill).unwrap(),
        "user edit after crash"
    );
    assert!(!transaction_root.exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn final_tree_rejects_content_changed_after_security_scan() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("tree-scan-binding");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "safe skill").unwrap();
    register_backup_source(&connection, &source);

    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    let copied_skill = repo
        .join("assistants/roots")
        .join(backup_root_id(&source))
        .join("SKILL.md");
    let scanned = scan_final_backup_paths(&repo).unwrap();
    fs::write(&copied_skill, "github_pat_abcdefghijklmnopqrstuvwxyz123456").unwrap();
    stage_backup_snapshot(&repo).unwrap();
    let tree = staged_git_tree(&repo).unwrap();
    validate_backup_tree_scope(&repo, &transaction.journal, &tree).unwrap();

    let error = validate_backup_tree_blobs(&repo, &tree, &scanned).unwrap_err();

    assert!(error.contains("疑似敏感内容"));
    transaction.rollback().unwrap();
    fs::remove_dir_all(base).ok();
}

#[test]
fn immutable_blob_scan_ignores_replace_refs() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let repo = test_dir("replace-ref-scan");
    initialize_test_git_repo(&repo);
    fs::create_dir(repo.join("assistants")).unwrap();
    fs::write(
        repo.join("assistants/SKILL.md"),
        "github_pat_abcdefghijklmnopqrstuvwxyz123456",
    )
    .unwrap();
    fs::write(repo.join("skillmate-backup.json"), "{}").unwrap();
    stage_backup_snapshot(&repo).unwrap();
    let tree = staged_git_tree(&repo).unwrap();
    let sensitive_oid = git_output(
        &repo,
        &["rev-parse", &format!("{}:assistants/SKILL.md", tree)],
    )
    .unwrap();
    let safe_blob = run_git_with_input(
        &repo,
        &["hash-object", "-w", "--stdin"],
        b"safe skill",
        Duration::from_secs(10),
    )
    .unwrap();
    assert!(safe_blob.status.success());
    let safe_oid = String::from_utf8_lossy(&safe_blob.stdout)
        .trim()
        .to_string();
    run_git_checked(
        &repo,
        &[
            "update-ref",
            &format!("refs/replace/{}", sensitive_oid),
            &safe_oid,
        ],
        Duration::from_secs(10),
    )
    .unwrap();
    let expected = BTreeSet::from([
        b"assistants/SKILL.md".to_vec(),
        b"skillmate-backup.json".to_vec(),
    ]);

    let error = validate_backup_tree_blobs(&repo, &tree, &expected).unwrap_err();

    assert!(error.contains("疑似敏感内容"));
    fs::remove_dir_all(repo).ok();
}

#[test]
fn exact_tree_commit_ignores_worktree_changes_after_staging() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("exact-tree-commit");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "safe skill").unwrap();
    register_backup_source(&connection, &source);

    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    let copied_skill = repo
        .join("assistants/roots")
        .join(backup_root_id(&source))
        .join("SKILL.md");
    transaction.stage_for_commit().unwrap();
    let expected_tree = transaction.journal.expected_tree.clone().unwrap();
    fs::write(&copied_skill, "github_pat_abcdefghijklmnopqrstuvwxyz123456").unwrap();

    let commit_transaction_root = transaction.transaction_root.clone();
    commit_backup_snapshot(
        &repo,
        &commit_transaction_root,
        "safe backup",
        &mut transaction.journal,
    )
    .unwrap();

    assert_eq!(git_tree(&repo, "HEAD").unwrap(), expected_tree);
    let committed = git_output(
        &repo,
        &[
            "show",
            &format!("HEAD:assistants/roots/{}/SKILL.md", backup_root_id(&source)),
        ],
    )
    .unwrap();
    assert_eq!(committed, "safe skill");
    assert!(fs::read_to_string(&copied_skill)
        .unwrap()
        .contains("github_pat_"));
    transaction.finish_commit().unwrap();
    fs::remove_dir_all(base).ok();
}

#[test]
fn git_line_ending_filter_is_scanned_after_normalization() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("clean-filter-binding");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    fs::write(repo.join(".gitattributes"), "assistants/** text eol=lf\n").unwrap();
    run_git_checked(
        &repo,
        &["add", "--", ".gitattributes"],
        Duration::from_secs(10),
    )
    .unwrap();
    run_git_checked(
        &repo,
        &["commit", "-m", "attributes"],
        Duration::from_secs(30),
    )
    .unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), b"safe\r\nskill\r\n").unwrap();
    register_backup_source(&connection, &source);

    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    transaction.stage_for_commit().unwrap();
    let expected_tree = transaction.journal.expected_tree.clone().unwrap();
    let skill_path = format!(
        "{}:assistants/roots/{}/SKILL.md",
        expected_tree,
        backup_root_id(&source)
    );
    let blob = run_git(&repo, &["show", &skill_path], Duration::from_secs(10)).unwrap();

    assert!(blob.status.success());
    assert_eq!(blob.stdout, b"safe\nskill\n");
    transaction.rollback().unwrap();
    fs::remove_dir_all(base).ok();
}

#[test]
fn changed_head_with_unconfirmed_tree_fails_closed() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("marker-tree-mismatch");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "new skill").unwrap();
    register_backup_source(&connection, &source);
    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    let transaction_root = transaction.transaction_root.clone();
    let copied_skill = repo
        .join("assistants/roots")
        .join(backup_root_id(&source))
        .join("SKILL.md");
    transaction.stage_for_commit().unwrap();
    fs::write(repo.join("external.txt"), "external commit").unwrap();
    run_git_checked(
        &repo,
        &["add", "--", "external.txt"],
        Duration::from_secs(10),
    )
    .unwrap();
    run_git_checked(
        &repo,
        &["commit", "--only", "-m", "external", "--", "external.txt"],
        Duration::from_secs(30),
    )
    .unwrap();
    std::mem::forget(transaction);

    let error = recover_backup_transactions(&repo).unwrap_err();

    assert!(error.contains("拒绝自动回滚"));
    assert!(copied_skill.exists());
    assert!(transaction_root.exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn same_tree_external_commit_is_not_mistaken_for_transaction_commit() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("exact-commit-recovery");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "new skill").unwrap();
    register_backup_source(&connection, &source);
    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    transaction.stage_for_commit().unwrap();
    let transaction_root = transaction.transaction_root.clone();
    let tree = transaction.journal.expected_tree.clone().unwrap();
    let baseline = transaction.journal.baseline_head.clone().unwrap();
    let expected_commit = git_output(
        &repo,
        &[
            "commit-tree",
            &tree,
            "-p",
            &baseline,
            "-m",
            "expected transaction commit",
        ],
    )
    .unwrap();
    update_backup_snapshot_journal(&transaction_root, &mut transaction.journal, |journal| {
        journal.expected_commit = Some(expected_commit.clone())
    })
    .unwrap();
    let external_commit = git_output(
        &repo,
        &[
            "commit-tree",
            &tree,
            "-p",
            &baseline,
            "-m",
            "external commit with the same tree",
        ],
    )
    .unwrap();
    assert_ne!(external_commit, expected_commit);
    let reference = format!(
        "refs/heads/{}",
        transaction.journal.baseline_branch.as_deref().unwrap()
    );
    run_git_checked(
        &repo,
        &["update-ref", &reference, &external_commit, &baseline],
        Duration::from_secs(10),
    )
    .unwrap();
    std::mem::forget(transaction);

    let error = recover_backup_transactions(&repo).unwrap_err();

    assert!(error.contains("不是本事务提交"));
    assert!(transaction_root.exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn deleted_baseline_branch_ref_fails_closed_instead_of_rolling_back() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let base = test_dir("deleted-baseline-ref");
    let repo = base.join("repo");
    let source = base.join("source");
    let connection = managed_connection();
    initialize_test_git_repo(&repo);
    commit_old_backup_snapshot(&repo);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "new skill").unwrap();
    register_backup_source(&connection, &source);
    let mut transaction = snapshot_assistants(&connection, &repo).unwrap();
    let transaction_root = transaction.transaction_root.clone();
    let branch = current_git_branch(&repo).unwrap();
    let reference = format!("refs/heads/{}", branch);
    run_git_checked(
        &repo,
        &["update-ref", "-d", &reference],
        Duration::from_secs(10),
    )
    .unwrap();

    let error = transaction.rollback().unwrap_err();

    assert!(error.contains("变为 unborn"));
    assert!(transaction_root.exists());
    assert!(repo.join("assistants").exists());
    assert!(transaction_root.join("previous-assistants").exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn backup_filter_excludes_runtime_and_sensitive_metadata() {
    for name in [
        ".skillmate-state.json",
        ".env",
        "credentials.json",
        "token.txt",
        "github_token.txt",
        "api_key",
        "private.key",
        "node_modules",
        "__pycache__",
    ] {
        assert!(backup_exclusion_reason(name).is_some(), "{} 应被排除", name);
    }
    assert!(backup_exclusion_reason("SKILL.md").is_none());
    assert!(backup_exclusion_reason("references").is_none());
    assert!(backup_exclusion_reason(".github").is_none());
    assert!(backup_exclusion_reason(".config").is_none());
    assert!(backup_exclusion_reason("tokenizer.json").is_none());
}

#[test]
fn backup_repo_rejects_project_managed_root_overlap() {
    let connection = managed_connection();
    let base = test_dir("project-root-overlap");
    let managed_root = base.join("project/.codex/skills");
    let repo = managed_root.join("backup");
    fs::create_dir_all(&managed_root).unwrap();
    connection
        .execute(
            "INSERT INTO managed_roots (root_path, scope, project_path, updated_at)
             VALUES (?, 'project', ?, 'now')",
            params![
                managed_root.to_string_lossy().to_string(),
                base.to_string_lossy().to_string()
            ],
        )
        .unwrap();

    let error = validate_backup_repo_location(&connection, &repo).unwrap_err();

    assert!(error.contains("项目级受管 Skills 目录"));
    fs::remove_dir_all(base).ok();
}

#[test]
fn backup_sources_include_only_explicitly_managed_skills() {
    let connection = managed_connection();
    let base = test_dir("managed-only");
    let root = base.join("skills");
    let managed = root.join("managed");
    let unmanaged = root.join("unmanaged");
    fs::create_dir_all(&managed).unwrap();
    fs::create_dir_all(&unmanaged).unwrap();
    fs::write(managed.join("SKILL.md"), "managed").unwrap();
    fs::write(unmanaged.join("SKILL.md"), "private").unwrap();
    connection
        .execute(
            "INSERT INTO managed_installations (
                skill_path, assistant, source, source_kind, target_name, scope,
                install_mode, installed_at
             ) VALUES (?, 'Codex', 'local', 'local', 'managed', 'global', 'copy', 'now')",
            [managed.to_string_lossy().to_string()],
        )
        .unwrap();

    let sources = collect_backup_sources(&connection).unwrap();

    assert_eq!(sources.len(), 1);
    assert_eq!(sources.values().next().unwrap().path, managed);
    assert!(!sources.values().any(|source| source.path == root));
    assert!(!sources.values().any(|source| source.path == unmanaged));
    fs::remove_dir_all(base).ok();
}

#[cfg(unix)]
#[test]
fn backup_source_dedup_prefers_real_path_in_any_order() {
    use std::os::unix::fs::symlink;

    let base = test_dir("source-dedup");
    let real = base.join("real");
    let linked = base.join("linked");
    fs::create_dir_all(&real).unwrap();
    symlink(&real, &linked).unwrap();

    for paths in [
        [linked.clone(), real.clone()],
        [real.clone(), linked.clone()],
    ] {
        let mut sources = BTreeMap::new();
        for path in paths {
            add_backup_source(&mut sources, path, "Codex", "global", None).unwrap();
        }
        assert_eq!(sources.len(), 1);
        assert_eq!(sources.values().next().unwrap().path, real);
    }
    fs::remove_dir_all(base).ok();
}

#[test]
fn backup_copy_keeps_hidden_assets_and_records_exclusions() {
    let base = test_dir("copy-policy");
    let source = base.join("source");
    let target = base.join("target");
    fs::create_dir_all(source.join(".github")).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "password: provided by user at runtime",
    )
    .unwrap();
    fs::write(source.join(".github/config.yml"), "visible").unwrap();
    fs::write(source.join(".env"), "SECRET=value").unwrap();
    fs::write(source.join("private.key"), "key").unwrap();
    fs::write(
        source.join("settings.json"),
        r#"{"access_token":"github_pat_abcdefghijklmnopqrstuvwxyz123456"}"#,
    )
    .unwrap();
    fs::write(
        source.join("settings.example.json"),
        r#"{"access_token":"${GITHUB_TOKEN}"}"#,
    )
    .unwrap();
    let mut budget = BackupCopyBudget::default();
    let mut report = BackupCopyReport::default();

    copy_backup_tree(&source, &target, &source, 0, &mut budget, &mut report).unwrap();

    assert!(target.join("SKILL.md").exists());
    assert!(target.join(".github/config.yml").exists());
    assert!(!target.join(".env").exists());
    assert!(!target.join("private.key").exists());
    assert!(!target.join("settings.json").exists());
    assert!(target.join("settings.example.json").exists());
    assert_eq!(report.copied_files, 3);
    assert!(report
        .exclusions
        .iter()
        .any(|entry| entry.path == ".env" && entry.reason == "sensitive"));
    assert!(report
        .exclusions
        .iter()
        .any(|entry| entry.path == "private.key" && entry.reason == "sensitive"));
    assert!(report
        .exclusions
        .iter()
        .any(|entry| { entry.path == "settings.json" && entry.reason == "sensitive_content" }));
    fs::remove_dir_all(base).ok();
}

#[test]
fn sensitive_scan_avoids_runtime_documentation_and_detects_known_tokens() {
    let base = test_dir("sensitive-content");
    fs::create_dir_all(&base).unwrap();
    let file = base.join("content.txt");
    fs::write(&file, "password: provided by user at runtime").unwrap();
    assert_eq!(scan_backup_file(&file).unwrap(), SensitiveScan::Safe);

    for token in [
        "sk-proj-abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
        "AKIA1234567890ABCDEF",
        // Slack 令牌样本用转义拼接,避免触发托管平台密钥扫描误报
        concat!("xox", "b-123456789012-123456789012-abcdefghijklmnopqrstuvwxyzABCD"),
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.c2lnbmF0dXJlX3dpdGhfbGVuZ3Ro",
    ] {
        fs::write(&file, token).unwrap();
        assert_eq!(
            scan_backup_file(&file).unwrap(),
            SensitiveScan::Sensitive,
            "应识别高置信凭据: {token}"
        );
    }

    fs::write(&file, vec![b'a'; MAX_SENSITIVE_SCAN_BYTES as usize + 1]).unwrap();
    assert_eq!(
        scan_backup_file(&file).unwrap(),
        SensitiveScan::Unscannable("unscannable_too_large")
    );
    fs::write(&file, [0xff, 0xfe, 0xfd]).unwrap();
    assert_eq!(
        scan_backup_file(&file).unwrap(),
        SensitiveScan::Unscannable("unscannable_encoding")
    );
    fs::remove_dir_all(base).ok();
}

#[test]
fn copied_backup_bytes_are_the_same_bytes_that_were_scanned() {
    let base = test_dir("scan-copy-same-bytes");
    let source = base.join("source.txt");
    let target = base.join("target.txt");
    fs::create_dir_all(&base).unwrap();
    fs::write(&source, "safe content").unwrap();
    let metadata = fs::symlink_metadata(&source).unwrap();
    let scanned = read_scanned_backup_file(&source, &metadata).unwrap();
    fs::write(&source, "github_pat_abcdefghijklmnopqrstuvwxyz123456").unwrap();

    write_scanned_backup_file(&target, &scanned).unwrap();

    assert_eq!(fs::read_to_string(target).unwrap(), "safe content");
    fs::remove_dir_all(base).ok();
}

#[test]
fn sensitive_core_skill_file_aborts_snapshot() {
    let base = test_dir("sensitive-core");
    let source = base.join("source");
    let target = base.join("target");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "github_pat_abcdefghijklmnopqrstuvwxyz123456",
    )
    .unwrap();
    let mut budget = BackupCopyBudget::default();
    let mut report = BackupCopyReport::default();

    let error =
        copy_backup_tree(&source, &target, &source, 0, &mut budget, &mut report).unwrap_err();

    assert!(error.contains("核心 Skill 文件"));
    fs::remove_dir_all(base).ok();
}

#[test]
fn unscannable_core_skill_file_aborts_snapshot() {
    let base = test_dir("unscannable-core");
    let source = base.join("source");
    let target = base.join("target");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), b"skill\0content").unwrap();
    let mut budget = BackupCopyBudget::default();
    let mut report = BackupCopyReport::default();

    let error =
        copy_backup_tree(&source, &target, &source, 0, &mut budget, &mut report).unwrap_err();

    assert!(error.contains("核心 Skill 文件"));
    fs::remove_dir_all(base).ok();
}

#[test]
fn unscannable_non_core_file_is_excluded_and_reported() {
    let base = test_dir("unscannable-asset");
    let source = base.join("source");
    let target = base.join("target");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "skill").unwrap();
    fs::write(source.join("asset.bin"), b"asset\0content").unwrap();
    let mut budget = BackupCopyBudget::default();
    let mut report = BackupCopyReport::default();

    copy_backup_tree(&source, &target, &source, 0, &mut budget, &mut report).unwrap();

    assert!(!target.join("asset.bin").exists());
    assert!(report
        .exclusions
        .iter()
        .any(|entry| entry.path == "asset.bin" && entry.reason == "unscannable_binary"));
    fs::remove_dir_all(base).ok();
}

#[test]
fn sensitive_exclusions_still_consume_visit_budget() {
    let base = test_dir("sensitive-budget");
    let source = base.join("source");
    let target = base.join("target");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("settings.json"),
        r#"{"access_token":"github_pat_abcdefghijklmnopqrstuvwxyz123456"}"#,
    )
    .unwrap();
    let mut budget = BackupCopyBudget {
        files: MAX_BACKUP_FILES,
        bytes: 0,
    };
    let mut report = BackupCopyReport::default();

    let error =
        copy_backup_tree(&source, &target, &source, 0, &mut budget, &mut report).unwrap_err();

    assert!(error.contains("备份超过限制"));
    fs::remove_dir_all(base).ok();
}

#[cfg(unix)]
#[test]
fn backup_copy_records_symlink_without_following_it() {
    use std::os::unix::fs::symlink;

    let base = test_dir("copy-symlink");
    let source = base.join("source");
    let target = base.join("target");
    let outside = base.join("outside");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    symlink(&outside, source.join("linked-assets")).unwrap();
    let mut budget = BackupCopyBudget::default();
    let mut report = BackupCopyReport::default();

    copy_backup_tree(&source, &target, &source, 0, &mut budget, &mut report).unwrap();

    assert!(!target.join("linked-assets").exists());
    assert!(report
        .exclusions
        .iter()
        .any(|entry| entry.path == "linked-assets" && entry.reason == "symlink"));
    fs::remove_dir_all(base).ok();
}

#[cfg(unix)]
#[test]
fn backup_source_root_symlink_is_not_followed() {
    use std::os::unix::fs::symlink;

    let base = test_dir("source-root-symlink");
    let outside = base.join("outside");
    let linked = base.join("linked");
    let target = base.join("target");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    symlink(&outside, &linked).unwrap();
    let source = BackupSource {
        path: linked,
        ..BackupSource::default()
    };
    let mut budget = BackupCopyBudget::default();
    let mut report = BackupCopyReport::default();

    let copied = snapshot_backup_source(&source, &target, &mut budget, &mut report).unwrap();

    assert!(!copied);
    assert!(!target.exists());
    assert!(report
        .exclusions
        .iter()
        .any(|entry| entry.path == "." && entry.reason == "symlink"));
    fs::remove_dir_all(base).ok();
}

#[test]
fn overlap_check_is_bidirectional() {
    let root = PathBuf::from("/tmp/skillmate-overlap");
    let nested = root.join("nested/repo");

    assert!(paths_overlap(&root, &nested));
    assert!(paths_overlap(&nested, &root));
    assert!(!paths_overlap(&root, Path::new("/tmp/other")));
}

#[test]
fn worktree_status_failure_is_not_treated_as_clean() {
    let root = test_dir("not-a-repo");
    fs::create_dir_all(&root).unwrap();

    let error = ensure_git_worktree_clean(&root).unwrap_err();

    assert!(error.contains("Git 工作区状态"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn git_commands_ignore_redirecting_environment() {
    const CHILD_FLAG: &str = "SKILLMATE_GIT_ENV_TEST_CHILD";
    const CHILD_REPO: &str = "SKILLMATE_GIT_ENV_TEST_REPO";
    if let Some(repo) = std::env::var_os(CHILD_REPO) {
        let repo = PathBuf::from(repo);
        let actual =
            PathBuf::from(git_output(&repo, &["rev-parse", "--show-toplevel"]).unwrap());
        assert_eq!(actual.canonicalize().unwrap(), repo.canonicalize().unwrap());
        fs::write(repo.join("target.txt"), "target").unwrap();
        run_git_checked(&repo, &["add", "--", "target.txt"], Duration::from_secs(10)).unwrap();
        return;
    }
    if std::env::var_os(CHILD_FLAG).is_some()
        || Command::new("git").arg("--version").output().is_err()
    {
        return;
    }

    let base = test_dir("git-environment");
    let repo = base.join("repo");
    let redirected = base.join("redirected");
    initialize_test_git_repo(&repo);
    initialize_test_git_repo(&redirected);
    let redirected_config = base.join("redirected.gitconfig");
    fs::write(&redirected_config, "[core]\n\tbare = true\n").unwrap();
    let shallow_file = base.join("redirected-shallow");
    fs::write(&shallow_file, "invalid\n").unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("git_backup::tests::git_commands_ignore_redirecting_environment")
        .arg("--exact")
        .arg("--nocapture")
        .env(CHILD_FLAG, "1")
        .env(CHILD_REPO, &repo)
        .env("GIT_DIR", redirected.join(".git"))
        .env("GIT_WORK_TREE", &redirected)
        .env("GIT_COMMON_DIR", redirected.join(".git"))
        .env("GIT_INDEX_FILE", redirected.join(".git/redirected-index"))
        .env("GIT_OBJECT_DIRECTORY", redirected.join(".git/objects"))
        .env(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            redirected.join(".git/objects"),
        )
        .env("GIT_NAMESPACE", "redirected")
        .env("GIT_CONFIG", &redirected_config)
        .env("GIT_SHALLOW_FILE", &shallow_file)
        .env("GIT_FUTURE_REDIRECT", "must-be-removed")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "子进程失败:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git_output(&repo, &["diff", "--cached", "--name-only"]).unwrap(),
        "target.txt"
    );
    assert!(!redirected.join(".git/redirected-index").exists());
    fs::remove_dir_all(base).ok();
}

#[test]
fn backup_commit_limits_initial_commit_to_managed_paths() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let repo = test_dir("scoped-initial-commit");
    fs::create_dir_all(repo.join("assistants")).unwrap();
    ensure_git_repo(&repo).unwrap();
    ensure_git_identity(&repo).unwrap();
    fs::write(repo.join("assistants/SKILL.md"), "skill").unwrap();
    fs::write(repo.join("skillmate-backup.json"), "{}").unwrap();
    fs::write(repo.join("private.env"), "TOKEN=private").unwrap();

    stage_backup_snapshot(&repo).unwrap();
    let initially_staged = git_output(&repo, &["diff", "--cached", "--name-only"]).unwrap();
    assert!(initially_staged.contains("assistants/SKILL.md"));
    assert!(initially_staged.contains("skillmate-backup.json"));
    assert!(!initially_staged.contains("private.env"));
    let transaction_root = repo.join(".git/skillmate-backup-scoped-initial-commit");
    fs::create_dir(&transaction_root).unwrap();
    let mut journal = BackupSnapshotJournal {
        version: BACKUP_JOURNAL_VERSION,
        generation: 1,
        state: BackupSnapshotState::Prepared,
        transaction_id: "scoped-initial-commit".to_string(),
        baseline_branch: Some(current_git_branch(&repo).unwrap()),
        baseline_head: None,
        expected_tree: Some(staged_git_tree(&repo).unwrap()),
        expected_commit: None,
        previous_snapshot_marker: None,
        previous_manifest_len: None,
        previous_manifest_sha256: None,
        had_snapshot: false,
        had_manifest: false,
    };
    update_backup_snapshot_journal(&transaction_root, &mut journal, |_| {}).unwrap();

    run_git_checked(
        &repo,
        &["add", "--", "private.env"],
        Duration::from_secs(10),
    )
    .unwrap();
    commit_backup_snapshot(&repo, &transaction_root, "backup", &mut journal).unwrap();

    let committed = git_output(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]).unwrap();
    assert!(committed.contains("assistants/SKILL.md"));
    assert!(committed.contains("skillmate-backup.json"));
    assert!(!committed.contains("private.env"));
    assert_eq!(
        git_output(&repo, &["diff", "--cached", "--name-only"]).unwrap(),
        "private.env"
    );
    fs::remove_dir_all(repo).ok();
}

#[test]
fn backup_stage_rejects_external_cached_paths_and_scoped_unstage_preserves_them() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let repo = test_dir("scoped-unstage");
    fs::create_dir_all(repo.join("assistants")).unwrap();
    ensure_git_repo(&repo).unwrap();
    fs::write(repo.join("assistants/SKILL.md"), "skill").unwrap();
    fs::write(repo.join("skillmate-backup.json"), "{}").unwrap();
    fs::write(repo.join("private.env"), "TOKEN=private").unwrap();
    run_git_checked(
        &repo,
        &["add", "--", "private.env"],
        Duration::from_secs(10),
    )
    .unwrap();

    let error = stage_backup_snapshot(&repo).unwrap_err();
    assert!(error.contains("非 SkillMate 备份路径"));
    unstage_backup_snapshot(&repo).unwrap();

    assert_eq!(
        git_output(&repo, &["diff", "--cached", "--name-only"]).unwrap(),
        "private.env"
    );
    fs::remove_dir_all(repo).ok();
}

#[test]
fn branch_switch_preserves_history_and_rejects_dirty_tree() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let root = test_dir("branch");
    fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.name", "SkillMate Test"]);
    git(&["config", "user.email", "skillmate-test@example.com"]);
    fs::write(root.join("main.txt"), "main").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "main"]);
    git(&["switch", "-c", "backup"]);
    fs::write(root.join("backup.txt"), "backup").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "backup"]);
    let backup_head = git_output(&root, &["rev-parse", "HEAD"]).unwrap();
    git(&["switch", "main"]);

    ensure_git_worktree_clean(&root).unwrap();
    checkout_git_branch(&root, "backup").unwrap();

    assert_eq!(
        git_output(&root, &["rev-parse", "HEAD"]).unwrap(),
        backup_head
    );
    fs::write(root.join("backup.txt"), "dirty").unwrap();
    assert!(ensure_git_worktree_clean(&root).is_err());
    fs::remove_dir_all(root).ok();
}
