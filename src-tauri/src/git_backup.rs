use crate::app_core::run_command_with_timeout;
use crate::app_core::{assistant_definitions, atomic_write, expand_path, generate_id, git_output};
use crate::managed_installation::list_managed_installations;
use crate::operation_plan::StableHash;
use crate::skill_install::{
    sanitize_git_remote_url, validate_git_reference, validate_git_repo_locator,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const BACKUP_ROOT_MARKER: &str = ".skillmate-backup-root";
const MAX_BACKUP_FILES: usize = 20_000;
const MAX_BACKUP_BYTES: u64 = 512 * 1024 * 1024;
const MAX_BACKUP_DEPTH: usize = 32;
const MAX_BACKUP_EXCLUSIONS: usize = 2_000;

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
            "SELECT enabled, remote_url, repo_path, branch, last_sync FROM git_backup WHERE id = 1",
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
    validate_backup_repo_location(&repo)?;
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
    validate_backup_repo_location(&repo)?;
    validate_git_reference(&normalized_branch(&backup.branch))?;
    if !backup.remote_url.trim().is_empty() {
        validate_git_repo_locator(&backup.remote_url)?;
    }
    ensure_git_repo(&repo)?;
    ensure_git_identity(&repo)?;
    ensure_git_worktree_clean(&repo)?;
    checkout_git_branch(&repo, &backup.branch)?;
    ensure_git_worktree_clean(&repo)?;
    configure_git_remote(&repo, &backup.remote_url)?;
    let mut snapshot = snapshot_assistants(connection, &repo)?;

    let commit_result = (|| {
        run_git_checked(&repo, &["add", "-A"], Duration::from_secs(30))?;
        let commit_message = if message.trim().is_empty() {
            "SkillMate backup"
        } else {
            message.trim()
        };
        let commit = run_git(
            &repo,
            &["commit", "-m", commit_message],
            Duration::from_secs(30),
        )?;
        if !commit.status.success() {
            let output = command_output(&commit);
            if !output.contains("nothing to commit") && !output.contains("nothing added to commit")
            {
                return Err(output);
            }
        }
        Ok(())
    })();
    if let Err(error) = commit_result {
        let mut rollback_errors = Vec::new();
        if let Err(rollback_error) = snapshot.rollback() {
            rollback_errors.push(rollback_error);
        }
        if let Err(index_error) = restore_git_index(&repo) {
            rollback_errors.push(index_error);
        }
        return Err(if rollback_errors.is_empty() {
            error
        } else {
            format!("{}；备份回滚不完整: {}", error, rollback_errors.join("；"))
        });
    }
    snapshot.commit()?;

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

fn normalized_branch(branch: &str) -> String {
    if branch.trim().is_empty() {
        "main".to_string()
    } else {
        branch.trim().to_string()
    }
}

fn validate_backup_repo_location(repo: &Path) -> Result<(), String> {
    let repo = canonicalize_for_comparison(repo)?;
    for assistant in assistant_definitions() {
        for skill_root in assistant.global_discovery_roots() {
            let skill_root = canonicalize_for_comparison(&skill_root)?;
            if paths_overlap(&repo, &skill_root) {
                return Err(format!(
                    "备份仓库不能与 {} 的 Skills 目录互相包含",
                    assistant.name
                ));
            }
        }
    }
    Ok(())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn canonicalize_for_comparison(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    if absolute.exists() {
        return absolute.canonicalize().map_err(|error| error.to_string());
    }
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| "无法解析备份仓库路径".to_string())?;
        suffix.push(name.to_os_string());
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "无法解析备份仓库父目录".to_string())?;
    }
    let mut resolved = ancestor.canonicalize().map_err(|error| error.to_string())?;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn ensure_git_repo(repo: &Path) -> Result<(), String> {
    fs::create_dir_all(repo).map_err(|error| error.to_string())?;
    if repo.join(".git").exists() {
        return Ok(());
    }
    run_git_checked(repo, &["init"], Duration::from_secs(10))
}

fn ensure_git_identity(repo: &Path) -> Result<(), String> {
    if git_output(repo, &["config", "--get", "user.name"])
        .unwrap_or_default()
        .is_empty()
    {
        run_git_checked(
            repo,
            &["config", "user.name", "SkillMate"],
            Duration::from_secs(5),
        )?;
    }
    if git_output(repo, &["config", "--get", "user.email"])
        .unwrap_or_default()
        .is_empty()
    {
        run_git_checked(
            repo,
            &["config", "user.email", "skillmate@local"],
            Duration::from_secs(5),
        )?;
    }
    Ok(())
}

