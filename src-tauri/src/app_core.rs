use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};
use toml::Value as TomlValue;

#[derive(Debug, Clone, Copy)]
pub struct AssistantDefinition {
    pub name: &'static str,
    pub path: &'static str,
    pub ai_type: &'static str,
    pub icon: &'static str,
}

pub fn generate_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    )
}

pub fn expand_path(path: &str) -> PathBuf {
    if path.starts_with('~') {
        let home = dirs::home_dir().unwrap_or_default();
        PathBuf::from(home).join(path.trim_start_matches('~').trim_start_matches('/'))
    } else {
        PathBuf::from(path)
    }
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn assistant_definitions() -> Vec<AssistantDefinition> {
    vec![
        AssistantDefinition {
            name: "Claude Code",
            path: "~/.claude/skills",
            ai_type: "skill",
            icon: "🔮",
        },
        AssistantDefinition {
            name: "Codex",
            path: "~/.codex/skills",
            ai_type: "skill",
            icon: "📝",
        },
        AssistantDefinition {
            name: "OpenClaw",
            path: "~/.openclaw/skills",
            ai_type: "skill",
            icon: "🦞",
        },
        AssistantDefinition {
            name: "Gemini CLI",
            path: "~/.gemini/cli/extensions",
            ai_type: "skill",
            icon: "🌟",
        },
    ]
}

pub fn assistant_root_by_name(name: &str) -> Result<PathBuf, String> {
    assistant_definitions()
        .into_iter()
        .find(|assistant| assistant.name == name)
        .map(|assistant| expand_path(assistant.path))
        .ok_or_else(|| format!("不支持的助手: {}", name))
}

pub fn project_skill_root_by_name(name: &str, project_path: &Path) -> Result<PathBuf, String> {
    let relative = match name {
        "Claude Code" => [".claude", "skills"].iter().collect::<PathBuf>(),
        "Codex" => [".codex", "skills"].iter().collect::<PathBuf>(),
        "OpenClaw" => [".openclaw", "skills"].iter().collect::<PathBuf>(),
        "Gemini CLI" => [".gemini", "cli", "extensions"].iter().collect::<PathBuf>(),
        _ => return Err(format!("不支持的助手: {}", name)),
    };
    Ok(project_path.join(relative))
}

pub fn assistant_slug(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn managed_skill_roots() -> Vec<PathBuf> {
    assistant_definitions()
        .into_iter()
        .map(|assistant| expand_path(assistant.path))
        .collect()
}

pub fn is_managed_skill_path(path: &Path, roots: &[PathBuf]) -> bool {
    let Ok(canonical_path) = path.canonicalize() else {
        return false;
    };

    roots.iter().any(|root| {
        root.canonicalize()
            .map(|canonical_root| {
                canonical_path.starts_with(&canonical_root) && canonical_path != canonical_root
            })
            .unwrap_or(false)
    })
}

pub fn is_managed_link_entry_path(path: &Path, roots: &[PathBuf]) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if path.file_name().is_none() {
        return false;
    }
    let Ok(canonical_parent) = parent.canonicalize() else {
        return false;
    };

    roots.iter().any(|root| {
        root.canonicalize()
            .map(|canonical_root| canonical_parent.starts_with(&canonical_root))
            .unwrap_or(false)
    })
}

pub fn git_base_dir(path: &Path) -> &Path {
    if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    }
}

pub fn git_output(path: &Path, args: &[&str]) -> Option<String> {
    let base = git_base_dir(path);
    let out = Command::new("git")
        .args(args)
        .current_dir(base)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn find_git_repo_root(path: &Path) -> Option<PathBuf> {
    let root = git_output(path, &["rev-parse", "--show-toplevel"])?;
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

pub fn get_git_remote_url(path: &Path) -> Option<String> {
    let remote = git_output(path, &["config", "--get", "remote.origin.url"])?;
    if remote.is_empty() {
        None
    } else {
        Some(remote)
    }
}

pub fn has_git_upstream(path: &Path) -> bool {
    git_output(
        path,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .is_some()
}

pub fn git_count(path: &Path, spec: &str) -> u32 {
    git_output(path, &["rev-list", "--count", spec])
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0)
}

pub fn manifest_base_dir(path: &Path) -> &Path {
    if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    }
}

pub fn read_json_name_version(path: &Path) -> Option<(String, String)> {
    let content = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let name = value.get("name")?.as_str()?.trim().to_string();
    let version = value
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        None
    } else {
        Some((name, version))
    }
}

pub fn read_pyproject_name_version(path: &Path) -> Option<(String, String)> {
    let content = fs::read_to_string(path).ok()?;
    let value: TomlValue = content.parse().ok()?;
    let project = value.get("project")?;
    let name = project.get("name")?.as_str()?.trim().to_string();
    let version = project
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        None
    } else {
        Some((name, version))
    }
}

pub fn detect_npm_package(path: &Path) -> Option<(String, String)> {
    read_json_name_version(&manifest_base_dir(path).join("package.json"))
}

pub fn detect_pip_package(path: &Path) -> Option<(String, String)> {
    read_pyproject_name_version(&manifest_base_dir(path).join("pyproject.toml"))
}

pub fn command_exists(cmd: &str) -> bool {
    let check = if cfg!(target_os = "windows") {
        Command::new("where").arg(cmd).output()
    } else {
        Command::new("which").arg(cmd).output()
    };
    check.map(|o| o.status.success()).unwrap_or(false)
}

pub fn run_command_with_timeout(
    cmd: &str,
    args: &[&str],
    current_dir: Option<&Path>,
    timeout: Duration,
    envs: &[(&str, &str)],
) -> Result<Output, String> {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => return child.wait_with_output().map_err(|e| e.to_string()),
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("命令执行超时（{} 秒）", timeout.as_secs()));
            }
            None => sleep(Duration::from_millis(100)),
        }
    }
}
