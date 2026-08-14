use super::*;

use crate::app_core::assistant_definitions;
use crate::managed_installation::list_managed_roots;
use rusqlite::Connection;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) fn normalized_branch(branch: &str) -> String {
    if branch.trim().is_empty() {
        "main".to_string()
    } else {
        branch.trim().to_string()
    }
}

pub(super) fn validate_backup_repo_location(connection: &Connection, repo: &Path) -> Result<(), String> {
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

pub(super) fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

pub(super) fn canonicalize_for_comparison(path: &Path) -> Result<PathBuf, String> {
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

pub(super) fn ensure_git_repo(repo: &Path) -> Result<(), String> {
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

pub(super) fn validated_backup_git_dir(repo: &Path) -> Result<PathBuf, String> {
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

pub(super) fn ensure_git_identity(repo: &Path) -> Result<(), String> {
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

pub(super) fn git_output(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = run_git(repo, args, Duration::from_secs(10))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(command_output(&output))
    }
}

pub(super) fn checkout_git_branch(repo: &Path, branch: &str) -> Result<(), String> {
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

pub(super) fn ensure_git_worktree_clean(repo: &Path) -> Result<(), String> {
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

pub(super) fn configure_git_remote(repo: &Path, remote_url: &str) -> Result<(), String> {
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