fn checkout_git_branch(repo: &Path, branch: &str) -> Result<(), String> {
    let branch = normalized_branch(branch);
    if git_output(repo, &["branch", "--show-current"]).unwrap_or_default() == branch {
        return Ok(());
    }
    let branch_ref = format!("refs/heads/{}", branch);
    let branch_exists = run_git(
        repo,
        &["show-ref", "--verify", "--quiet", &branch_ref],
        Duration::from_secs(5),
    )
    .map(|output| output.status.success())
    .unwrap_or(false);
    if branch_exists {
        run_git_checked(repo, &["switch", &branch], Duration::from_secs(10))
    } else {
        run_git_checked(repo, &["switch", "-c", &branch], Duration::from_secs(10))
    }
}

fn ensure_git_worktree_clean(repo: &Path) -> Result<(), String> {
    let status =
        git_output(repo, &["status", "--porcelain", "--untracked-files=all"]).unwrap_or_default();
    if status.is_empty() {
        Ok(())
    } else {
        Err("备份仓库存在未提交修改，请先提交或清理后再同步".to_string())
    }
}

fn configure_git_remote(repo: &Path, remote_url: &str) -> Result<(), String> {
    if remote_url.trim().is_empty() {
        return Ok(());
    }
    let current = git_output(repo, &["remote", "get-url", "origin"]).unwrap_or_default();
    if current == remote_url {
        return Ok(());
    }
    if current.is_empty() {
        run_git_checked(
            repo,
            &["remote", "add", "origin", remote_url],
            Duration::from_secs(10),
        )
    } else {
        run_git_checked(
            repo,
            &["remote", "set-url", "origin", remote_url],
            Duration::from_secs(10),
        )
    }
}

