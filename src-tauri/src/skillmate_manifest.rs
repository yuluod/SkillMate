use crate::app_core::{assistant_root_by_name, expand_path};
use crate::skill_install::{
    install_target_name, is_git_install_source, parse_git_install_spec, preview_install_source,
    InstallPreview,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillMateManifest {
    pub version: u32,
    pub skills: Vec<SkillMateManifestSkill>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillMateManifestSkill {
    pub assistant: String,
    pub source: String,
    pub source_kind: String,
    pub target_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SkillMateManifestPreview {
    pub can_apply: bool,
    pub validation_issues: Vec<SkillMateManifestIssue>,
    pub actions: Vec<SkillMateManifestAction>,
    pub conflicts: Vec<SkillMateManifestConflict>,
    pub install_previews: Vec<InstallPreview>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SkillMateManifestIssue {
    pub index: usize,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SkillMateManifestAction {
    pub assistant: String,
    pub source: String,
    pub target_name: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SkillMateManifestConflict {
    pub assistant: String,
    pub source: String,
    pub reason: String,
}

pub fn read_skillmate_manifest(path: impl AsRef<Path>) -> Result<SkillMateManifest, String> {
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&content).map_err(|e| e.to_string())
}

pub fn write_skillmate_manifest(
    path: impl AsRef<Path>,
    manifest: &SkillMateManifest,
) -> Result<String, String> {
    let content = toml::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    fs::write(path, content).map_err(|e| e.to_string())?;
    Ok("已导出 skillmate.toml".to_string())
}

pub fn preview_skillmate_manifest(
    manifest: &SkillMateManifest,
) -> Result<SkillMateManifestPreview, String> {
    let validation_issues = validate_skillmate_manifest(manifest);
    let mut actions = Vec::new();
    let mut conflicts = Vec::new();
    let mut install_previews = Vec::new();

    for (index, skill) in manifest.skills.iter().enumerate() {
        if validation_issues.iter().any(|issue| issue.index == index) {
            continue;
        }
        let target_name = match skill.target_name.clone() {
            Some(name) if !name.trim().is_empty() => name,
            _ => install_target_name(&skill.source, &skill.source_kind)?,
        };
        let target_root = match assistant_root_by_name(&skill.assistant) {
            Ok(path) => path,
            Err(err) => {
                conflicts.push(SkillMateManifestConflict {
                    assistant: skill.assistant.clone(),
                    source: skill.source.clone(),
                    reason: err,
                });
                continue;
            }
        };
        let preview = preview_install_source(&skill.source, &skill.source_kind, &target_root);
        if preview.can_apply {
            actions.push(SkillMateManifestAction {
                assistant: skill.assistant.clone(),
                source: skill.source.clone(),
                target_name,
                message: preview.message.clone(),
            });
        } else {
            conflicts.push(SkillMateManifestConflict {
                assistant: skill.assistant.clone(),
                source: skill.source.clone(),
                reason: preview.message.clone(),
            });
        }
        install_previews.push(preview);
    }

    Ok(SkillMateManifestPreview {
        can_apply: validation_issues.is_empty() && conflicts.is_empty(),
        validation_issues,
        actions,
        conflicts,
        install_previews,
    })
}

pub fn validate_skillmate_manifest(manifest: &SkillMateManifest) -> Vec<SkillMateManifestIssue> {
    let mut issues = Vec::new();
    for (index, skill) in manifest.skills.iter().enumerate() {
        if skill.assistant.trim().is_empty() {
            issues.push(issue(index, "missing_assistant", "缺少 assistant"));
        } else if assistant_root_by_name(&skill.assistant).is_err() {
            issues.push(issue(index, "unsupported_assistant", "不支持的 assistant"));
        }
        if skill.source.trim().is_empty() {
            issues.push(issue(index, "missing_source", "缺少 source"));
        }
        match skill.source_kind.as_str() {
            "local" => {
                if !skill.source.trim().is_empty() && !expand_path(skill.source.trim()).exists() {
                    issues.push(issue(index, "source_missing", "本地来源路径不存在"));
                }
            }
            kind if is_git_install_source(kind) => {
                if !skill.source.trim().is_empty() && parse_git_install_spec(&skill.source).is_err()
                {
                    issues.push(issue(index, "invalid_git_source", "Git 来源格式无效"));
                }
            }
            "archive" => issues.push(issue(index, "archive_unsupported", "暂不支持压缩包来源")),
            _ => issues.push(issue(
                index,
                "unsupported_source_kind",
                "source_kind 仅支持 git/local",
            )),
        }
        if let Some(target_name) = skill.target_name.as_deref() {
            if target_name.contains(['/', '\\']) || target_name == "." || target_name == ".." {
                issues.push(issue(
                    index,
                    "invalid_target_name",
                    "target_name 不能包含路径分隔符",
                ));
            }
        }
    }
    issues
}

fn issue(index: usize, code: &str, message: &str) -> SkillMateManifestIssue {
    SkillMateManifestIssue {
        index,
        code: code.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_skillmate_manifest() {
        let manifest: SkillMateManifest = toml::from_str(
            r#"
version = 1

[[skills]]
assistant = "Codex"
source = "/tmp/writer"
source_kind = "local"
target_name = "writer"
"#,
        )
        .unwrap();

        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.skills.len(), 1);
        assert_eq!(manifest.skills[0].assistant, "Codex");
        assert_eq!(manifest.skills[0].target_name, Some("writer".to_string()));
    }

    #[test]
    fn validates_manifest_records() {
        let manifest = SkillMateManifest {
            version: 1,
            skills: vec![SkillMateManifestSkill {
                assistant: "".to_string(),
                source: "/definitely/missing/skill".to_string(),
                source_kind: "local".to_string(),
                target_name: Some("../bad".to_string()),
            }],
        };

        let issues = validate_skillmate_manifest(&manifest);

        assert!(issues.iter().any(|issue| issue.code == "missing_assistant"));
        assert!(issues.iter().any(|issue| issue.code == "source_missing"));
        assert!(issues
            .iter()
            .any(|issue| issue.code == "invalid_target_name"));
    }
}
