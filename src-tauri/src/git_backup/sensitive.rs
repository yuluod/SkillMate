use super::*;

use std::fs::{self};
use std::io::Read;
use std::path::Path;

pub(super) fn read_bounded_regular_file(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SensitiveScan {
    Safe,
    Sensitive,
    Unscannable(&'static str),
}

pub(super) struct ScannedBackupFile {
    pub(super) classification: SensitiveScan,
    pub(super) bytes: Vec<u8>,
    pub(super) permissions: fs::Permissions,
    pub(super) file_size: u64,
}

pub(super) fn backup_exclusion_reason(name: &str) -> Option<&'static str> {
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

pub(super) fn sensitive_name_word(name: &str) -> bool {
    name.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| matches!(part, "token" | "password" | "passwd" | "apikey"))
}

pub(super) fn is_core_skill_file(source_root: &Path, path: &Path) -> bool {
    path.parent() == Some(source_root)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.eq_ignore_ascii_case("SKILL.md"))
            .unwrap_or(false)
}

pub(super) fn unscannable_description(reason: &str) -> &'static str {
    match reason {
        "unscannable_too_large" => "文件超过安全扫描上限",
        "unscannable_binary" => "文件包含二进制或 UTF-16 内容",
        "unscannable_encoding" => "文件不是合法 UTF-8 文本",
        _ => "未知格式",
    }
}

pub(super) fn read_scanned_backup_file(
    path: &Path,
    expected_metadata: &fs::Metadata,
) -> Result<ScannedBackupFile, String> {
    read_scanned_backup_file_with_limit(path, expected_metadata, MAX_SENSITIVE_SCAN_BYTES)
}

pub(super) fn read_scanned_backup_file_with_limit(
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

pub(super) fn classify_backup_bytes(bytes: &[u8]) -> SensitiveScan {
    if bytes.contains(&0) {
        SensitiveScan::Unscannable("unscannable_binary")
    } else {
        match std::str::from_utf8(bytes) {
            Ok(content) => classify_sensitive_content(content),
            Err(_) => SensitiveScan::Unscannable("unscannable_encoding"),
        }
    }
}

pub(super) fn write_scanned_backup_file(target: &Path, scanned: &ScannedBackupFile) -> Result<(), String> {
    fs::write(target, &scanned.bytes).map_err(|error| error.to_string())?;
    fs::set_permissions(target, scanned.permissions.clone()).map_err(|error| error.to_string())
}

#[cfg(unix)]
pub(super) fn same_backup_file_identity(expected: &fs::Metadata, opened: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    expected.dev() == opened.dev() && expected.ino() == opened.ino()
}

#[cfg(not(unix))]
pub(super) fn same_backup_file_identity(expected: &fs::Metadata, opened: &fs::Metadata) -> bool {
    expected.is_file() && opened.is_file() && expected.len() == opened.len()
}

#[cfg(test)]
pub(super) fn scan_backup_file(path: &Path) -> Result<SensitiveScan, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    read_scanned_backup_file(path, &metadata).map(|scanned| scanned.classification)
}

pub(super) fn classify_sensitive_content(content: &str) -> SensitiveScan {
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

pub(super) fn contains_prefixed_secret(content: &str, prefix: &str, minimum_tail: usize) -> bool {
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

pub(super) fn contains_jwt(content: &str) -> bool {
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

pub(super) fn looks_like_secret_value(value: &str) -> bool {
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
