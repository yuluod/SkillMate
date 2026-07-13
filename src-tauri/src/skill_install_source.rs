use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallDetection {
    pub detector: String,
    pub source_kind: String,
    pub normalized_source: String,
    pub original_input: String,
    pub repo_url: Option<String>,
    pub reference: Option<String>,
    pub subdir: Option<String>,
    pub target_name: Option<String>,
    pub confidence: String,
    pub warnings: Vec<String>,
    pub needs_model: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitInstallSpec {
    pub original: String,
    pub repo_url: String,
    pub reference: Option<String>,
    pub subdir: Option<String>,
    pub target_name: String,
}

pub fn is_git_install_source(source: &str) -> bool {
    matches!(source, "git" | "github")
}

pub fn install_target_name(package: &str, source: &str) -> Result<String, String> {
    if source == "local" {
        let source_path = expand_user_path(package.trim());
        return source_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| "无法从本地目录推断 Skill 名称".to_string());
    }
    if is_git_install_source(source) {
        return parse_git_install_spec(package).map(|spec| spec.target_name);
    }

    Err("当前版本仅支持 Git 仓库和本地目录安装".to_string())
}

pub fn detect_install_source_rules(input: &str) -> InstallDetection {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return install_detection(
            "unknown",
            "",
            trimmed,
            "low",
            vec!["empty_input".to_string()],
            false,
        );
    }

    let path = expand_user_path(trimmed);
    if path.is_dir() || trimmed.starts_with('~') || Path::new(trimmed).is_absolute() {
        let target_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .filter(|name| !name.trim().is_empty());
        let mut detection = install_detection(
            "local_dir",
            "local",
            trimmed,
            if path.is_dir() { "high" } else { "medium" },
            if path.is_dir() {
                vec![]
            } else {
                vec!["path_missing".to_string()]
            },
            false,
        );
        detection.target_name = target_name;
        return detection;
    }

    if looks_like_archive_input(trimmed) {
        return install_detection(
            "archive",
            "",
            trimmed,
            "medium",
            vec!["archive_unsupported".to_string()],
            false,
        );
    }

    if looks_like_git_input(trimmed) {
        match parse_git_install_spec(trimmed) {
            Ok(spec) => {
                let mut detection = install_detection(
                    if spec.subdir.is_some() {
                        "git_subdir"
                    } else {
                        "git"
                    },
                    "git",
                    trimmed,
                    "high",
                    vec![],
                    false,
                );
                detection.repo_url = Some(spec.repo_url);
                detection.reference = spec.reference;
                detection.subdir = spec.subdir;
                detection.target_name = Some(spec.target_name);
                return detection;
            }
            Err(err) => {
                return install_detection("git", "git", trimmed, "low", vec![err], false);
            }
        }
    }

    install_detection(
        "unknown",
        "",
        trimmed,
        "low",
        vec!["unrecognized_input".to_string()],
        true,
    )
}

pub fn parse_git_install_spec(input: &str) -> Result<GitInstallSpec, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("请输入 Git 仓库地址".to_string());
    }

    if let Some(spec) = parse_github_tree_url(trimmed)? {
        return Ok(spec);
    }
    if let Some(spec) = parse_github_shorthand(trimmed) {
        return Ok(spec);
    }

    let (repo_url, reference, subdir) = if let Some((repo, fragment)) = trimmed.split_once('#') {
        let (reference, subdir) = parse_git_fragment(fragment)?;
        (repo.trim().to_string(), reference, subdir)
    } else {
        (trimmed.to_string(), None, None)
    };

    if repo_url.trim().is_empty() {
        return Err("请输入 Git 仓库地址".to_string());
    }
    validate_git_repo_locator(&repo_url)?;

    let target_name = subdir
        .as_deref()
        .and_then(subdir_target_name)
        .unwrap_or(git_repo_name(&repo_url)?);

    Ok(GitInstallSpec {
        original: trimmed.to_string(),
        repo_url,
        reference,
        subdir,
        target_name,
    })
}

