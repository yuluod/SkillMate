use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};
use toml::Value as TomlValue;

#[derive(Debug, Clone, Copy)]
pub struct AssistantDefinition {
    pub name: &'static str,
    pub ai_type: &'static str,
    pub icon: &'static str,
    global_install_path: &'static str,
    global_discovery_paths: &'static [&'static str],
    project_install_path: &'static str,
    recursive_discovery_depth: usize,
}

impl AssistantDefinition {
    pub fn global_install_root(self) -> PathBuf {
        expand_path(self.global_install_path)
    }

    pub fn global_discovery_roots(self) -> impl Iterator<Item = PathBuf> {
        self.global_discovery_paths
            .iter()
            .map(|path| expand_path(path))
    }

    pub fn project_install_root(self, project_path: &Path) -> PathBuf {
        project_path.join(self.project_install_path)
    }

    pub fn recursive_discovery_depth(self) -> usize {
        self.recursive_discovery_depth
    }
}

pub fn generate_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static ID_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let sequence = ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:x}-{:x}", timestamp, std::process::id(), sequence)
}

pub fn expand_path(path: &str) -> PathBuf {
    if path.starts_with('~') {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(path.trim_start_matches('~').trim_start_matches('/'))
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

pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| "目标文件缺少父目录".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "state".into());
    let sequence = WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".{}.skillmate-tmp-{}-{}",
        name,
        std::process::id(),
        sequence
    ));
    let mut file = fs::File::create(&temp).map_err(|error| error.to_string())?;
    if let Err(error) = file.write_all(content).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temp);
        return Err(error.to_string());
    }
    drop(file);

    #[cfg(not(target_os = "windows"))]
    {
        fs::rename(&temp, path).map_err(|error| {
            let _ = fs::remove_file(&temp);
            error.to_string()
        })?;
    }
    #[cfg(target_os = "windows")]
    {
        let backup = parent.join(format!(".{}.skillmate-old-{}", name, sequence));
        let had_target = path.exists();
        if had_target {
            fs::rename(path, &backup).map_err(|error| {
                let _ = fs::remove_file(&temp);
                error.to_string()
            })?;
        }
        if let Err(error) = fs::rename(&temp, path) {
            if had_target {
                let _ = fs::rename(&backup, path);
            }
            let _ = fs::remove_file(&temp);
            return Err(error.to_string());
        }
        if had_target {
            fs::remove_file(&backup).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn assistant_definitions() -> &'static [AssistantDefinition] {
    &[
        AssistantDefinition {
            name: "Claude Code",
            ai_type: "skill",
            icon: "claude",
            global_install_path: "~/.claude/skills",
            global_discovery_paths: &["~/.claude/skills"],
            project_install_path: ".claude/skills",
            recursive_discovery_depth: 2,
        },
        AssistantDefinition {
            name: "Codex",
            ai_type: "skill",
            icon: "codex",
            global_install_path: "~/.agents/skills",
            global_discovery_paths: &["~/.agents/skills", "~/.codex/skills"],
            project_install_path: ".agents/skills",
            recursive_discovery_depth: 6,
        },
        AssistantDefinition {
            name: "OpenClaw",
            ai_type: "skill",
            icon: "openclaw",
            global_install_path: "~/.openclaw/skills",
            global_discovery_paths: &["~/.openclaw/skills", "~/.agents/skills"],
            project_install_path: "skills",
            recursive_discovery_depth: 6,
        },
        AssistantDefinition {
            name: "Gemini CLI",
            ai_type: "skill",
            icon: "gemini",
            global_install_path: "~/.gemini/skills",
            // Gemini 在同一层级中优先采用共享别名，再回退原生目录。
            global_discovery_paths: &["~/.agents/skills", "~/.gemini/skills"],
            project_install_path: ".gemini/skills",
            recursive_discovery_depth: 2,
        },
    ]
}

pub fn assistant_root_by_name(name: &str) -> Result<PathBuf, String> {
    assistant_definitions()
        .iter()
        .find(|assistant| assistant.name == name)
        .map(|assistant| assistant.global_install_root())
        .ok_or_else(|| format!("不支持的助手: {}", name))
}

pub fn project_skill_root_by_name(name: &str, project_path: &Path) -> Result<PathBuf, String> {
    assistant_definitions()
        .iter()
        .find(|assistant| assistant.name == name)
        .map(|assistant| assistant.project_install_root(project_path))
        .ok_or_else(|| format!("不支持的助手: {}", name))
}

pub fn managed_skill_roots() -> Vec<PathBuf> {
    assistant_definitions()
        .iter()
        .flat_map(|assistant| assistant.global_discovery_roots())
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
    let base = git_base_dir(path);
    for ancestor in base.ancestors() {
        let git_entry = ancestor.join(".git");
        if git_entry.is_dir() || git_entry.is_file() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
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
    static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let capture_id = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let capture_root = std::env::temp_dir().join(format!(
        "skillmate-command-{}-{}-{}",
        std::process::id(),
        now_ms(),
        capture_id
    ));
    fs::create_dir_all(&capture_root).map_err(|error| error.to_string())?;
    let stdout_path = capture_root.join("stdout");
    let stderr_path = capture_root.join("stderr");
    let stdout_file = fs::File::create(&stdout_path).map_err(|error| error.to_string())?;
    let stderr_file = fs::File::create(&stderr_path).map_err(|error| error.to_string())?;
    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_dir_all(&capture_root);
            return Err(error.to_string());
        }
    };
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_dir_all(&capture_root);
                return Err(format!("命令执行超时（{} 秒）", timeout.as_secs()));
            }
            None => sleep(Duration::from_millis(100)),
        }
    };
    let stdout = fs::read(&stdout_path).unwrap_or_default();
    let stderr = fs::read(&stderr_path).unwrap_or_default();
    let _ = fs::remove_dir_all(&capture_root);
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_uses_agent_skills_directories() {
        let project = Path::new("/tmp/example-project");
        let global = assistant_root_by_name("Gemini CLI").unwrap();
        let project_root = project_skill_root_by_name("Gemini CLI", project).unwrap();

        assert!(global.ends_with(".gemini/skills"));
        assert_eq!(project_root, project.join(".gemini/skills"));
    }

    #[test]
    fn gemini_discovers_shared_agents_directory() {
        let gemini = assistant_definitions()
            .iter()
            .find(|assistant| assistant.name == "Gemini CLI")
            .copied()
            .unwrap();
        let roots = gemini.global_discovery_roots().collect::<Vec<_>>();

        assert!(roots.iter().any(|root| root.ends_with(".gemini/skills")));
        assert!(roots.iter().any(|root| root.ends_with(".agents/skills")));
    }

    #[test]
    fn gemini_discovery_order_matches_upstream_precedence() {
        let gemini = assistant_definitions()
            .iter()
            .find(|assistant| assistant.name == "Gemini CLI")
            .unwrap();
        let roots = gemini.global_discovery_roots().collect::<Vec<_>>();

        assert!(roots[0].ends_with(".agents/skills"));
        assert!(roots[1].ends_with(".gemini/skills"));
    }

    #[test]
    fn assistant_path_matrix_matches_upstream_conventions() {
        struct ExpectedPaths {
            name: &'static str,
            icon: &'static str,
            global_install: &'static str,
            global_discovery: &'static [&'static str],
            project_install: &'static str,
        }

        let project = Path::new("/tmp/example-project");
        let expected = [
            ExpectedPaths {
                name: "Claude Code",
                icon: "claude",
                global_install: ".claude/skills",
                global_discovery: &[".claude/skills"],
                project_install: ".claude/skills",
            },
            ExpectedPaths {
                name: "Codex",
                icon: "codex",
                global_install: ".agents/skills",
                global_discovery: &[".agents/skills", ".codex/skills"],
                project_install: ".agents/skills",
            },
            ExpectedPaths {
                name: "OpenClaw",
                icon: "openclaw",
                global_install: ".openclaw/skills",
                global_discovery: &[".openclaw/skills", ".agents/skills"],
                project_install: "skills",
            },
            ExpectedPaths {
                name: "Gemini CLI",
                icon: "gemini",
                global_install: ".gemini/skills",
                global_discovery: &[".gemini/skills", ".agents/skills"],
                project_install: ".gemini/skills",
            },
        ];

        for expected in expected {
            let assistant = assistant_definitions()
                .iter()
                .find(|assistant| assistant.name == expected.name)
                .copied()
                .unwrap();
            assert_eq!(assistant.icon, expected.icon);
            assert!(assistant
                .global_install_root()
                .ends_with(expected.global_install));
            let discovery = assistant.global_discovery_roots().collect::<Vec<_>>();
            assert_eq!(discovery.len(), expected.global_discovery.len());
            for path in expected.global_discovery {
                assert!(discovery.iter().any(|root| root.ends_with(path)));
            }
            assert_eq!(
                assistant.project_install_root(project),
                project.join(expected.project_install)
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn command_timeout_capture_handles_output_larger_than_pipe_buffer() {
        let output = run_command_with_timeout(
            "sh",
            &[
                "-c",
                "i=0; while [ $i -lt 20000 ]; do echo 1234567890; i=$((i+1)); done",
            ],
            None,
            Duration::from_secs(5),
            &[],
        )
        .unwrap();

        assert!(output.status.success());
        assert!(output.stdout.len() > 100_000);
    }

    #[test]
    fn git_root_is_found_from_filesystem_without_invoking_repository_children() {
        let root = std::env::temp_dir().join(format!("skillmate-git-root-{}", generate_id()));
        let skill = root.join("skills/writer");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(&skill).unwrap();

        assert_eq!(find_git_repo_root(&skill).as_deref(), Some(root.as_path()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generated_ids_remain_unique_during_burst_writes() {
        let ids = (0..1_000)
            .map(|_| generate_id())
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(ids.len(), 1_000);
    }
}
