use super::*;

use crate::app_core::atomic_write;
use crate::operation_plan::StableHash;
use serde::{Deserialize, Serialize};

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) fn write_backup_transaction_owner(
    transaction_root: &Path,
    transaction_id: &str,
) -> Result<(), String> {
    let owner_path = transaction_root.join(BACKUP_OWNER_FILE);
    let mut owner = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&owner_path)
        .map_err(|error| format!("无法创建备份事务所有权标记: {}", error))?;
    owner
        .write_all(transaction_id.as_bytes())
        .and_then(|_| owner.sync_all())
        .map_err(|error| format!("无法写入备份事务所有权标记: {}", error))
}

pub(super) fn read_backup_transaction_owner(
    transaction_root: &Path,
) -> Result<Option<String>, String> {
    let owner_path = transaction_root.join(BACKUP_OWNER_FILE);
    let Some(payload) = read_bounded_regular_file(&owner_path, "备份事务所有权标记", 256)?
    else {
        return Ok(None);
    };
    let owner =
        String::from_utf8(payload).map_err(|_| "备份事务所有权标记不是 UTF-8 文本".to_string())?;
    let owner = owner.trim().to_string();
    if owner.is_empty() {
        Err("备份事务所有权标记为空".to_string())
    } else {
        Ok(Some(owner))
    }
}

pub(super) fn validate_backup_transaction_owner(
    transaction_root: &Path,
    transaction_id: &str,
) -> Result<(), String> {
    validate_backup_transaction_directory(transaction_root, transaction_id)?;
    match read_backup_transaction_owner(transaction_root)? {
        Some(owner) if owner == transaction_id => Ok(()),
        Some(_) => Err("备份事务所有权标记与日志不匹配".to_string()),
        None => Err("备份事务缺少所有权标记，已拒绝恢复".to_string()),
    }
}

pub(super) fn validate_backup_transaction_directory(
    transaction_root: &Path,
    transaction_id: &str,
) -> Result<(), String> {
    let expected_name = format!("{}{}", BACKUP_TRANSACTION_PREFIX, transaction_id);
    if transaction_root.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
        return Err("备份事务目录与日志标识不匹配".to_string());
    }
    Ok(())
}

pub(super) fn journal_artifact_order(name: &str) -> Option<(u64, u8)> {
    let kind = if name.starts_with(".journal.json.skillmate-tmp-") {
        1
    } else if name.starts_with(".journal.json.skillmate-old-") {
        0
    } else {
        return None;
    };
    let sequence = name.rsplit('-').next()?.parse().ok()?;
    Some((sequence, kind))
}