fn restore_git_index(repo: &Path) -> Result<(), String> {
    let reset = run_git(repo, &["reset", "--mixed"], Duration::from_secs(10))?;
    if reset.status.success() {
        return Ok(());
    }
    let clear = run_git(
        repo,
        &["rm", "--cached", "-r", "--ignore-unmatch", "."],
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

fn validate_existing_snapshot_root(repo: &Path) -> Result<(), String> {
    let snapshot_root = repo.join("assistants");
    let marker = snapshot_root.join(BACKUP_ROOT_MARKER);
    if snapshot_root.exists() {
        if !snapshot_root.is_dir() {
            return Err("备份仓库中的 assistants 路径已存在但不是目录".to_string());
        }
        if !marker.exists() {
            return Err(
                "备份仓库中的 assistants 目录不是 SkillMate 管理目录，已拒绝覆盖".to_string(),
            );
        }
    }
    Ok(())
}

#[derive(Default)]
struct BackupCopyBudget {
    files: usize,
    bytes: u64,
}

#[derive(Debug, Default)]
struct BackupSource {
    path: PathBuf,
    assistants: BTreeSet<String>,
    scopes: BTreeSet<String>,
    projects: BTreeSet<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupCopyReport {
    copied_files: usize,
    copied_bytes: u64,
    top_level_entries: usize,
    exclusions: Vec<BackupExclusion>,
    exclusions_truncated: bool,
}

#[derive(Debug, Serialize)]
struct BackupExclusion {
    path: String,
    reason: &'static str,
}

struct TemporaryBackupDirectory {
    path: PathBuf,
    keep: bool,
}

impl Drop for TemporaryBackupDirectory {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct BackupSnapshotTransaction {
    snapshot_root: PathBuf,
    backup_root: Option<PathBuf>,
    transaction_root: PathBuf,
    manifest_path: PathBuf,
    previous_manifest: Option<Vec<u8>>,
    finished: bool,
}

impl BackupSnapshotTransaction {
    fn rollback(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        let mut errors = Vec::new();
        if self.snapshot_root.exists() {
            if let Err(error) = fs::remove_dir_all(&self.snapshot_root) {
                errors.push(format!("移除新快照失败: {}", error));
            }
        }
        if let Some(backup_root) = &self.backup_root {
            if backup_root.exists() {
                if let Err(error) = fs::rename(backup_root, &self.snapshot_root) {
                    errors.push(format!("恢复旧快照失败: {}", error));
                }
            }
        }
        let manifest_result = match &self.previous_manifest {
            Some(content) => atomic_write(&self.manifest_path, content),
            None => match fs::remove_file(&self.manifest_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.to_string()),
            },
        };
        if let Err(error) = manifest_result {
            errors.push(format!("恢复备份 manifest 失败: {}", error));
        }
        if let Err(error) = fs::remove_dir_all(&self.transaction_root) {
            if error.kind() != std::io::ErrorKind::NotFound {
                errors.push(format!("清理备份临时目录失败: {}", error));
            }
        }
        self.finished = true;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }

    fn commit(mut self) -> Result<(), String> {
        if self.transaction_root.exists() {
            fs::remove_dir_all(&self.transaction_root).map_err(|error| error.to_string())?;
        }
        self.finished = true;
        Ok(())
    }
}

impl Drop for BackupSnapshotTransaction {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.rollback();
        }
    }
}

fn snapshot_assistants(
    connection: &Connection,
    repo: &Path,
) -> Result<BackupSnapshotTransaction, String> {
    validate_existing_snapshot_root(repo)?;
    let snapshot_root = repo.join("assistants");
    let transaction_root = repo
        .join(".git")
        .join(format!("skillmate-backup-{}", generate_id()));
    let staging_root = transaction_root.join("assistants");
    let backup_root = transaction_root.join("previous-assistants");
    fs::create_dir_all(&staging_root).map_err(|error| error.to_string())?;
    let mut temporary_directory = TemporaryBackupDirectory {
        path: transaction_root.clone(),
        keep: false,
    };
    fs::write(
        staging_root.join(BACKUP_ROOT_MARKER),
        "Managed by SkillMate. This directory may be replaced during backup sync.\n",
    )
    .map_err(|error| error.to_string())?;

    let sources = collect_backup_sources(connection)?;
    let mut manifest = Vec::new();
    let mut budget = BackupCopyBudget::default();
    for source in sources.values() {
        let root_id = backup_root_id(&source.path);
        let target_root = staging_root.join("roots").join(&root_id);
        let mut report = BackupCopyReport::default();
        if source.path.exists() {
            copy_backup_tree(
                &source.path,
                &target_root,
                &source.path,
                0,
                &mut budget,
                &mut report,
            )?;
        }
        manifest.push(serde_json::json!({
            "sourcePath": display_backup_path(&source.path),
            "snapshotPath": format!("assistants/roots/{}", root_id),
            "exists": source.path.exists(),
            "assistants": &source.assistants,
            "scopes": &source.scopes,
            "projects": &source.projects,
            "report": report,
        }));
    }
    let payload = serde_json::json!({
        "version": 2,
        "kind": "skill-content-snapshot",
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "roots": manifest,
        "limitations": [
            "不包含 SkillMate 数据库、标签、场景或 Profile",
            "不跟随或复制软连接",
            "不复制凭据、密钥、运行时缓存或受管 sidecar"
        ],
    });
    let manifest_path = repo.join("skillmate-backup.json");
    let previous_manifest = match fs::read(&manifest_path) {
        Ok(content) => Some(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let manifest_payload =
        serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;

    let had_snapshot = snapshot_root.exists();
    if had_snapshot {
        fs::rename(&snapshot_root, &backup_root).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(&staging_root, &snapshot_root) {
        if had_snapshot && backup_root.exists() {
            let _ = fs::rename(&backup_root, &snapshot_root);
        }
        return Err(format!("无法启用新备份快照: {}", error));
    }
    if let Err(error) = atomic_write(&manifest_path, manifest_payload.as_bytes()) {
        let _ = fs::remove_dir_all(&snapshot_root);
        if had_snapshot && backup_root.exists() {
            let _ = fs::rename(&backup_root, &snapshot_root);
        }
        if let Some(content) = &previous_manifest {
            let _ = atomic_write(&manifest_path, content);
        } else {
            let _ = fs::remove_file(&manifest_path);
        }
        return Err(error);
    }
    temporary_directory.keep = true;
    Ok(BackupSnapshotTransaction {
        snapshot_root,
        backup_root: had_snapshot.then_some(backup_root),
        transaction_root,
        manifest_path,
        previous_manifest,
        finished: false,
    })
}

fn copy_backup_tree(
    source: &Path,
    target: &Path,
    source_root: &Path,
    depth: usize,
    budget: &mut BackupCopyBudget,
    report: &mut BackupCopyReport,
) -> Result<(), String> {
    if depth > MAX_BACKUP_DEPTH {
        return Err(format!("备份目录层级超过 {} 层", MAX_BACKUP_DEPTH));
    }
    fs::create_dir_all(target).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        let source_path = entry.path();
        if let Some(reason) = backup_exclusion_reason(&name_text) {
            record_backup_exclusion(report, source_root, &source_path, reason);
            continue;
        }
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            record_backup_exclusion(report, source_root, &source_path, "symlink");
            continue;
        }
        let target_path = target.join(name);
        if depth == 0 {
            report.top_level_entries += 1;
        }
        if metadata.is_dir() {
            copy_backup_tree(
                &source_path,
                &target_path,
                source_root,
                depth + 1,
                budget,
                report,
            )?;
        } else if metadata.is_file() {
            budget.files += 1;
            budget.bytes = budget.bytes.saturating_add(metadata.len());
            if budget.files > MAX_BACKUP_FILES || budget.bytes > MAX_BACKUP_BYTES {
                return Err(format!(
                    "备份超过限制（最多 {} 个文件、{} MB）",
                    MAX_BACKUP_FILES,
                    MAX_BACKUP_BYTES / 1024 / 1024
                ));
            }
            fs::copy(&source_path, &target_path).map_err(|error| error.to_string())?;
            report.copied_files += 1;
            report.copied_bytes = report.copied_bytes.saturating_add(metadata.len());
        } else {
            record_backup_exclusion(report, source_root, &source_path, "special_file");
        }
    }
    Ok(())
}

fn collect_backup_sources(
    connection: &Connection,
) -> Result<BTreeMap<PathBuf, BackupSource>, String> {
    let mut sources = BTreeMap::new();
    for assistant in assistant_definitions() {
        for root in assistant.global_discovery_roots() {
            add_backup_source(&mut sources, root, assistant.name, "global", None)?;
        }
    }
    for installation in list_managed_installations(connection)? {
        let Some(root) = installation.path.parent() else {
            continue;
        };
        add_backup_source(
            &mut sources,
            root.to_path_buf(),
            &installation.skill.assistant,
            installation.skill.scope.as_deref().unwrap_or("global"),
            installation.skill.project_path.as_deref(),
        )?;
    }
    Ok(sources)
}

fn add_backup_source(
    sources: &mut BTreeMap<PathBuf, BackupSource>,
    path: PathBuf,
    assistant: &str,
    scope: &str,
    project: Option<&str>,
) -> Result<(), String> {
    let identity = canonicalize_for_comparison(&path)?;
    let source = sources.entry(identity).or_insert_with(|| BackupSource {
        path,
        ..BackupSource::default()
    });
    source.assistants.insert(assistant.to_string());
    source.scopes.insert(scope.to_string());
    if let Some(project) = project.filter(|value| !value.trim().is_empty()) {
        source
            .projects
            .insert(display_backup_path(Path::new(project)));
    }
    Ok(())
}

fn backup_root_id(path: &Path) -> String {
    let mut hash = StableHash::new();
    hash.update(path.to_string_lossy().as_bytes());
    let digest = hash.finish();
    format!("root-{}", &digest[..12])
}

fn display_backup_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(home) {
            return Path::new("~").join(relative).to_string_lossy().to_string();
        }
    }
    path.to_string_lossy().to_string()
}

