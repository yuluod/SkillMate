use super::*;

use crate::app_core::atomic_write;

use std::path::Path;
use std::time::Duration;

pub(super) fn stage_backup_snapshot(repo: &Path) -> Result<(), String> {
    run_git_checked(
        repo,
        &[
            "add",
            "-f",
            "-A",
            "--",
            BACKUP_SNAPSHOT_PATHS[0],
            BACKUP_SNAPSHOT_PATHS[1],
        ],
        Duration::from_secs(30),
    )?;
    validate_staged_backup_paths(repo)
}

pub(super) fn validate_staged_backup_paths(repo: &Path) -> Result<(), String> {
    let staged = run_git(
        repo,
        &["diff", "--cached", "--name-only", "--no-renames", "-z"],
        Duration::from_secs(10),
    )?;
    if !staged.status.success() {
        return Err(format!("检查 Git 暂存区失败: {}", command_output(&staged)));
    }
    if let Some(path) = staged
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .find(|path| !is_backup_snapshot_path(path))
    {
        return Err(format!(
            "Git 暂存区包含非 SkillMate 备份路径: {}",
            String::from_utf8_lossy(path)
        ));
    }
    Ok(())
}

pub(super) fn is_backup_snapshot_path(path: &[u8]) -> bool {
    path == BACKUP_SNAPSHOT_PATHS[0].as_bytes()
        || path.starts_with(b"assistants/")
        || path == BACKUP_SNAPSHOT_PATHS[1].as_bytes()
}

pub(super) fn commit_backup_snapshot(
    repo: &Path,
    transaction_root: &Path,
    message: &str,
    journal: &mut BackupSnapshotJournal,
) -> Result<(), String> {
    ensure_backup_transaction_branch(repo, journal)?;
    let current_head = current_git_head(repo)?;
    if current_head != journal.baseline_head {
        return Err("备份提交前 Git HEAD 已变化，已拒绝提交".to_string());
    }
    let expected_tree = journal
        .expected_tree
        .as_deref()
        .ok_or_else(|| "备份事务缺少已验证的 Git tree".to_string())?;
    let mut args = vec!["commit-tree", expected_tree];
    if let Some(parent) = journal.baseline_head.as_deref() {
        args.extend(["-p", parent]);
    }
    args.extend(["-m", message]);
    let commit = run_git(repo, &args, Duration::from_secs(30))?;
    if !commit.status.success() {
        return Err(format!("创建备份提交失败: {}", command_output(&commit)));
    }
    let commit_id = String::from_utf8_lossy(&commit.stdout).trim().to_string();
    if commit_id.is_empty() {
        return Err("创建备份提交后未返回 commit ID".to_string());
    }
    update_backup_snapshot_journal(transaction_root, journal, |journal| {
        journal.expected_commit = Some(commit_id.clone())
    })?;
    let branch = journal
        .baseline_branch
        .as_deref()
        .ok_or_else(|| "备份事务缺少基线分支".to_string())?;
    ensure_backup_transaction_branch(repo, journal)?;
    let reference = format!("refs/heads/{}", branch);
    let baseline = journal.baseline_head.as_deref().unwrap_or("");
    run_git_checked(
        repo,
        &["update-ref", &reference, &commit_id, baseline],
        Duration::from_secs(10),
    )?;
    ensure_backup_transaction_branch(repo, journal).map_err(|error| {
        format!(
            "备份提交已写入原分支，但提交期间当前分支发生变化；已保留事务现场: {}",
            error
        )
    })?;
    if current_git_head(repo)?.as_deref() != Some(commit_id.as_str()) {
        return Err("备份提交后分支 HEAD 再次变化，已保留事务现场".to_string());
    }
    Ok(())
}

pub(super) fn unstage_backup_snapshot(repo: &Path) -> Result<(), String> {
    let reset = run_git(
        repo,
        &[
            "reset",
            "--",
            BACKUP_SNAPSHOT_PATHS[0],
            BACKUP_SNAPSHOT_PATHS[1],
        ],
        Duration::from_secs(10),
    )?;
    if reset.status.success() {
        return Ok(());
    }
    let clear = run_git(
        repo,
        &[
            "rm",
            "--cached",
            "-r",
            "--ignore-unmatch",
            "--",
            BACKUP_SNAPSHOT_PATHS[0],
            BACKUP_SNAPSHOT_PATHS[1],
        ],
        Duration::from_secs(10),
    )?;
    if clear.status.success() {
        Ok(())
    } else {
        Err(format!(
            "恢复 Git 暂存区失败: {}；{}",
            command_output(&reset),
            command_output(&clear)
        ))
    }
}

pub(super) fn update_backup_snapshot_journal(
    transaction_root: &Path,
    journal: &mut BackupSnapshotJournal,
    update: impl FnOnce(&mut BackupSnapshotJournal),
) -> Result<(), String> {
    let mut next = journal.clone();
    update(&mut next);
    next.generation = next
        .generation
        .checked_add(1)
        .ok_or_else(|| "备份事务日志 generation 已耗尽".to_string())?;
    let payload = serde_json::to_vec(&next).map_err(|error| error.to_string())?;
    let journal_path = transaction_root.join(BACKUP_JOURNAL_FILE);
    if let Err(error) = atomic_write(&journal_path, &payload) {
        let published =
            read_bounded_regular_file(&journal_path, "备份事务日志", MAX_BACKUP_JOURNAL_BYTES)?
                .is_some_and(|current| current == payload);
        if !published {
            return Err(error);
        }
    }
    *journal = next;
    Ok(())
}
