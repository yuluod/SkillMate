use crate::app_core::{
    assistant_definitions, atomic_write, expand_path, generate_id, run_command_with_options,
    CommandOptions,
};
use crate::managed_installation::{list_managed_installations, list_managed_roots};
use crate::operation_plan::StableHash;
use crate::skill_install_source::{
    sanitize_git_remote_url, validate_git_reference, validate_git_repo_locator,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const BACKUP_ROOT_MARKER: &str = ".skillmate-backup-root";
const MAX_BACKUP_FILES: usize = 20_000;
const MAX_BACKUP_BYTES: u64 = 512 * 1024 * 1024;
const MAX_BACKUP_DEPTH: usize = 32;
const MAX_BACKUP_EXCLUSIONS: usize = 2_000;
const MAX_SENSITIVE_SCAN_BYTES: u64 = 1024 * 1024;
const MAX_BACKUP_MANIFEST_BYTES: u64 = 128 * 1024 * 1024;
const BACKUP_SNAPSHOT_PATHS: [&str; 2] = ["assistants", "skillmate-backup.json"];
const BACKUP_TRANSACTION_PREFIX: &str = "skillmate-backup-";
const BACKUP_JOURNAL_FILE: &str = "journal.json";
const BACKUP_OWNER_FILE: &str = "owner";
const BACKUP_PREVIOUS_MANIFEST_FILE: &str = "previous-manifest";
const BACKUP_JOURNAL_VERSION: u32 = 2;
const MAX_BACKUP_JOURNAL_BYTES: u64 = 128 * 1024;

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

fn normalized_branch(branch: &str) -> String {
    if branch.trim().is_empty() {
        "main".to_string()
    } else {
        branch.trim().to_string()
    }
}

fn validate_backup_repo_location(connection: &Connection, repo: &Path) -> Result<(), String> {
    let repo = canonicalize_for_comparison(repo)?;
    let mut protected_roots = Vec::new();
    for assistant in assistant_definitions() {
        for skill_root in assistant.global_discovery_roots() {
            protected_roots.push((skill_root, assistant.name.to_string()));
        }
    }
    protected_roots.extend(
        list_managed_roots(connection)?
            .into_iter()
            .map(|root| (root.path, "项目级受管".to_string())),
    );
    for (skill_root, label) in protected_roots {
        let skill_root = canonicalize_for_comparison(&skill_root)?;
        if paths_overlap(&repo, &skill_root) {
            return Err(format!("备份仓库不能与 {} Skills 目录互相包含", label));
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
    match fs::symlink_metadata(repo.join(".git")) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            run_git_checked(repo, &["init"], Duration::from_secs(10))?;
        }
        Err(error) => return Err(error.to_string()),
    }
    validated_backup_git_dir(repo)?;
    let top_level = run_git(
        repo,
        &["rev-parse", "--show-toplevel"],
        Duration::from_secs(10),
    )?;
    if !top_level.status.success() {
        return Err(format!(
            "备份路径不是有效的 Git 工作区: {}",
            command_output(&top_level)
        ));
    }
    let actual = PathBuf::from(String::from_utf8_lossy(&top_level.stdout).trim());
    let expected = repo.canonicalize().map_err(|error| error.to_string())?;
    let actual = actual.canonicalize().map_err(|error| error.to_string())?;
    if actual != expected {
        return Err("备份路径必须是独立 Git 工作区的根目录".to_string());
    }
    Ok(())
}

fn validated_backup_git_dir(repo: &Path) -> Result<PathBuf, String> {
    let git_dir = repo.join(".git");
    let metadata = fs::symlink_metadata(&git_dir)
        .map_err(|error| format!("读取备份仓库 .git 失败: {}", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(
            "备份仓库的 .git 必须是仓库内的普通目录，不支持软连接或外部 gitdir".to_string(),
        );
    }
    Ok(git_dir)
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

fn git_output(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = run_git(repo, args, Duration::from_secs(10))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(command_output(&output))
    }
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
    let status = run_git(
        repo,
        &["status", "--porcelain", "--untracked-files=all"],
        Duration::from_secs(10),
    )?;
    if !status.status.success() {
        return Err(format!(
            "检查 Git 工作区状态失败: {}",
            command_output(&status)
        ));
    }
    if status.stdout.is_empty() {
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

fn stage_backup_snapshot(repo: &Path) -> Result<(), String> {
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

fn validate_staged_backup_paths(repo: &Path) -> Result<(), String> {
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

fn is_backup_snapshot_path(path: &[u8]) -> bool {
    path == BACKUP_SNAPSHOT_PATHS[0].as_bytes()
        || path.starts_with(b"assistants/")
        || path == BACKUP_SNAPSHOT_PATHS[1].as_bytes()
}

fn commit_backup_snapshot(
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

fn unstage_backup_snapshot(repo: &Path) -> Result<(), String> {
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

fn update_backup_snapshot_journal(
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

fn current_git_branch(repo: &Path) -> Result<String, String> {
    let output = run_git(repo, &["branch", "--show-current"], Duration::from_secs(10))?;
    if !output.status.success() {
        return Err(format!("读取 Git 分支失败: {}", command_output(&output)));
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        Err("备份事务不支持 detached HEAD，请先切回配置分支".to_string())
    } else {
        Ok(branch)
    }
}

fn current_git_head(repo: &Path) -> Result<Option<String>, String> {
    let output = run_git(
        repo,
        &["rev-parse", "--verify", "--quiet", "HEAD"],
        Duration::from_secs(10),
    )?;
    if output.status.success() {
        let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return if head.is_empty() {
            Err("Git HEAD 为空".to_string())
        } else {
            Ok(Some(head))
        };
    }
    if output.status.code() == Some(1) && output.stdout.is_empty() && output.stderr.is_empty() {
        Ok(None)
    } else {
        Err(format!("读取 Git HEAD 失败: {}", command_output(&output)))
    }
}

fn git_tree(repo: &Path, revision: &str) -> Result<String, String> {
    let output = run_git(
        repo,
        &["rev-parse", &format!("{}^{{tree}}", revision)],
        Duration::from_secs(10),
    )?;
    if !output.status.success() {
        return Err(format!("读取 Git tree 失败: {}", command_output(&output)));
    }
    let tree = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if tree.is_empty() {
        Err("Git tree 为空".to_string())
    } else {
        Ok(tree)
    }
}

fn staged_git_tree(repo: &Path) -> Result<String, String> {
    let output = run_git(repo, &["write-tree"], Duration::from_secs(10))?;
    if !output.status.success() {
        return Err(format!(
            "记录待提交备份内容失败: {}",
            command_output(&output)
        ));
    }
    let tree = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if tree.is_empty() {
        Err("待提交备份的 Git tree 为空".to_string())
    } else {
        Ok(tree)
    }
}

fn scan_final_backup_paths(repo: &Path) -> Result<BTreeSet<Vec<u8>>, String> {
    let mut paths = BTreeSet::new();
    let mut files = 0usize;
    let mut bytes = 0u64;
    for relative in BACKUP_SNAPSHOT_PATHS {
        scan_final_backup_path(
            &repo.join(relative),
            Path::new(relative),
            0,
            &mut files,
            &mut bytes,
            &mut paths,
        )?;
    }
    Ok(paths)
}

fn scan_final_backup_path(
    path: &Path,
    relative: &Path,
    depth: usize,
    files: &mut usize,
    bytes: &mut u64,
    paths: &mut BTreeSet<Vec<u8>>,
) -> Result<(), String> {
    if depth > MAX_BACKUP_DEPTH + 3 {
        return Err(format!("最终备份目录层级超过 {} 层", MAX_BACKUP_DEPTH));
    }
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if let Some(reason) = backup_exclusion_reason(name) {
            return Err(format!(
                "最终备份内容出现禁止路径（{}），已拒绝提交: {}",
                reason,
                path.to_string_lossy()
            ));
        }
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("读取最终备份内容失败 {}: {}", path.to_string_lossy(), error))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "最终备份内容包含软连接，已拒绝提交: {}",
            path.to_string_lossy()
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            scan_final_backup_path(
                &entry.path(),
                &relative.join(entry.file_name()),
                depth + 1,
                files,
                bytes,
                paths,
            )?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(format!(
            "最终备份内容包含特殊文件，已拒绝提交: {}",
            path.to_string_lossy()
        ));
    }

    let scan_limit = if relative == Path::new(BACKUP_SNAPSHOT_PATHS[1]) {
        MAX_BACKUP_MANIFEST_BYTES
    } else {
        MAX_SENSITIVE_SCAN_BYTES
    };
    let scanned = read_scanned_backup_file_with_limit(path, &metadata, scan_limit)?;
    match scanned.classification {
        SensitiveScan::Safe => {}
        SensitiveScan::Sensitive => {
            return Err(format!(
                "最终 Git tree 包含疑似敏感内容，已拒绝提交: {}",
                path.to_string_lossy()
            ));
        }
        SensitiveScan::Unscannable(reason) => {
            return Err(format!(
                "最终 Git tree 包含无法安全扫描的文件（{}），已拒绝提交: {}",
                unscannable_description(reason),
                path.to_string_lossy()
            ));
        }
    }
    *files = files.saturating_add(1);
    *bytes = bytes.saturating_add(scanned.bytes.len() as u64);
    if *files > MAX_BACKUP_FILES + BACKUP_SNAPSHOT_PATHS.len()
        || *bytes > MAX_BACKUP_BYTES + MAX_BACKUP_MANIFEST_BYTES + MAX_SENSITIVE_SCAN_BYTES
    {
        return Err("最终 Git tree 超过备份安全上限".to_string());
    }
    let key = git_path_bytes(relative)?;
    if !paths.insert(key) {
        return Err("最终 Git tree 包含重复路径".to_string());
    }
    Ok(())
}

fn git_path_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(format!("Git 相对路径无效: {}", path.to_string_lossy()));
        };
        if !result.is_empty() {
            result.push(b'/');
        }
        result.extend(os_string_bytes(part)?);
    }
    Ok(result)
}

#[cfg(unix)]
fn os_string_bytes(value: &std::ffi::OsStr) -> Result<&[u8], String> {
    use std::os::unix::ffi::OsStrExt;
    Ok(value.as_bytes())
}

#[cfg(not(unix))]
fn os_string_bytes(value: &std::ffi::OsStr) -> Result<&[u8], String> {
    value
        .to_str()
        .map(str::as_bytes)
        .ok_or_else(|| "Git 路径不是有效的 Unicode".to_string())
}

fn validate_backup_tree_scope(
    repo: &Path,
    journal: &BackupSnapshotJournal,
    expected_tree: &str,
) -> Result<(), String> {
    let output = if let Some(head) = journal.baseline_head.as_deref() {
        run_git(
            repo,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "--no-renames",
                "-r",
                "-z",
                head,
                expected_tree,
            ],
            Duration::from_secs(10),
        )?
    } else {
        run_git(
            repo,
            &["ls-tree", "-r", "-z", "--name-only", expected_tree],
            Duration::from_secs(10),
        )?
    };
    if !output.status.success() {
        return Err(format!(
            "检查备份 Git tree 范围失败: {}",
            command_output(&output)
        ));
    }
    if let Some(path) = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .find(|path| !is_backup_snapshot_path(path))
    {
        return Err(format!(
            "Git tree 包含非 SkillMate 备份变更: {}",
            String::from_utf8_lossy(path)
        ));
    }
    Ok(())
}

fn validate_backup_tree_blobs(
    repo: &Path,
    expected_tree: &str,
    expected: &BTreeSet<Vec<u8>>,
) -> Result<(), String> {
    let output = run_git(
        repo,
        &[
            "ls-tree",
            "-r",
            "-z",
            "--full-tree",
            expected_tree,
            "--",
            BACKUP_SNAPSHOT_PATHS[0],
            BACKUP_SNAPSHOT_PATHS[1],
        ],
        Duration::from_secs(10),
    )?;
    if !output.status.success() {
        return Err(format!(
            "读取最终备份 Git tree 失败: {}",
            command_output(&output)
        ));
    }

    let mut actual = BTreeMap::new();
    for entry in output.stdout.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let tab = entry
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| "最终备份 Git tree 条目格式无效".to_string())?;
        let metadata = std::str::from_utf8(&entry[..tab])
            .map_err(|_| "最终备份 Git tree 元数据不是 UTF-8".to_string())?;
        let mut fields = metadata.split_whitespace();
        let mode = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        let oid = fields.next().unwrap_or_default();
        if fields.next().is_some()
            || kind != "blob"
            || !matches!(mode, "100644" | "100755")
            || oid.is_empty()
        {
            return Err(format!(
                "最终备份 Git tree 包含不安全条目: {}",
                String::from_utf8_lossy(&entry[tab + 1..])
            ));
        }
        if actual
            .insert(entry[tab + 1..].to_vec(), oid.to_string())
            .is_some()
        {
            return Err("最终备份 Git tree 包含重复路径".to_string());
        }
    }

    if actual.len() != expected.len() {
        return Err("最终 Git tree 与已扫描备份文件数量不一致".to_string());
    }
    for path in expected {
        if !actual.contains_key(path) {
            return Err(format!(
                "最终 Git tree 缺少已扫描文件: {}",
                String::from_utf8_lossy(path)
            ));
        }
    }
    scan_immutable_git_blobs(repo, &actual)
}

#[derive(Debug)]
struct ImmutableGitBlob {
    oid: String,
    size: u64,
    sample_path: Vec<u8>,
}

fn scan_immutable_git_blobs(
    repo: &Path,
    entries: &BTreeMap<Vec<u8>, String>,
) -> Result<(), String> {
    let mut paths_by_oid = BTreeMap::<String, Vec<&[u8]>>::new();
    for (path, oid) in entries {
        paths_by_oid.entry(oid.clone()).or_default().push(path);
    }
    let object_ids = paths_by_oid.keys().cloned().collect::<Vec<_>>();
    let input = object_ids.join("\n") + "\n";
    let output = run_git_with_input(
        repo,
        &[
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ],
        input.as_bytes(),
        Duration::from_secs(30),
    )?;
    if !output.status.success() {
        return Err(format!(
            "读取最终 Git blob 元数据失败: {}",
            command_output(&output)
        ));
    }
    let lines = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.len() != object_ids.len() {
        return Err("最终 Git blob 元数据数量不一致".to_string());
    }

    let mut blobs = Vec::with_capacity(object_ids.len());
    let mut total_bytes = 0u64;
    let mut size_by_oid = BTreeMap::new();
    for (expected_oid, line) in object_ids.iter().zip(lines) {
        let line =
            std::str::from_utf8(line).map_err(|_| "最终 Git blob 元数据不是 UTF-8".to_string())?;
        let mut fields = line.split_whitespace();
        let oid = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        let size = fields
            .next()
            .unwrap_or_default()
            .parse::<u64>()
            .map_err(|_| "最终 Git blob 大小无效".to_string())?;
        if fields.next().is_some() || oid != expected_oid || kind != "blob" {
            return Err("最终 Git blob 元数据与 tree 不一致".to_string());
        }
        let paths = paths_by_oid
            .get(expected_oid)
            .ok_or_else(|| "最终 Git blob 缺少路径".to_string())?;
        let max_size = paths
            .iter()
            .map(|path| {
                if *path == BACKUP_SNAPSHOT_PATHS[1].as_bytes() {
                    MAX_BACKUP_MANIFEST_BYTES
                } else {
                    MAX_SENSITIVE_SCAN_BYTES
                }
            })
            .min()
            .unwrap_or(MAX_SENSITIVE_SCAN_BYTES);
        if size > max_size {
            return Err(format!(
                "最终 Git tree 文件超过安全扫描上限: {}",
                String::from_utf8_lossy(paths[0])
            ));
        }
        size_by_oid.insert(expected_oid.clone(), size);
        blobs.push(ImmutableGitBlob {
            oid: expected_oid.clone(),
            size,
            sample_path: paths[0].to_vec(),
        });
    }
    for oid in entries.values() {
        total_bytes = total_bytes.saturating_add(*size_by_oid.get(oid).unwrap_or(&u64::MAX));
    }
    if total_bytes > MAX_BACKUP_BYTES + MAX_BACKUP_MANIFEST_BYTES + MAX_SENSITIVE_SCAN_BYTES {
        return Err("最终 Git tree 超过备份安全上限".to_string());
    }

    for chunk in git_blob_chunks(&blobs) {
        scan_immutable_git_blob_chunk(repo, chunk)?;
    }
    Ok(())
}

fn git_blob_chunks(blobs: &[ImmutableGitBlob]) -> Vec<&[ImmutableGitBlob]> {
    const MAX_BATCH_BYTES: u64 = 16 * 1024 * 1024;
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < blobs.len() {
        let mut end = start;
        let mut bytes = 0u64;
        while end < blobs.len() && (end == start || bytes + blobs[end].size <= MAX_BATCH_BYTES) {
            bytes = bytes.saturating_add(blobs[end].size);
            end += 1;
        }
        chunks.push(&blobs[start..end]);
        start = end;
    }
    chunks
}

fn scan_immutable_git_blob_chunk(repo: &Path, blobs: &[ImmutableGitBlob]) -> Result<(), String> {
    let input = blobs
        .iter()
        .map(|blob| blob.oid.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let output = run_git_with_input(
        repo,
        &["cat-file", "--batch"],
        input.as_bytes(),
        Duration::from_secs(120),
    )?;
    if !output.status.success() {
        return Err(format!(
            "读取最终 Git blob 失败: {}",
            command_output(&output)
        ));
    }

    let mut cursor = 0usize;
    for blob in blobs {
        let header_end = output.stdout[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| cursor + offset)
            .ok_or_else(|| "最终 Git blob 响应缺少头部".to_string())?;
        let header = std::str::from_utf8(&output.stdout[cursor..header_end])
            .map_err(|_| "最终 Git blob 头部不是 UTF-8".to_string())?;
        let mut fields = header.split_whitespace();
        let oid = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        let size = fields
            .next()
            .unwrap_or_default()
            .parse::<u64>()
            .map_err(|_| "最终 Git blob 响应大小无效".to_string())?;
        if fields.next().is_some() || oid != blob.oid || kind != "blob" || size != blob.size {
            return Err("最终 Git blob 响应与 tree 不一致".to_string());
        }
        let content_start = header_end + 1;
        let content_end = content_start
            .checked_add(size as usize)
            .ok_or_else(|| "最终 Git blob 大小溢出".to_string())?;
        if output.stdout.get(content_end) != Some(&b'\n') {
            return Err("最终 Git blob 响应不完整".to_string());
        }
        let classification = classify_backup_bytes(&output.stdout[content_start..content_end]);
        match classification {
            SensitiveScan::Safe => {}
            SensitiveScan::Sensitive => {
                return Err(format!(
                    "最终 Git tree 包含疑似敏感内容，已拒绝提交: {}",
                    String::from_utf8_lossy(&blob.sample_path)
                ));
            }
            SensitiveScan::Unscannable(reason) => {
                return Err(format!(
                    "最终 Git tree 包含无法安全扫描的文件（{}），已拒绝提交: {}",
                    unscannable_description(reason),
                    String::from_utf8_lossy(&blob.sample_path)
                ));
            }
        }
        cursor = content_end + 1;
    }
    if cursor != output.stdout.len() {
        return Err("最终 Git blob 响应包含额外数据".to_string());
    }
    Ok(())
}

fn prepared_backup_was_committed(
    repo: &Path,
    journal: &BackupSnapshotJournal,
) -> Result<bool, String> {
    let current_head = current_git_head(repo)?;
    if current_head == journal.baseline_head {
        return Ok(false);
    }
    if current_head.is_none() {
        return Err(
            "备份事务期间 Git HEAD 从已有提交变为 unborn，已保留现场并拒绝自动回滚".to_string(),
        );
    }
    let current_head = current_head.expect("已在上方排除 unborn HEAD");
    let expected_commit = journal.expected_commit.as_deref().ok_or_else(|| {
        "备份事务期间 Git HEAD 已变化，但日志缺少精确 commit；已保留现场并拒绝自动回滚".to_string()
    })?;
    if current_head != expected_commit {
        return Err(
            "备份事务期间 Git HEAD 已变化但不是本事务提交；已保留现场并拒绝自动回滚".to_string(),
        );
    }
    validate_recovered_backup_commit(repo, &current_head, journal)?;
    Ok(true)
}

fn validate_recovered_backup_commit(
    repo: &Path,
    commit: &str,
    journal: &BackupSnapshotJournal,
) -> Result<(), String> {
    let expected_tree = journal.expected_tree.as_deref().ok_or_else(|| {
        "备份事务期间 Git HEAD 已变化，但日志缺少预期 tree；已拒绝自动回滚".to_string()
    })?;
    if git_tree(repo, commit)? != expected_tree {
        return Err("备份事务提交的 tree 与日志不一致；已保留现场并拒绝自动回滚".to_string());
    }
    let expected_parents = journal.baseline_head.iter().cloned().collect::<Vec<_>>();
    if git_commit_parents(repo, commit)? != expected_parents {
        return Err("备份事务提交的 parent 与基线不一致；已保留现场并拒绝自动回滚".to_string());
    }
    Ok(())
}

fn git_commit_parents(repo: &Path, commit: &str) -> Result<Vec<String>, String> {
    let output = run_git(
        repo,
        &["rev-list", "--parents", "--max-count=1", commit],
        Duration::from_secs(10),
    )?;
    if !output.status.success() {
        return Err(format!(
            "读取备份提交 parent 失败: {}",
            command_output(&output)
        ));
    }
    let mut fields = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if fields.first().map(String::as_str) != Some(commit) {
        return Err("读取备份提交 parent 时返回了意外 commit".to_string());
    }
    fields.remove(0);
    Ok(fields)
}

fn write_backup_transaction_owner(
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

fn read_bounded_regular_file(
    path: &Path,
    label: &str,
    max_bytes: u64,
) -> Result<Option<Vec<u8>>, String> {
    let expected = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取 {} 失败: {}", label, error)),
    };
    if expected.file_type().is_symlink() || !expected.is_file() {
        return Err(format!("{} 不是普通文件", label));
    }
    if expected.len() > max_bytes {
        return Err(format!("{} 超过 {} 字节上限", label, max_bytes));
    }

    let mut file =
        fs::File::open(path).map_err(|error| format!("读取 {} 失败: {}", label, error))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("读取 {} 元数据失败: {}", label, error))?;
    let current =
        fs::symlink_metadata(path).map_err(|error| format!("复核 {} 失败: {}", label, error))?;
    if current.file_type().is_symlink()
        || !current.is_file()
        || !same_backup_file_identity(&expected, &opened)
        || !same_backup_file_identity(&expected, &current)
    {
        return Err(format!("{} 在读取期间被替换", label));
    }

    let mut payload = Vec::new();
    Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut payload)
        .map_err(|error| format!("读取 {} 失败: {}", label, error))?;
    if payload.len() as u64 > max_bytes {
        return Err(format!("{} 超过 {} 字节上限", label, max_bytes));
    }
    let after = file
        .metadata()
        .map_err(|error| format!("复核 {} 元数据失败: {}", label, error))?;
    if !same_backup_file_identity(&opened, &after) || opened.len() != after.len() {
        return Err(format!("{} 在读取期间发生变化", label));
    }
    Ok(Some(payload))
}