fn record_backup_exclusion(
    report: &mut BackupCopyReport,
    source_root: &Path,
    path: &Path,
    reason: &'static str,
) {
    if report.exclusions.len() >= MAX_BACKUP_EXCLUSIONS {
        report.exclusions_truncated = true;
        return;
    }
    let relative = path.strip_prefix(source_root).unwrap_or(path);
    report.exclusions.push(BackupExclusion {
        path: relative.to_string_lossy().to_string(),
        reason,
    });
}

fn backup_exclusion_reason(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower == ".skillmate-state.json" {
        return Some("managed_state");
    }
    if matches!(
        lower.as_str(),
        ".git"
            | ".hg"
            | ".svn"
            | ".ds_store"
            | "node_modules"
            | "target"
            | "__pycache__"
            | ".venv"
            | "venv"
    ) {
        return Some("runtime");
    }
    if lower == ".env"
        || lower.starts_with(".env.")
        || matches!(
            lower.as_str(),
            ".npmrc"
                | ".pypirc"
                | ".netrc"
                | ".git-credentials"
                | "credentials"
                | "credentials.json"
                | "secrets"
                | "id_rsa"
                | "id_ed25519"
        )
        || lower.contains("credential")
        || lower.contains("secret")
        || [".pem", ".key", ".p12", ".pfx"]
            .iter()
            .any(|extension| lower.ends_with(extension))
    {
        return Some("sensitive");
    }
    None
}

