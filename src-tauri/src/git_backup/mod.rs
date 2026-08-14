use crate::app_core::expand_path;
use crate::skill_install_source::{
    sanitize_git_remote_url, validate_git_reference, validate_git_repo_locator,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

mod git_util;
mod journal;
mod repo;
mod sensitive;
mod snapshot;
mod staging;
mod verify;

#[cfg(test)]
mod tests;

use git_util::*;
use journal::*;
use repo::*;
use sensitive::*;
use snapshot::*;
use staging::*;
use verify::*;

pub(super) const BACKUP_ROOT_MARKER: &str = ".skillmate-backup-root";
pub(super) const MAX_BACKUP_FILES: usize = 20_000;
pub(super) const MAX_BACKUP_BYTES: u64 = 512 * 1024 * 1024;
pub(super) const MAX_BACKUP_DEPTH: usize = 32;
pub(super) const MAX_BACKUP_EXCLUSIONS: usize = 2_000;
pub(super) const MAX_SENSITIVE_SCAN_BYTES: u64 = 1024 * 1024;
pub(super) const MAX_BACKUP_MANIFEST_BYTES: u64 = 128 * 1024 * 1024;
pub(super) const BACKUP_SNAPSHOT_PATHS: [&str; 2] = ["assistants", "skillmate-backup.json"];
pub(super) const BACKUP_TRANSACTION_PREFIX: &str = "skillmate-backup-";
pub(super) const BACKUP_JOURNAL_FILE: &str = "journal.json";
pub(super) const BACKUP_OWNER_FILE: &str = "owner";
pub(super) const BACKUP_PREVIOUS_MANIFEST_FILE: &str = "previous-manifest";
pub(super) const BACKUP_JOURNAL_VERSION: u32 = 2;
pub(super) const MAX_BACKUP_JOURNAL_BYTES: u64 = 128 * 1024;
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GitBackup {
    pub enabled: bool,
    pub remote_url: String,
    pub repo_path: String,
    pub branch: String,
    pub last_sync: String,
}

impl Default for GitBackup {
    fn default() -> Self {
        Self {
            enabled: false,
            remote_url: String::new(),
            repo_path: String::new(),
            branch: "main".to_string(),
            last_sync: String::new(),
        }
    }
}

pub fn load(connection: &Connection) -> Result<GitBackup, String> {
    connection
        .query_row(
            "SELECT
                COALESCE(enabled, 0),
                COALESCE(remote_url, ''),
                COALESCE(repo_path, ''),
                COALESCE(branch, 'main'),
                COALESCE(last_sync, '')
             FROM git_backup WHERE id = 1",
            [],
            |row| {
                Ok(GitBackup {
                    enabled: row.get::<_, i32>(0)? != 0,
                    remote_url: row.get(1)?,
                    repo_path: row.get(2)?,
                    branch: row.get(3)?,
                    last_sync: row.get(4)?,
                })
            },
        )
        .optional()
        .map(|backup| backup.unwrap_or_default())
        .map_err(|error| error.to_string())
}

pub fn configure(
    connection: &Connection,
    repo_path: &str,
    remote_url: &str,
    branch: &str,
) -> Result<(), String> {
    let repo = expand_path(repo_path.trim());
    if repo.to_string_lossy().trim().is_empty() {
        return Err("仓库路径不能为空".to_string());
    }
    validate_backup_repo_location(connection, &repo)?;
    let branch = normalized_branch(branch);
    validate_git_reference(&branch)?;
    let remote_url = remote_url.trim();
    if !remote_url.is_empty() {
        validate_git_repo_locator(remote_url)?;
    }
    let safe_remote_url = sanitize_git_remote_url(remote_url);
    connection
        .execute(
            "INSERT OR REPLACE INTO git_backup (id, enabled, remote_url, repo_path, branch, last_sync)
             VALUES (1, 1, ?, ?, ?, COALESCE((SELECT last_sync FROM git_backup WHERE id = 1), ''))",
            params![
                safe_remote_url,
                repo.to_string_lossy().to_string(),
                branch
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn sync(connection: &Connection, message: &str) -> Result<String, String> {
    let backup = load(connection)?;
    if !backup.enabled {
        return Err("Git 备份未启用".to_string());
    }
    if backup.repo_path.trim().is_empty() {
        return Err("未配置仓库路径".to_string());
    }
    let repo = PathBuf::from(&backup.repo_path);
    validate_backup_repo_location(connection, &repo)?;
    validate_git_reference(&normalized_branch(&backup.branch))?;
    if !backup.remote_url.trim().is_empty() {
        validate_git_repo_locator(&backup.remote_url)?;
    }
    ensure_git_repo(&repo)?;
    recover_backup_transactions(&repo)?;
    ensure_git_identity(&repo)?;
    ensure_git_worktree_clean(&repo)?;
    checkout_git_branch(&repo, &backup.branch)?;
    ensure_git_worktree_clean(&repo)?;
    configure_git_remote(&repo, &backup.remote_url)?;
    let snapshot = snapshot_assistants(connection, &repo)?;

    let commit_message = if message.trim().is_empty() {
        "SkillMate backup"
    } else {
        message.trim()
    };
    snapshot.commit_git_snapshot(commit_message)?;

    let result = if backup.remote_url.trim().is_empty() {
        "本地快照同步成功".to_string()
    } else {
        run_git_checked(
            &repo,
            &["push", "-u", "origin", backup.branch.as_str()],
            Duration::from_secs(120),
        )?;
        "同步并推送成功".to_string()
    };
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    connection
        .execute(
            "UPDATE git_backup SET last_sync = ? WHERE id = 1",
            params![now],
        )
        .map_err(|error| error.to_string())?;
    Ok(result)
}