fn read_backup_transaction_owner(transaction_root: &Path) -> Result<Option<String>, String> {
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

fn validate_backup_transaction_owner(
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

fn validate_backup_transaction_directory(
    transaction_root: &Path,
    transaction_id: &str,
) -> Result<(), String> {
    let expected_name = format!("{}{}", BACKUP_TRANSACTION_PREFIX, transaction_id);
    if transaction_root.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
        return Err("备份事务目录与日志标识不匹配".to_string());
    }
    Ok(())
}

fn journal_artifact_order(name: &str) -> Option<(u64, u8)> {
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

fn read_backup_snapshot_journal(
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

fn ensure_backup_transaction_branch(
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

fn recover_backup_transactions(repo: &Path) -> Result<(), String> {
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

fn restore_prepared_backup_transaction(
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

fn validate_previous_manifest(
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

fn cleanup_backup_manifest_artifacts(repo: &Path) -> Result<(), String> {
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

fn remove_backup_path(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).map_err(|error| error.to_string())
    } else {
        fs::remove_file(path).map_err(|error| error.to_string())
    }
}

fn validate_existing_snapshot_root(repo: &Path) -> Result<(), String> {
    let snapshot_root = repo.join("assistants");
    read_managed_snapshot_marker(&snapshot_root, "备份仓库中的 assistants")?;
    Ok(())
}

fn read_managed_snapshot_marker(path: &Path, label: &str) -> Result<Option<String>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取 {} 失败: {}", label, error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("{} 已存在但不是普通目录", label));
    }
    let marker = path.join(BACKUP_ROOT_MARKER);
    let marker_label = format!("{} 的 SkillMate 管理标记", label);
    let Some(payload) = read_bounded_regular_file(&marker, &marker_label, 1024)? else {
        return Err(format!("{} 不是 SkillMate 管理目录，已拒绝覆盖", label));
    };
    String::from_utf8(payload)
        .map(Some)
        .map_err(|_| format!("{} 不是 UTF-8 文本", marker_label))
}

fn transaction_snapshot_marker(transaction_id: &str) -> String {
    format!(
        "Managed by SkillMate backup transaction {}. This directory may be replaced during backup sync.\n",
        transaction_id
    )
}

#[derive(Default)]
struct BackupCopyBudget {
    files: usize,
    bytes: u64,
}

impl BackupCopyBudget {
    fn visit_file(&mut self, bytes: u64) -> Result<(), String> {
        self.files = self.files.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
        if self.files > MAX_BACKUP_FILES || self.bytes > MAX_BACKUP_BYTES {
            Err(format!(
                "备份超过限制（最多 {} 个文件、{} MB）",
                MAX_BACKUP_FILES,
                MAX_BACKUP_BYTES / 1024 / 1024
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SensitiveScan {
    Safe,
    Sensitive,
    Unscannable(&'static str),
}

struct ScannedBackupFile {
    classification: SensitiveScan,
    bytes: Vec<u8>,
    permissions: fs::Permissions,
    file_size: u64,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BackupSnapshotState {
    Prepared,
    Committed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupSnapshotJournal {
    version: u32,
    #[serde(default)]
    generation: u64,
    state: BackupSnapshotState,
    #[serde(default)]
    transaction_id: String,
    #[serde(default)]
    baseline_branch: Option<String>,
    baseline_head: Option<String>,
    #[serde(default)]
    expected_tree: Option<String>,
    #[serde(default)]
    expected_commit: Option<String>,
    #[serde(default)]
    previous_snapshot_marker: Option<String>,
    #[serde(default)]
    previous_manifest_len: Option<u64>,
    #[serde(default)]
    previous_manifest_sha256: Option<String>,
    had_snapshot: bool,
    had_manifest: bool,
}

struct BackupSnapshotTransaction {
    repo: PathBuf,
    transaction_root: PathBuf,
    journal: BackupSnapshotJournal,
    finished: bool,
}

impl BackupSnapshotTransaction {
    fn stage_for_commit(&mut self) -> Result<(), String> {
        let expected_paths = scan_final_backup_paths(&self.repo)?;
        stage_backup_snapshot(&self.repo)?;
        let expected_tree = staged_git_tree(&self.repo)?;
        validate_backup_tree_scope(&self.repo, &self.journal, &expected_tree)?;
        validate_backup_tree_blobs(&self.repo, &expected_tree, &expected_paths)?;
        update_backup_snapshot_journal(&self.transaction_root, &mut self.journal, move |journal| {
            journal.expected_tree = Some(expected_tree)
        })
    }

    fn commit_git_snapshot(mut self, message: &str) -> Result<(), String> {
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

    fn rollback(&mut self) -> Result<(), String> {
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

    fn mark_committed(&mut self) -> Result<(), String> {
        update_backup_snapshot_journal(&self.transaction_root, &mut self.journal, |journal| {
            journal.state = BackupSnapshotState::Committed
        })
    }

    fn verify_commit_result(&self) -> Result<(), String> {
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

    fn finish_commit(&mut self) -> Result<(), String> {
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

fn snapshot_assistants(
    connection: &Connection,
    repo: &Path,
) -> Result<BackupSnapshotTransaction, String> {
    validate_existing_snapshot_root(repo)?;
    let snapshot_root = repo.join("assistants");
    let transaction_id = generate_id();
    let transaction_root = validated_backup_git_dir(repo)?
        .join(format!("{}{}", BACKUP_TRANSACTION_PREFIX, transaction_id));
    let staging_root = transaction_root.join("assistants");
    let backup_root = transaction_root.join("previous-assistants");
    fs::create_dir(&transaction_root).map_err(|error| error.to_string())?;
    let mut temporary_directory = TemporaryBackupDirectory {
        path: transaction_root.clone(),
        keep: false,
    };
    write_backup_transaction_owner(&transaction_root, &transaction_id)?;
    fs::create_dir(&staging_root).map_err(|error| error.to_string())?;
    fs::write(
        staging_root.join(BACKUP_ROOT_MARKER),
        transaction_snapshot_marker(&transaction_id),
    )
    .map_err(|error| error.to_string())?;

    let sources = collect_backup_sources(connection)?;
    let mut manifest = Vec::new();
    let mut budget = BackupCopyBudget::default();
    for source in sources.values() {
        let root_id = backup_root_id(&source.path);
        let target_root = staging_root.join("roots").join(&root_id);
        let mut report = BackupCopyReport::default();
        let copied = snapshot_backup_source(source, &target_root, &mut budget, &mut report)?;
        manifest.push(serde_json::json!({
            "sourcePath": display_backup_path(&source.path),
            "snapshotPath": format!("assistants/roots/{}", root_id),
            "exists": source.path.exists(),
            "copied": copied,
            "assistants": &source.assistants,
            "scopes": &source.scopes,
            "projects": &source.projects,
            "report": report,
        }));
    }
    let payload = serde_json::json!({
        "version": 3,
        "kind": "managed-skill-content-snapshot",
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "roots": manifest,
        "limitations": [
            "不包含 SkillMate 数据库、标签、场景或 Profile",
            "不跟随或复制软连接",
            "仅备份 SkillMate 明确登记的受管 Skill",
            "无法完成敏感内容扫描的非核心文件会被排除；核心 SKILL.md 会中止同步",
            "尽力排除常见凭据、密钥、运行时缓存和受管 sidecar"
        ],
    });
    let manifest_path = repo.join("skillmate-backup.json");
    let previous_manifest = read_bounded_regular_file(
        &manifest_path,
        "现有备份 manifest",
        MAX_BACKUP_MANIFEST_BYTES,
    )?;
    let manifest_payload =
        serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;

    let previous_snapshot_marker =
        read_managed_snapshot_marker(&snapshot_root, "备份仓库中的 assistants")?;
    let had_snapshot = previous_snapshot_marker.is_some();
    let previous_manifest_len = previous_manifest
        .as_ref()
        .map(|content| content.len() as u64);
    let previous_manifest_sha256 = previous_manifest.as_ref().map(|content| {
        let mut hash = StableHash::new();
        hash.update(content);
        hash.finish()
    });
    let mut journal = BackupSnapshotJournal {
        version: BACKUP_JOURNAL_VERSION,
        generation: 0,
        state: BackupSnapshotState::Prepared,
        transaction_id,
        baseline_branch: Some(current_git_branch(repo)?),
        baseline_head: current_git_head(repo)?,
        expected_tree: None,
        expected_commit: None,
        previous_snapshot_marker,
        previous_manifest_len,
        previous_manifest_sha256,
        had_snapshot,
        had_manifest: previous_manifest.is_some(),
    };
    if let Some(content) = &previous_manifest {
        atomic_write(
            &transaction_root.join(BACKUP_PREVIOUS_MANIFEST_FILE),
            content,
        )?;
    }
    update_backup_snapshot_journal(&transaction_root, &mut journal, |_| {})?;
    temporary_directory.keep = true;
    let transaction = BackupSnapshotTransaction {
        repo: repo.to_path_buf(),
        transaction_root,
        journal,
        finished: false,
    };

    if had_snapshot {
        fs::rename(&snapshot_root, &backup_root).map_err(|error| error.to_string())?;
    }
    fs::rename(&staging_root, &snapshot_root)
        .map_err(|error| format!("无法启用新备份快照: {}", error))?;
    atomic_write(&manifest_path, manifest_payload.as_bytes())?;
    Ok(transaction)
}

fn snapshot_backup_source(
    source: &BackupSource,
    target: &Path,
    budget: &mut BackupCopyBudget,
    report: &mut BackupCopyReport,
) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(&source.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() {
        record_backup_exclusion(report, &source.path, &source.path, "symlink");
        return Ok(false);
    }
    if !metadata.is_dir() {
        return Err(format!(
            "受管 Skill 不是目录: {}",
            source.path.to_string_lossy()
        ));
    }
    copy_backup_tree(&source.path, target, &source.path, 0, budget, report)?;
    Ok(true)
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
            let scanned = read_scanned_backup_file(&source_path, &metadata)?;
            budget.visit_file(scanned.file_size)?;
            match scanned.classification {
                SensitiveScan::Safe => {}
                SensitiveScan::Sensitive => {
                    if is_core_skill_file(source_root, &source_path) {
                        return Err(format!(
                            "核心 Skill 文件包含疑似敏感内容，已中止备份: {}",
                            display_backup_path(&source_path)
                        ));
                    }
                    record_backup_exclusion(report, source_root, &source_path, "sensitive_content");
                    continue;
                }
                SensitiveScan::Unscannable(reason) => {
                    if is_core_skill_file(source_root, &source_path) {
                        return Err(format!(
                            "核心 Skill 文件无法安全扫描（{}），已中止备份: {}",
                            unscannable_description(reason),
                            display_backup_path(&source_path)
                        ));
                    }
                    record_backup_exclusion(report, source_root, &source_path, reason);
                    continue;
                }
            }
            write_scanned_backup_file(&target_path, &scanned)?;
            report.copied_files += 1;
            report.copied_bytes = report
                .copied_bytes
                .saturating_add(scanned.bytes.len() as u64);
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
    for installation in list_managed_installations(connection)? {
        add_backup_source(
            &mut sources,
            installation.path,
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
    let candidate_is_symlink = fs::symlink_metadata(&path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    let source = sources.entry(identity).or_insert_with(|| BackupSource {
        path: path.clone(),
        ..BackupSource::default()
    });
    let current_is_symlink = fs::symlink_metadata(&source.path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    if current_is_symlink && !candidate_is_symlink {
        source.path = path;
    }
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
    let display_path = if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative.to_string_lossy().to_string()
    };
    report.exclusions.push(BackupExclusion {
        path: display_path,
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
                | "token"
                | "token.txt"
                | "auth.json"
                | "access-token"
                | "access_token"
                | "api-key"
                | "api_key"
                | "id_rsa"
                | "id_ed25519"
        )
        || lower.contains("credential")
        || lower.contains("secret")
        || sensitive_name_word(&lower)
        || [".pem", ".key", ".p12", ".pfx"]
            .iter()
            .any(|extension| lower.ends_with(extension))
    {
        return Some("sensitive");
    }
    None
}

fn sensitive_name_word(name: &str) -> bool {
    name.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| matches!(part, "token" | "password" | "passwd" | "apikey"))
}

fn is_core_skill_file(source_root: &Path, path: &Path) -> bool {
    path.parent() == Some(source_root)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.eq_ignore_ascii_case("SKILL.md"))
            .unwrap_or(false)
}

fn unscannable_description(reason: &str) -> &'static str {
    match reason {
        "unscannable_too_large" => "文件超过安全扫描上限",
        "unscannable_binary" => "文件包含二进制或 UTF-16 内容",
        "unscannable_encoding" => "文件不是合法 UTF-8 文本",
        _ => "未知格式",
    }
}

fn read_scanned_backup_file(
    path: &Path,
    expected_metadata: &fs::Metadata,
) -> Result<ScannedBackupFile, String> {
    read_scanned_backup_file_with_limit(path, expected_metadata, MAX_SENSITIVE_SCAN_BYTES)
}

fn read_scanned_backup_file_with_limit(
    path: &Path,
    expected_metadata: &fs::Metadata,
    max_bytes: u64,
) -> Result<ScannedBackupFile, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let opened_metadata = file.metadata().map_err(|error| error.to_string())?;
    if !opened_metadata.is_file() || !same_backup_file_identity(expected_metadata, &opened_metadata)
    {
        return Err(format!(
            "备份扫描期间文件已被替换，已中止: {}",
            display_backup_path(path)
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let classification = if bytes.len() as u64 > max_bytes {
        SensitiveScan::Unscannable("unscannable_too_large")
    } else {
        classify_backup_bytes(&bytes)
    };
    Ok(ScannedBackupFile {
        classification,
        bytes,
        permissions: opened_metadata.permissions(),
        file_size: opened_metadata.len(),
    })
}

fn classify_backup_bytes(bytes: &[u8]) -> SensitiveScan {
    if bytes.contains(&0) {
        SensitiveScan::Unscannable("unscannable_binary")
    } else {
        match std::str::from_utf8(bytes) {
            Ok(content) => classify_sensitive_content(content),
            Err(_) => SensitiveScan::Unscannable("unscannable_encoding"),
        }
    }
}

fn write_scanned_backup_file(target: &Path, scanned: &ScannedBackupFile) -> Result<(), String> {
    fs::write(target, &scanned.bytes).map_err(|error| error.to_string())?;
    fs::set_permissions(target, scanned.permissions.clone()).map_err(|error| error.to_string())
}

#[cfg(unix)]
fn same_backup_file_identity(expected: &fs::Metadata, opened: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    expected.dev() == opened.dev() && expected.ino() == opened.ino()
}

#[cfg(not(unix))]
fn same_backup_file_identity(expected: &fs::Metadata, opened: &fs::Metadata) -> bool {
    expected.is_file() && opened.is_file() && expected.len() == opened.len()
}

#[cfg(test)]
fn scan_backup_file(path: &Path) -> Result<SensitiveScan, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    read_scanned_backup_file(path, &metadata).map(|scanned| scanned.classification)
}

fn classify_sensitive_content(content: &str) -> SensitiveScan {
    let lower = content.to_ascii_lowercase();
    if lower.contains("-----begin private key-----")
        || lower.contains("-----begin rsa private key-----")
        || lower.contains("-----begin openssh private key-----")
        || [
            ("github_pat_", 20),
            ("ghp_", 20),
            ("gho_", 20),
            ("ghu_", 20),
            ("ghs_", 20),
            ("ghr_", 20),
            ("sk-", 32),
            ("AKIA", 16),
            ("ASIA", 16),
            ("xoxb-", 20),
            ("xoxp-", 20),
            ("xoxa-", 20),
            ("xoxr-", 20),
            ("xoxs-", 20),
        ]
        .iter()
        .any(|(prefix, minimum_tail)| contains_prefixed_secret(content, prefix, *minimum_tail))
        || contains_jwt(content)
    {
        return SensitiveScan::Sensitive;
    }
    for line in lower.lines() {
        for key in [
            "access_token",
            "refresh_token",
            "api_key",
            "apikey",
            "client_secret",
            "private_key",
            "password",
        ] {
            let Some(key_index) = line.find(key) else {
                continue;
            };
            let remainder = &line[key_index + key.len()..];
            let Some(separator) = remainder.find(['=', ':']) else {
                continue;
            };
            if looks_like_secret_value(&remainder[separator + 1..]) {
                return SensitiveScan::Sensitive;
            }
        }
    }
    SensitiveScan::Safe
}

fn contains_prefixed_secret(content: &str, prefix: &str, minimum_tail: usize) -> bool {
    content.match_indices(prefix).any(|(index, _)| {
        let has_token_boundary = index == 0
            || !content.as_bytes()[index - 1].is_ascii_alphanumeric()
                && !matches!(content.as_bytes()[index - 1], b'_' | b'-');
        if !has_token_boundary {
            return false;
        }
        content[index + prefix.len()..]
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || matches!(*character, '_' | '-')
            })
            .count()
            >= minimum_tail
    })
}

fn contains_jwt(content: &str) -> bool {
    content
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
        })
        .map(|candidate| candidate.trim_matches('.'))
        .any(|candidate| {
            let mut parts = candidate.split('.');
            let (Some(header), Some(payload), Some(signature)) =
                (parts.next(), parts.next(), parts.next())
            else {
                return false;
            };
            parts.next().is_none()
                && header.starts_with("eyJ")
                && header.len() >= 12
                && payload.len() >= 12
                && signature.len() >= 16
                && [header, payload, signature].iter().all(|part| {
                    part.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                    })
                })
        })
}

fn looks_like_secret_value(value: &str) -> bool {
    let value = value
        .trim()
        .trim_matches(|character| matches!(character, '\'' | '"' | ',' | ';'));
    if value.len() < 8 {
        return false;
    }
    if value.chars().any(char::is_whitespace) {
        return false;
    }
    let placeholders = [
        "${",
        "{{",
        "<",
        "example",
        "placeholder",
        "changeme",
        "replace_me",
        "your_",
        "dummy",
        "xxxx",
        "****",
    ];
    !placeholders
        .iter()
        .any(|placeholder| value.contains(placeholder))
        && value
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .count()
            >= 8
}

fn run_git(repo: &Path, args: &[&str], timeout: Duration) -> Result<std::process::Output, String> {
    run_git_with_optional_input(repo, args, None, timeout)
}

fn run_git_with_input(
    repo: &Path,
    args: &[&str],
    input: &[u8],
    timeout: Duration,
) -> Result<std::process::Output, String> {
    run_git_with_optional_input(repo, args, Some(input), timeout)
}

fn run_git_with_optional_input(
    repo: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    const SAFE_GIT_ENVS: &[(&str, &str)] = &[
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GCM_INTERACTIVE", "Never"),
        ("GIT_LFS_SKIP_SMUDGE", "1"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_NO_REPLACE_OBJECTS", "1"),
    ];
    let mut safe_args = vec![
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "protocol.file.allow=user",
        "-c",
        "submodule.recurse=false",
    ];
    safe_args.extend_from_slice(args);
    run_command_with_options(
        "git",
        &safe_args,
        Some(repo),
        timeout,
        CommandOptions {
            envs: SAFE_GIT_ENVS,
            removed_env_prefixes: &["GIT_"],
            stdin: input,
            ..CommandOptions::default()
        },
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
}