fn run_git(repo: &Path, args: &[&str], timeout: Duration) -> Result<std::process::Output, String> {
    let mut safe_args = vec![
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "protocol.file.allow=user",
        "-c",
        "submodule.recurse=false",
    ];
    safe_args.extend_from_slice(args);
    run_command_with_timeout(
        "git",
        &safe_args,
        Some(repo),
        timeout,
        &[
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GCM_INTERACTIVE", "Never"),
            ("GIT_LFS_SKIP_SMUDGE", "1"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
        ],
    )
}

fn run_git_checked(repo: &Path, args: &[&str], timeout: Duration) -> Result<(), String> {
    let output = run_git(repo, args, timeout)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_output(&output))
    }
}

fn command_output(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match (stdout.is_empty(), stderr.is_empty()) {
        (false, false) => format!("{}\n{}", stdout, stderr),
        (false, true) => stdout,
        (true, false) => stderr,
        (true, true) => "Git 命令执行失败".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_core::now_ms;
    use std::process::Command;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("skillmate-backup-test-{}-{}", name, now_ms()))
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
            "备份仓库中的 assistants 目录不是 SkillMate 管理目录，已拒绝覆盖"
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
        fs::create_dir_all(&snapshot_root).unwrap();
        fs::create_dir_all(&backup_root).unwrap();
        fs::write(snapshot_root.join("new"), "new").unwrap();
        fs::write(backup_root.join("old"), "old").unwrap();
        fs::write(&manifest_path, "new manifest").unwrap();
        let mut transaction = BackupSnapshotTransaction {
            snapshot_root: snapshot_root.clone(),
            backup_root: Some(backup_root),
            transaction_root,
            manifest_path: manifest_path.clone(),
            previous_manifest: Some(b"old manifest".to_vec()),
            finished: false,
        };

        transaction.rollback().unwrap();

        assert!(snapshot_root.join("old").exists());
        assert!(!snapshot_root.join("new").exists());
        assert_eq!(fs::read_to_string(manifest_path).unwrap(), "old manifest");
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn backup_filter_excludes_runtime_and_sensitive_metadata() {
        for name in [
            ".skillmate-state.json",
            ".env",
            "credentials.json",
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
    }

    #[test]
    fn backup_copy_keeps_hidden_assets_and_records_exclusions() {
        let base = test_dir("copy-policy");
        let source = base.join("source");
        let target = base.join("target");
        fs::create_dir_all(source.join(".github")).unwrap();
        fs::write(source.join("SKILL.md"), "skill").unwrap();
        fs::write(source.join(".github/config.yml"), "visible").unwrap();
        fs::write(source.join(".env"), "SECRET=value").unwrap();
        fs::write(source.join("private.key"), "key").unwrap();
        let mut budget = BackupCopyBudget::default();
        let mut report = BackupCopyReport::default();

        copy_backup_tree(&source, &target, &source, 0, &mut budget, &mut report).unwrap();

        assert!(target.join("SKILL.md").exists());
        assert!(target.join(".github/config.yml").exists());
        assert!(!target.join(".env").exists());
        assert!(!target.join("private.key").exists());
        assert_eq!(report.copied_files, 2);
        assert!(report
            .exclusions
            .iter()
            .any(|entry| entry.path == ".env" && entry.reason == "sensitive"));
        assert!(report
            .exclusions
            .iter()
            .any(|entry| entry.path == "private.key" && entry.reason == "sensitive"));
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

    #[test]
    fn overlap_check_is_bidirectional() {
        let root = PathBuf::from("/tmp/skillmate-overlap");
        let nested = root.join("nested/repo");

        assert!(paths_overlap(&root, &nested));
        assert!(paths_overlap(&nested, &root));
        assert!(!paths_overlap(&root, Path::new("/tmp/other")));
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
}