pub fn validate_git_repo_locator(locator: &str) -> Result<(), String> {
    let trimmed = locator.trim();
    if trimmed.is_empty() {
        return Err("请输入 Git 仓库地址".to_string());
    }
    if trimmed.starts_with('-') || trimmed.chars().any(char::is_control) {
        return Err("Git 仓库地址包含不安全字符".to_string());
    }
    if Path::new(trimmed).exists()
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || Path::new(trimmed).is_absolute()
    {
        return Ok(());
    }
    if trimmed.starts_with("git@") && trimmed.contains(':') {
        return Ok(());
    }
    let Some((scheme, _)) = trimmed.split_once("://") else {
        return Err("Git 仓库地址仅支持 HTTPS、SSH、Git 或本地路径".to_string());
    };
    if matches!(
        scheme.to_ascii_lowercase().as_str(),
        "https" | "http" | "ssh" | "git" | "file"
    ) {
        Ok(())
    } else {
        Err(format!("不支持的 Git URL scheme: {}", scheme))
    }
}

pub fn sanitize_git_locator(locator: &str) -> String {
    let trimmed = locator.trim();
    let (base, fragment) = trimmed
        .split_once('#')
        .map(|(base, fragment)| (base, Some(fragment)))
        .unwrap_or((trimmed, None));
    let sanitized = sanitize_git_remote_url(base);
    match fragment.filter(|value| !value.is_empty()) {
        Some(fragment) => format!("{}#{}", sanitized, fragment),
        None => sanitized,
    }
}

pub fn sanitize_git_remote_url(locator: &str) -> String {
    let without_query = locator.trim().split('?').next().unwrap_or(locator.trim());
    let Some((scheme, remainder)) = without_query.split_once("://") else {
        if let Some((userinfo, host_path)) = without_query.split_once('@') {
            if userinfo.contains(':') && host_path.contains(':') {
                let (host, path) = host_path.split_once(':').unwrap_or((host_path, ""));
                return format!("ssh://{}/{}", host, path.trim_start_matches('/'));
            }
        }
        return without_query.to_string();
    };
    let authority_end = remainder.find('/').unwrap_or(remainder.len());
    let (authority, path) = remainder.split_at(authority_end);
    let host = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    format!("{}://{}{}", scheme, host, path)
}

fn expand_user_path(path: &str) -> PathBuf {
    if path.starts_with('~') {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(path.trim_start_matches('~').trim_start_matches('/'))
    } else {
        PathBuf::from(path)
    }
}

