use crate::app_core::{run_command_with_options, CommandOptions};

use std::path::Path;
use std::time::Duration;

pub(super) fn current_git_branch(repo: &Path) -> Result<String, String> {
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

pub(super) fn current_git_head(repo: &Path) -> Result<Option<String>, String> {
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

pub(super) fn git_tree(repo: &Path, revision: &str) -> Result<String, String> {
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

pub(super) fn staged_git_tree(repo: &Path) -> Result<String, String> {
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

pub(super) fn run_git(
    repo: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<std::process::Output, String> {
    run_git_with_optional_input(repo, args, None, timeout)
}

pub(super) fn run_git_with_input(
    repo: &Path,
    args: &[&str],
    input: &[u8],
    timeout: Duration,
) -> Result<std::process::Output, String> {
    run_git_with_optional_input(repo, args, Some(input), timeout)
}

pub(super) fn run_git_with_optional_input(
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

pub(super) fn run_git_checked(repo: &Path, args: &[&str], timeout: Duration) -> Result<(), String> {
    let output = run_git(repo, args, timeout)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_output(&output))
    }
}

pub(super) fn command_output(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match (stdout.is_empty(), stderr.is_empty()) {
        (false, false) => format!("{}\n{}", stdout, stderr),
        (false, true) => stdout,
        (true, false) => stderr,
        (true, true) => "Git 命令执行失败".to_string(),
    }
}
