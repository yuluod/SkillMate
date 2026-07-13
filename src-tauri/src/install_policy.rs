use crate::app_core::expand_path;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const INSTALL_POLICY_OFF: &str = "off";
pub const INSTALL_POLICY_BLOCK_CRITICAL: &str = "block-critical";
pub const INSTALL_POLICY_TRUSTED_ONLY: &str = "trusted-only";

const CRITICAL_WARNING_CODES: &[&str] = &[
    "unsafe_paths",
    "safety_scan_incomplete",
    "structure_preview_failed",
    "entry_document_truncated",
];

const RISK_WARNING_CODES: &[&str] = &[
    "contains_scripts",
    "declares_dependencies",
    "contains_symlinks",
    "contains_hidden_files",
    "references_network",
    "references_environment",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallPolicyConfig {
    pub mode: String,
    #[serde(default)]
    pub block_risky_content: bool,
    #[serde(default)]
    pub trusted_git_hosts: Vec<String>,
    #[serde(default)]
    pub trusted_local_roots: Vec<String>,
}

impl Default for InstallPolicyConfig {
    fn default() -> Self {
        Self {
            mode: INSTALL_POLICY_OFF.to_string(),
            block_risky_content: false,
            trusted_git_hosts: Vec::new(),
            trusted_local_roots: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallPolicyDecision {
    pub mode: String,
    pub allowed: bool,
    pub findings: Vec<InstallPolicyFinding>,
    pub message: String,
}

impl Default for InstallPolicyDecision {
    fn default() -> Self {
        Self {
            mode: INSTALL_POLICY_OFF.to_string(),
            allowed: true,
            findings: Vec::new(),
            message: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallPolicyFinding {
    pub code: String,
    pub severity: String,
    pub message: String,
}

pub struct InstallPolicyInput<'a> {
    pub source_kind: &'a str,
    pub source: &'a str,
    pub structure_status: &'a str,
    pub warnings: &'a [String],
}

pub fn load_install_policy(db: &Connection) -> Result<InstallPolicyConfig, String> {
    let row = db
        .query_row(
            "SELECT mode, block_risky_content, trusted_git_hosts_json, trusted_local_roots_json
             FROM install_policy WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i32>(1)? != 0,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((mode, block_risky_content, hosts, roots)) = row else {
        return Ok(InstallPolicyConfig::default());
    };
    let config = InstallPolicyConfig {
        mode,
        block_risky_content,
        trusted_git_hosts: serde_json::from_str(&hosts)
            .map_err(|error| format!("安装策略 Git 主机配置损坏: {error}"))?,
        trusted_local_roots: serde_json::from_str(&roots)
            .map_err(|error| format!("安装策略本地根目录配置损坏: {error}"))?,
    };
    validate_policy_mode(&config.mode)?;
    Ok(config)
}

pub fn save_install_policy(
    db: &Connection,
    config: InstallPolicyConfig,
) -> Result<InstallPolicyConfig, String> {
    validate_policy_mode(&config.mode)?;
    let normalized = InstallPolicyConfig {
        mode: config.mode,
        block_risky_content: config.block_risky_content,
        trusted_git_hosts: normalize_hosts(config.trusted_git_hosts)?,
        trusted_local_roots: normalize_roots(config.trusted_local_roots)?,
    };
    let hosts =
        serde_json::to_string(&normalized.trusted_git_hosts).map_err(|error| error.to_string())?;
    let roots = serde_json::to_string(&normalized.trusted_local_roots)
        .map_err(|error| error.to_string())?;
    db.execute(
        "INSERT INTO install_policy (
            id, mode, block_risky_content, trusted_git_hosts_json, trusted_local_roots_json
         ) VALUES (1, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            mode = excluded.mode,
            block_risky_content = excluded.block_risky_content,
            trusted_git_hosts_json = excluded.trusted_git_hosts_json,
            trusted_local_roots_json = excluded.trusted_local_roots_json",
        params![
            normalized.mode,
            i32::from(normalized.block_risky_content),
            hosts,
            roots,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(normalized)
}

pub fn evaluate_install_policy(
    config: &InstallPolicyConfig,
    input: InstallPolicyInput<'_>,
) -> InstallPolicyDecision {
    let mut findings = Vec::new();
    if input.structure_status == "nonstandard" {
        findings.push(finding(
            "nonstandard_skill",
            "critical",
            "来源未识别为标准 Skill",
        ));
    }
    for code in CRITICAL_WARNING_CODES {
        if input.warnings.iter().any(|warning| warning == code) {
            findings.push(finding(code, "critical", critical_warning_message(code)));
        }
    }
    if config.block_risky_content {
        for code in RISK_WARNING_CODES {
            if input.warnings.iter().any(|warning| warning == code) {
                findings.push(finding(code, "critical", risk_warning_message(code)));
            }
        }
    }
    if config.mode == INSTALL_POLICY_TRUSTED_ONLY {
        evaluate_source_trust(config, &input, &mut findings);
    }

    let enforced = config.mode != INSTALL_POLICY_OFF;
    let blocked = enforced && findings.iter().any(|item| item.severity == "critical");
    InstallPolicyDecision {
        mode: config.mode.clone(),
        allowed: !blocked,
        message: if blocked {
            format!("安装策略阻止了 {} 项风险", findings.len())
        } else if findings.is_empty() {
            "安装策略检查通过".to_string()
        } else if enforced {
            "安装策略检查通过，但存在提醒".to_string()
        } else {
            "安装策略未启用，仅记录风险".to_string()
        },
        findings,
    }
}

pub fn policy_failure_decision(message: impl Into<String>) -> InstallPolicyDecision {
    let message = message.into();
    InstallPolicyDecision {
        mode: "error".to_string(),
        allowed: false,
        findings: vec![finding("policy_unavailable", "critical", &message)],
        message,
    }
}

fn evaluate_source_trust(
    config: &InstallPolicyConfig,
    input: &InstallPolicyInput<'_>,
    findings: &mut Vec<InstallPolicyFinding>,
) {
    if input.source_kind.starts_with("git") {
        let local_git_path = expand_path(input.source);
        if local_git_path.is_absolute() || local_git_path.exists() {
            evaluate_local_source(config, &local_git_path, findings);
            return;
        }
        let host = git_host(input.source);
        if host.as_ref().map(|host| {
            config
                .trusted_git_hosts
                .iter()
                .any(|trusted| trusted == host)
        }) != Some(true)
        {
            findings.push(finding(
                "untrusted_git_host",
                "critical",
                &format!(
                    "Git 主机 {} 不在信任列表",
                    host.as_deref().unwrap_or("无法识别")
                ),
            ));
        }
    } else if input.source_kind == "local" {
        evaluate_local_source(config, &expand_path(input.source), findings);
    }
}

fn evaluate_local_source(
    config: &InstallPolicyConfig,
    source: &Path,
    findings: &mut Vec<InstallPolicyFinding>,
) {
    let source = comparable_path(source);
    let trusted = source.ok().is_some_and(|source| {
        config.trusted_local_roots.iter().any(|root| {
            comparable_path(&expand_path(root))
                .map(|root| source.starts_with(root))
                .unwrap_or(false)
        })
    });
    if !trusted {
        findings.push(finding(
            "untrusted_local_root",
            "critical",
            "本地来源不在信任根目录内",
        ));
    }
}

fn validate_policy_mode(mode: &str) -> Result<(), String> {
    if matches!(
        mode,
        INSTALL_POLICY_OFF | INSTALL_POLICY_BLOCK_CRITICAL | INSTALL_POLICY_TRUSTED_ONLY
    ) {
        Ok(())
    } else {
        Err("安装策略模式仅支持 off/block-critical/trusted-only".to_string())
    }
}

fn normalize_hosts(hosts: Vec<String>) -> Result<Vec<String>, String> {
    let mut normalized = BTreeSet::new();
    for host in hosts {
        let lowered = host.trim().to_ascii_lowercase();
        let host = lowered
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        if host.is_empty()
            || host.contains(['/', '\\', '@'])
            || host.chars().any(char::is_whitespace)
        {
            return Err(format!("无效的 Git 主机: {host}"));
        }
        normalized.insert(host);
    }
    Ok(normalized.into_iter().collect())
}

fn normalize_roots(roots: Vec<String>) -> Result<Vec<String>, String> {
    let mut normalized = BTreeSet::new();
    for root in roots {
        let root = root.trim();
        if root.is_empty() {
            return Err("信任根目录不能为空".to_string());
        }
        let path = expand_path(root);
        if !path.is_dir() {
            return Err(format!("信任根目录不存在: {}", path.to_string_lossy()));
        }
        let path = path.canonicalize().map_err(|error| error.to_string())?;
        normalized.insert(path.to_string_lossy().to_string());
    }
    Ok(normalized.into_iter().collect())
}

fn comparable_path(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        path.canonicalize().map_err(|error| error.to_string())
    } else if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path))
    }
}

fn git_host(locator: &str) -> Option<String> {
    let locator = locator.split('#').next().unwrap_or(locator).trim();
    if locator.split('/').count() == 2 && !locator.contains(':') && !locator.contains('@') {
        return Some("github.com".to_string());
    }
    if let Some(rest) = locator
        .strip_prefix("https://")
        .or_else(|| locator.strip_prefix("http://"))
        .or_else(|| locator.strip_prefix("ssh://"))
    {
        return rest
            .split('/')
            .next()
            .and_then(|authority| authority.rsplit('@').next())
            .map(|host| host.to_ascii_lowercase());
    }
    let authority = locator.split(':').next()?;
    authority
        .rsplit('@')
        .next()
        .filter(|host| host.contains('.'))
        .map(|host| host.to_ascii_lowercase())
}

fn finding(code: &str, severity: &str, message: &str) -> InstallPolicyFinding {
    InstallPolicyFinding {
        code: code.to_string(),
        severity: severity.to_string(),
        message: message.to_string(),
    }
}

fn critical_warning_message(code: &str) -> &'static str {
    match code {
        "unsafe_paths" => "Skill 包含异常或越界路径",
        "safety_scan_incomplete" => "安全扫描未完整覆盖 Skill 内容",
        "structure_preview_failed" => "Skill 结构预览失败",
        "entry_document_truncated" => "入口文档过大，未完整分析",
        _ => "Skill 包含关键风险",
    }
}

fn risk_warning_message(code: &str) -> &'static str {
    match code {
        "contains_scripts" => "Skill 包含可执行脚本",
        "declares_dependencies" => "Skill 声明了第三方依赖",
        "contains_symlinks" => "Skill 包含软连接",
        "contains_hidden_files" => "Skill 包含隐藏文件",
        "references_network" => "Skill 可能访问网络",
        "references_environment" => "Skill 可能读取环境变量或凭据",
        _ => "Skill 包含需要复核的内容",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_mode_only_reports_without_blocking() {
        let decision = evaluate_install_policy(
            &InstallPolicyConfig::default(),
            InstallPolicyInput {
                source_kind: "git",
                source: "owner/repo",
                structure_status: "complete",
                warnings: &["safety_scan_incomplete".to_string()],
            },
        );

        assert!(decision.allowed);
        assert_eq!(decision.findings.len(), 1);
    }

    #[test]
    fn critical_mode_blocks_incomplete_scan() {
        let decision = evaluate_install_policy(
            &InstallPolicyConfig {
                mode: INSTALL_POLICY_BLOCK_CRITICAL.to_string(),
                ..InstallPolicyConfig::default()
            },
            InstallPolicyInput {
                source_kind: "git",
                source: "owner/repo",
                structure_status: "complete",
                warnings: &["safety_scan_incomplete".to_string()],
            },
        );

        assert!(!decision.allowed);
        assert_eq!(decision.findings[0].code, "safety_scan_incomplete");
    }

    #[test]
    fn trusted_mode_checks_git_host() {
        let config = InstallPolicyConfig {
            mode: INSTALL_POLICY_TRUSTED_ONLY.to_string(),
            trusted_git_hosts: vec!["github.com".to_string()],
            ..InstallPolicyConfig::default()
        };
        let github = evaluate_install_policy(
            &config,
            InstallPolicyInput {
                source_kind: "git",
                source: "owner/repo",
                structure_status: "complete",
                warnings: &[],
            },
        );
        let unknown = evaluate_install_policy(
            &config,
            InstallPolicyInput {
                source_kind: "git",
                source: "https://example.com/owner/repo.git",
                structure_status: "complete",
                warnings: &[],
            },
        );

        assert!(github.allowed);
        assert!(!unknown.allowed);
        assert_eq!(unknown.findings[0].code, "untrusted_git_host");
    }

    #[test]
    fn trusted_mode_accepts_local_git_inside_trusted_root() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-policy-local-git-{}",
            crate::app_core::generate_id()
        ));
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let config = InstallPolicyConfig {
            mode: INSTALL_POLICY_TRUSTED_ONLY.to_string(),
            trusted_local_roots: vec![root.to_string_lossy().to_string()],
            ..InstallPolicyConfig::default()
        };

        let decision = evaluate_install_policy(
            &config,
            InstallPolicyInput {
                source_kind: "git",
                source: repo.to_string_lossy().as_ref(),
                structure_status: "complete",
                warnings: &[],
            },
        );

        assert!(decision.allowed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saving_policy_normalizes_and_roundtrips() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE install_policy (
                id INTEGER PRIMARY KEY,
                mode TEXT NOT NULL,
                block_risky_content INTEGER NOT NULL DEFAULT 0,
                trusted_git_hosts_json TEXT NOT NULL DEFAULT '[]',
                trusted_local_roots_json TEXT NOT NULL DEFAULT '[]'
             );",
        )
        .unwrap();
        let root = std::env::temp_dir();
        let saved = save_install_policy(
            &db,
            InstallPolicyConfig {
                mode: INSTALL_POLICY_TRUSTED_ONLY.to_string(),
                block_risky_content: true,
                trusted_git_hosts: vec![
                    "HTTPS://GitHub.com/".to_string(),
                    "github.com".to_string(),
                ],
                trusted_local_roots: vec![root.to_string_lossy().to_string()],
            },
        )
        .unwrap();

        assert_eq!(saved.trusted_git_hosts, vec!["github.com"]);
        assert_eq!(load_install_policy(&db).unwrap(), saved);
    }
}