fn clean_optional_part(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('/');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn validate_git_reference(reference: &str) -> Result<(), String> {
    let value = reference.trim();
    if value.is_empty() {
        return Err("Git reference 不能为空".to_string());
    }
    if value.starts_with('-')
        || value.ends_with('/')
        || value.ends_with('.')
        || value.contains("..")
        || value.contains("@{")
        || value.chars().any(|ch| {
            ch.is_control() || matches!(ch, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
    {
        return Err("Git reference 包含不安全或无效字符".to_string());
    }
    Ok(())
}

fn clean_git_reference(value: &str) -> Result<Option<String>, String> {
    let reference = clean_optional_part(value);
    if let Some(reference) = reference.as_deref() {
        validate_git_reference(reference)?;
    }
    Ok(reference)
}

fn clean_git_subdir(value: &str) -> Result<Option<String>, String> {
    let Some(subdir) = clean_optional_part(value) else {
        return Ok(None);
    };
    let path = Path::new(&subdir);
    if path
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
    {
        Ok(Some(subdir))
    } else {
        Err("Git 子目录不能包含绝对路径或上级目录".to_string())
    }
}

fn git_repo_name(repo_url: &str) -> Result<String, String> {
    let without_fragment = repo_url.split('#').next().unwrap_or(repo_url);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let trimmed = without_query.trim().trim_end_matches('/');
    let candidate = trimmed
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches(".git")
        .trim();
    if candidate.is_empty()
        || candidate == "."
        || candidate == ".."
        || candidate.contains(['/', '\\'])
        || candidate.contains('\0')
    {
        Err("无法从仓库地址推断 Skill 名称".to_string())
    } else {
        Ok(candidate.to_string())
    }
}

fn subdir_target_name(subdir: &str) -> Option<String> {
    Path::new(subdir)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
}

fn parse_git_fragment(fragment: &str) -> Result<(Option<String>, Option<String>), String> {
    let trimmed = fragment.trim();
    if trimmed.is_empty() {
        return Ok((None, None));
    }
    if let Some((reference, subdir)) = trimmed.split_once(':') {
        Ok((clean_git_reference(reference)?, clean_git_subdir(subdir)?))
    } else {
        Ok((clean_git_reference(trimmed)?, None))
    }
}

fn parse_github_tree_url(input: &str) -> Result<Option<GitInstallSpec>, String> {
    let trimmed = input.trim();
    let Some(rest) = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
    else {
        return Ok(None);
    };

    let rest = rest
        .split(['?', '#'])
        .next()
        .unwrap_or(rest)
        .trim_matches('/');
    let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() < 4 || parts[2] != "tree" {
        return Ok(None);
    }

    let owner = parts[0];
    let repo = parts[1].trim_end_matches(".git");
    if parts.len() > 4 && looks_like_multi_segment_branch_prefix(parts[3]) {
        return Err("GitHub tree URL 的分支名可能包含 /，请改用 repo#ref:path 格式".to_string());
    }
    let reference = clean_git_reference(parts[3])?;
    let subdir = clean_git_subdir(&parts[4..].join("/"))?;
    let target_name = subdir
        .as_deref()
        .and_then(subdir_target_name)
        .unwrap_or_else(|| repo.to_string());

    Ok(Some(GitInstallSpec {
        original: trimmed.to_string(),
        repo_url: format!("https://github.com/{}/{}.git", owner, repo),
        reference,
        subdir,
        target_name,
    }))
}

fn looks_like_multi_segment_branch_prefix(value: &str) -> bool {
    matches!(
        value,
        "feature"
            | "feat"
            | "fix"
            | "bugfix"
            | "hotfix"
            | "release"
            | "chore"
            | "deps"
            | "dependabot"
            | "renovate"
    )
}

fn parse_github_shorthand(input: &str) -> Option<GitInstallSpec> {
    let trimmed = input.trim();
    let (repo_part, reference, subdir) = if let Some((repo, fragment)) = trimmed.split_once('#') {
        let (reference, subdir) = parse_git_fragment(fragment).ok()?;
        (repo.trim(), reference, subdir)
    } else {
        (trimmed, None, None)
    };
    let parts: Vec<&str> = repo_part.split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    let valid = parts.iter().all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    });
    if !valid {
        return None;
    }

    let repo = parts[1].trim_end_matches(".git");
    let target_name = subdir
        .as_deref()
        .and_then(subdir_target_name)
        .unwrap_or_else(|| repo.to_string());
    Some(GitInstallSpec {
        original: trimmed.to_string(),
        repo_url: format!("https://github.com/{}/{}.git", parts[0], repo),
        reference,
        subdir,
        target_name,
    })
}

fn looks_like_archive_input(input: &str) -> bool {
    let lower = input.trim().to_ascii_lowercase();
    lower.ends_with(".zip")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.contains("/archive/")
        || lower.contains("/releases/download/")
}

fn looks_like_git_input(input: &str) -> bool {
    let trimmed = input.trim();
    trimmed.starts_with("git@")
        || trimmed.starts_with("ssh://")
        || trimmed.starts_with("git://")
        || trimmed.ends_with(".git")
        || trimmed.contains('#')
        || trimmed.contains("github.com/")
        || trimmed.contains("gitlab.com/")
        || trimmed.contains("bitbucket.org/")
        || parse_github_shorthand(trimmed).is_some()
}

fn install_detection(
    source_kind: impl Into<String>,
    normalized_source: impl Into<String>,
    original_input: impl Into<String>,
    confidence: impl Into<String>,
    warnings: Vec<String>,
    needs_model: bool,
) -> InstallDetection {
    InstallDetection {
        detector: "rules".to_string(),
        source_kind: source_kind.into(),
        normalized_source: normalized_source.into(),
        original_input: original_input.into(),
        repo_url: None,
        reference: None,
        subdir: None,
        target_name: None,
        confidence: confidence.into(),
        warnings,
        needs_model,
    }
}