pub(super) fn read_backup_snapshot_journal(
    transaction_root: &Path,
) -> Result<Option<BackupSnapshotJournal>, String> {
    let journal_path = transaction_root.join(BACKUP_JOURNAL_FILE);
    let mut valid = Vec::new();
    if let Some(payload) =
        read_bounded_regular_file(&journal_path, "备份事务日志", MAX_BACKUP_JOURNAL_BYTES)?
    {
        let journal = serde_json::from_slice::<BackupSnapshotJournal>(&payload)
            .map_err(|error| format!("备份事务日志损坏: {}", error))?;
        valid.push((journal.generation, (1, 0, 0), journal));
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(transaction_root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let Some(order) = name.to_str().and_then(journal_artifact_order) else {
            continue;
        };
        candidates.push((order, entry.path()));
    }
    let had_candidate = !valid.is_empty() || !candidates.is_empty();
    for (order, path) in candidates {
        let label = format!("备份事务日志恢复副本 {}", path.to_string_lossy());
        let Some(payload) = read_bounded_regular_file(&path, &label, MAX_BACKUP_JOURNAL_BYTES)?
        else {
            continue;
        };
        let journal = serde_json::from_slice::<BackupSnapshotJournal>(&payload)
            .map_err(|error| format!("备份事务日志恢复副本损坏: {}", error))?;
        valid.push((journal.generation, (0, order.0, order.1), journal));
    }
    if let Some((_, _, journal)) = valid.into_iter().max_by_key(|item| (item.0, item.1)) {
        return Ok(Some(journal));
    }
    if had_candidate {
        Err("备份事务日志及其恢复副本均已损坏".to_string())
    } else {
        Ok(None)
    }
}

pub(super) fn ensure_backup_transaction_branch(
    repo: &Path,
    journal: &BackupSnapshotJournal,
) -> Result<(), String> {
    let expected = journal
        .baseline_branch
        .as_deref()
        .ok_or_else(|| "备份事务缺少基线分支，已保留现场并拒绝自动恢复".to_string())?;
    let current = current_git_branch(repo)?;
    if current == expected {
        Ok(())
    } else {
        Err(format!(
            "备份事务属于分支 {}，当前位于 {}；请切回原分支后重试",
            expected, current
        ))
    }
}

pub(super) fn recover_backup_transactions(repo: &Path) -> Result<(), String> {
    let git_dir = validated_backup_git_dir(repo)?;
    let mut transaction_roots = Vec::new();
    for entry in fs::read_dir(&git_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(BACKUP_TRANSACTION_PREFIX)
            && entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
        {
            transaction_roots.push(entry.path());
        }
    }
    transaction_roots.sort();
    for transaction_root in transaction_roots {
        let owner = match read_backup_transaction_owner(&transaction_root) {
            Ok(Some(owner)) => owner,
            Ok(None) => continue,
            Err(error) => return Err(error),
        };
        validate_backup_transaction_directory(&transaction_root, &owner)?;
        let Some(mut journal) = read_backup_snapshot_journal(&transaction_root)? else {
            let snapshot_marker =
                read_managed_snapshot_marker(&repo.join(BACKUP_SNAPSHOT_PATHS[0]), "当前备份快照")?;
            let owned_marker = transaction_snapshot_marker(&owner);
            let activated = snapshot_marker.as_deref() == Some(owned_marker.as_str())
                || transaction_root.join("previous-assistants").exists();
            if activated {
                return Err("备份事务日志缺失但快照已启用，已保留现场并拒绝清理".to_string());
            }
            remove_backup_path(&transaction_root)?;
            continue;
        };
        if journal.version != BACKUP_JOURNAL_VERSION {
            return Err(format!("不支持的备份事务日志版本: {}", journal.version));
        }
        validate_backup_transaction_owner(&transaction_root, &journal.transaction_id)?;
        match journal.state {
            BackupSnapshotState::Committed | BackupSnapshotState::RolledBack => {
                cleanup_backup_manifest_artifacts(repo)?;
                remove_backup_path(&transaction_root)?;
            }
            BackupSnapshotState::Prepared => {
                ensure_backup_transaction_branch(repo, &journal)?;
                if prepared_backup_was_committed(repo, &journal)? {
                    update_backup_snapshot_journal(&transaction_root, &mut journal, |journal| {
                        journal.state = BackupSnapshotState::Committed
                    })?;
                    cleanup_backup_manifest_artifacts(repo)?;
                    remove_backup_path(&transaction_root)?;
                } else {
                    restore_prepared_backup_transaction(repo, &transaction_root, &mut journal)?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn restore_prepared_backup_transaction(
    repo: &Path,
    transaction_root: &Path,
    journal: &mut BackupSnapshotJournal,
) -> Result<(), String> {
    validate_backup_transaction_owner(transaction_root, &journal.transaction_id)?;
    ensure_backup_transaction_branch(repo, journal)?;
    let previous_manifest = if journal.had_manifest {
        let previous_manifest_path = transaction_root.join(BACKUP_PREVIOUS_MANIFEST_FILE);
        let content = read_bounded_regular_file(
            &previous_manifest_path,
            "旧备份 manifest",
            MAX_BACKUP_MANIFEST_BYTES,
        )?
        .ok_or_else(|| "旧备份 manifest 缺失，已保留现场并拒绝恢复".to_string())?;
        validate_previous_manifest(&content, journal)?;
        Some(content)
    } else {
        if journal.previous_manifest_len.is_some() || journal.previous_manifest_sha256.is_some() {
            return Err("备份事务日志的旧 manifest 状态不一致".to_string());
        }
        None
    };
    let snapshot_root = repo.join(BACKUP_SNAPSHOT_PATHS[0]);
    let backup_root = transaction_root.join("previous-assistants");
    let transaction_marker = transaction_snapshot_marker(&journal.transaction_id);
    if journal.had_snapshot {
        let previous_marker = journal
            .previous_snapshot_marker
            .as_deref()
            .ok_or_else(|| "备份事务缺少旧快照标记，已保留现场并拒绝恢复".to_string())?;
        if let Some(backup_marker) = read_managed_snapshot_marker(&backup_root, "旧备份快照")?
        {
            if backup_marker != previous_marker {
                return Err("旧备份快照标记与事务日志不匹配".to_string());
            }
            if let Some(current_marker) =
                read_managed_snapshot_marker(&snapshot_root, "当前备份快照")?
            {
                if current_marker != transaction_marker {
                    return Err("当前备份快照不属于待恢复事务，已拒绝覆盖".to_string());
                }
            }
            remove_backup_path(&snapshot_root)?;
            fs::rename(&backup_root, &snapshot_root)
                .map_err(|error| format!("恢复旧备份快照失败: {}", error))?;
        } else {
            let restored_marker = read_managed_snapshot_marker(&snapshot_root, "已恢复的备份快照")?
                .ok_or_else(|| "旧备份快照与当前快照均不存在，无法恢复".to_string())?;
            if restored_marker != previous_marker {
                return Err("已恢复的备份快照标记与事务日志不匹配".to_string());
            }
        }
    } else {
        if let Some(current_marker) =
            read_managed_snapshot_marker(&snapshot_root, "待回滚的备份快照")?
        {
            if current_marker != transaction_marker {
                return Err("当前备份快照不属于待回滚事务，已拒绝删除".to_string());
            }
            remove_backup_path(&snapshot_root)?;
        }
    }
    let manifest_path = repo.join(BACKUP_SNAPSHOT_PATHS[1]);
    match previous_manifest {
        Some(content) => atomic_write(&manifest_path, &content)?,
        None => remove_backup_path(&manifest_path)?,
    }
    cleanup_backup_manifest_artifacts(repo)?;
    unstage_backup_snapshot(repo)?;
    update_backup_snapshot_journal(transaction_root, journal, |journal| {
        journal.state = BackupSnapshotState::RolledBack;
    })?;
    remove_backup_path(transaction_root)
}

pub(super) fn validate_previous_manifest(
    content: &[u8],
    journal: &BackupSnapshotJournal,
) -> Result<(), String> {
    let expected_len = journal
        .previous_manifest_len
        .ok_or_else(|| "备份事务日志缺少旧 manifest 长度".to_string())?;
    let expected_hash = journal
        .previous_manifest_sha256
        .as_deref()
        .ok_or_else(|| "备份事务日志缺少旧 manifest 摘要".to_string())?;
    if content.len() as u64 != expected_len {
        return Err("旧备份 manifest 长度与事务日志不匹配".to_string());
    }
    let mut hash = StableHash::new();
    hash.update(content);
    if hash.finish() != expected_hash {
        return Err("旧备份 manifest 摘要与事务日志不匹配".to_string());
    }
    Ok(())
}

pub(super) fn cleanup_backup_manifest_artifacts(repo: &Path) -> Result<(), String> {
    for entry in fs::read_dir(repo).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".skillmate-backup.json.skillmate-tmp-")
            || name.starts_with(".skillmate-backup.json.skillmate-old-")
        {
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                return Err(format!(
                    "备份 manifest 临时路径不是文件，已拒绝递归删除: {}",
                    entry.path().to_string_lossy()
                ));
            }
            remove_backup_path(&entry.path())?;
        }
    }
    Ok(())
}

pub(super) fn remove_backup_path(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => crate::app_core::remove_path(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum BackupSnapshotState {
    Prepared,
    Committed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BackupSnapshotJournal {
    pub(super) version: u32,
    #[serde(default)]
    pub(super) generation: u64,
    pub(super) state: BackupSnapshotState,
    #[serde(default)]
    pub(super) transaction_id: String,
    #[serde(default)]
    pub(super) baseline_branch: Option<String>,
    pub(super) baseline_head: Option<String>,
    #[serde(default)]
    pub(super) expected_tree: Option<String>,
    #[serde(default)]
    pub(super) expected_commit: Option<String>,
    #[serde(default)]
    pub(super) previous_snapshot_marker: Option<String>,
    #[serde(default)]
    pub(super) previous_manifest_len: Option<u64>,
    #[serde(default)]
    pub(super) previous_manifest_sha256: Option<String>,
    pub(super) had_snapshot: bool,
    pub(super) had_manifest: bool,
}

pub(super) struct BackupSnapshotTransaction {
    pub(super) repo: PathBuf,
    pub(super) transaction_root: PathBuf,
    pub(super) journal: BackupSnapshotJournal,
    pub(super) finished: bool,
}

impl BackupSnapshotTransaction {
    pub(super) fn stage_for_commit(&mut self) -> Result<(), String> {
        let expected_paths = scan_final_backup_paths(&self.repo)?;
        stage_backup_snapshot(&self.repo)?;
        let expected_tree = staged_git_tree(&self.repo)?;
        validate_backup_tree_scope(&self.repo, &self.journal, &expected_tree)?;
        validate_backup_tree_blobs(&self.repo, &expected_tree, &expected_paths)?;
        update_backup_snapshot_journal(&self.transaction_root, &mut self.journal, move |journal| {
            journal.expected_tree = Some(expected_tree)
        })
    }

    pub(super) fn commit_git_snapshot(mut self, message: &str) -> Result<(), String> {
        let result = (|| {
            self.stage_for_commit()?;
            commit_backup_snapshot(
                &self.repo,
                &self.transaction_root,
                message,
                &mut self.journal,
            )?;
            self.finish_commit()
        })();
        if let Err(error) = result {
            return Err(if self.finished {
                error
            } else {
                match self.rollback() {
                    Ok(()) => error,
                    Err(rollback_error) => {
                        format!("{}；备份回滚不完整: {}", error, rollback_error)
                    }
                }
            });
        }
        Ok(())
    }

    pub(super) fn rollback(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        if let Err(error) = ensure_backup_transaction_branch(&self.repo, &self.journal) {
            self.finished = true;
            return Err(error);
        }
        match prepared_backup_was_committed(&self.repo, &self.journal) {
            Ok(true) => {
                let result = self
                    .mark_committed()
                    .and_then(|_| remove_backup_path(&self.transaction_root));
                self.finished = true;
                return result;
            }
            Ok(false) => {}
            Err(error) => {
                self.finished = true;
                return Err(error);
            }
        }
        let result = restore_prepared_backup_transaction(
            &self.repo,
            &self.transaction_root,
            &mut self.journal,
        );
        self.finished = true;
        result
    }

    pub(super) fn mark_committed(&mut self) -> Result<(), String> {
        update_backup_snapshot_journal(&self.transaction_root, &mut self.journal, |journal| {
            journal.state = BackupSnapshotState::Committed
        })
    }

    pub(super) fn verify_commit_result(&self) -> Result<(), String> {
        ensure_backup_transaction_branch(&self.repo, &self.journal)?;
        let current_head = current_git_head(&self.repo)?;
        let expected_commit = self
            .journal
            .expected_commit
            .as_deref()
            .ok_or_else(|| "完成备份事务前缺少精确 commit，已保留事务现场".to_string())?;
        if current_head.as_deref() != Some(expected_commit) {
            return Err("完成备份事务前 Git HEAD 已变化，已保留事务现场".to_string());
        }
        validate_recovered_backup_commit(&self.repo, expected_commit, &self.journal)
    }

    pub(super) fn finish_commit(&mut self) -> Result<(), String> {
        self.verify_commit_result()?;
        self.mark_committed()
            .map_err(|error| format!("记录备份提交状态失败，已保留事务现场: {}", error))?;
        let result = remove_backup_path(&self.transaction_root)
            .map_err(|error| format!("清理已提交的备份事务目录失败: {}", error));
        self.finished = true;
        result
    }
}

impl Drop for BackupSnapshotTransaction {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.rollback();
        }
    }
}
