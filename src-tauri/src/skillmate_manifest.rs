use crate::app_core::{
    assistant_root_by_name, atomic_write, expand_path, project_skill_root_by_name,
};
use crate::skill_install::{
    install_target_name, is_git_install_source, parse_git_install_spec, preview_install_source,
    preview_local_symlink_install, InstallPreview, PreviewConflict,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillMateManifest {
    pub version: u32,
    #[serde(default)]
    pub reconcile: bool,
    pub skills: Vec<SkillMateManifestSkill>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillMateManifestSkill {
    pub assistant: String,
    pub source: String,
    pub source_kind: String,
    pub target_name: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub install_mode: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub subdir: Option<String>,
    #[serde(default)]
    pub resolved_ref: Option<String>,
    #[serde(default)]
    pub content_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SkillMateManifestPreview {
    pub can_apply: bool,
    pub validation_issues: Vec<SkillMateManifestIssue>,
    pub actions: Vec<SkillMateManifestAction>,
    pub conflicts: Vec<SkillMateManifestConflict>,
    pub install_previews: Vec<InstallPreview>,
    pub plan_token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SkillMateManifestIssue {
    pub index: usize,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SkillMateManifestAction {
    pub kind: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingTargetDisposition {
    Missing,
    Matching,
    Replaceable,
    Drifted,
    Unmanaged,
}

pub fn read_skillmate_manifest(path: impl AsRef<Path>) -> Result<SkillMateManifest, String> {
    let path = path.as_ref();
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut manifest: SkillMateManifest = toml::from_str(&content).map_err(|e| e.to_string())?;
    let base = absolute_manifest_parent(path)?;
    for skill in &mut manifest.skills {
        if skill.source_kind == "local" {
            skill.source = resolve_manifest_path(&base, &skill.source);
        }
        if let Some(project_path) = skill.project_path.as_deref() {
            skill.project_path = Some(resolve_manifest_path(&base, project_path));
        }
    }
    sort_manifest_skills(&mut manifest.skills);
    Ok(manifest)
}

pub fn write_skillmate_manifest(
    path: impl AsRef<Path>,
    manifest: &SkillMateManifest,
) -> Result<String, String> {
    let path = path.as_ref();
    let base = absolute_manifest_parent(path)?;
    let mut portable = manifest.clone();
    for skill in &mut portable.skills {
        if skill.source_kind == "local" {
            skill.source = portable_manifest_path(&base, &skill.source);
        }
        if let Some(project_path) = skill.project_path.as_deref() {
            skill.project_path = Some(portable_manifest_path(&base, project_path));
        }
    }
    sort_manifest_skills(&mut portable.skills);
    let content = toml::to_string_pretty(&portable).map_err(|e| e.to_string())?;
    atomic_write(path, content.as_bytes())?;
    Ok(format!("已导出到 {}", path.to_string_lossy()))
}

pub fn sort_manifest_skills(skills: &mut [SkillMateManifestSkill]) {
    skills.sort_by(|left, right| manifest_sort_key(left).cmp(&manifest_sort_key(right)));
}

fn manifest_sort_key(
    skill: &SkillMateManifestSkill,
) -> (&str, &str, &str, &str, &str, &str, &str, &str) {
    (
        skill.assistant.as_str(),
        skill.scope.as_deref().unwrap_or("global"),
        skill.project_path.as_deref().unwrap_or(""),
        skill.target_name.as_deref().unwrap_or(""),
        skill.source_kind.as_str(),
        skill.source.as_str(),
        skill.reference.as_deref().unwrap_or(""),
        skill.subdir.as_deref().unwrap_or(""),
    )
}

fn absolute_manifest_parent(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    absolute
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "manifest 路径缺少父目录".to_string())
}

fn resolve_manifest_path(base: &Path, value: &str) -> String {
    let trimmed = value.trim();
    let path = expand_path(trimmed);
    if path.is_absolute() {
        path.to_string_lossy().to_string()
    } else {
        let mut resolved = base.to_path_buf();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    resolved.pop();
                }
                Component::Normal(value) => resolved.push(value),
                Component::Prefix(_) | Component::RootDir => {}
            }
        }
        resolved.to_string_lossy().to_string()
    }
}

fn portable_manifest_path(base: &Path, value: &str) -> String {
    let path = expand_path(value.trim());
    match path.strip_prefix(base) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_string(),
        Ok(relative) => Path::new(".").join(relative).to_string_lossy().to_string(),
        Err(_) => value.to_string(),
    }
}

pub fn preview_skillmate_manifest_with_existing(
    manifest: &SkillMateManifest,
    existing_target: impl Fn(
        &SkillMateManifestSkill,
        &Path,
    ) -> Result<ExistingTargetDisposition, String>,
) -> Result<SkillMateManifestPreview, String> {
    let validation_issues = validate_skillmate_manifest(manifest);
    let mut actions = Vec::new();
    let mut conflicts = Vec::new();
    let mut install_previews = Vec::new();
    let mut planned_targets = HashSet::<PathBuf>::new();

    for (index, skill) in manifest.skills.iter().enumerate() {
        if validation_issues.iter().any(|issue| issue.index == index) {
            continue;
        }
        let target_name = match skill.target_name.clone() {
            Some(name) if !name.trim().is_empty() => name,
            _ => install_target_name(&skill.source, &skill.source_kind)?,
        };
        let target_root = match manifest_target_root(skill) {
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
        let effective_source = resolved_manifest_source(skill)?;
        let mut preview = if skill.install_mode.as_deref() == Some("symlink") {
            preview_local_symlink_install(
                &expand_path(effective_source.trim()),
                &target_root,
                &target_name,
            )
        } else {
            preview_install_source(&effective_source, &skill.source_kind, &target_root)
        };
        let target_path = Path::new(&preview.target_path);
        if preview.package_detection.detected_skills.len() != 1 {
            preview.can_apply = false;
            preview.can_install = false;
            preview.message =
                "Manifest 每条记录必须精确解析为一个 Skill；多 Skill 来源请使用 subdir 分拆声明"
                    .to_string();
            conflicts.push(SkillMateManifestConflict {
                assistant: skill.assistant.clone(),
                source: skill.source.clone(),
                reason: preview.message.clone(),
            });
            install_previews.push(preview);
            continue;
        }
        if let Some(requested_name) = skill.target_name.as_deref() {
            let actual_name = target_path.file_name().and_then(|value| value.to_str());
            if actual_name != Some(requested_name) {
                preview.can_apply = false;
                preview.can_install = false;
                preview.message = format!(
                    "target_name {} 与 Skill 规范名称 {} 不一致",
                    requested_name,
                    actual_name.unwrap_or("未知")
                );
                conflicts.push(SkillMateManifestConflict {
                    assistant: skill.assistant.clone(),
                    source: skill.source.clone(),
                    reason: preview.message.clone(),
                });
                install_previews.push(preview);
                continue;
            }
        }
        if !planned_targets.insert(target_path.to_path_buf()) {
            preview.can_apply = false;
            preview.can_install = false;
            preview.message = "Manifest 包含重复目标路径".to_string();
            conflicts.push(SkillMateManifestConflict {
                assistant: skill.assistant.clone(),
                source: skill.source.clone(),
                reason: preview.message.clone(),
            });
            install_previews.push(preview);
            continue;
        }
        let disposition = existing_target(skill, target_path)?;
        if disposition == ExistingTargetDisposition::Matching {
            preview.can_apply = true;
            preview.can_install = true;
            preview.message = "目标已存在且来源一致，无需写入".to_string();
            preview.conflicts.clear();
            for action in &mut preview.target_actions {
                action.action = "keep".to_string();
                action.reason = "目标已存在且来源一致".to_string();
            }
            actions.push(SkillMateManifestAction {
                kind: "keep".to_string(),
                assistant: skill.assistant.clone(),
                source: skill.source.clone(),
                target_name,
                message: preview.message.clone(),
            });
        } else if disposition == ExistingTargetDisposition::Replaceable {
            preview.can_apply = true;
            preview.can_install = true;
            preview.message = "目标由 SkillMate 管理，将替换为声明的来源".to_string();
            preview.conflicts.clear();
            preview
                .package_detection
                .warnings
                .retain(|warning| warning != "target_exists");
            for action in &mut preview.target_actions {
                action.action = "replace".to_string();
                action.reason = "替换来源或版本已变化的受管 Skill".to_string();
            }
            actions.push(SkillMateManifestAction {
                kind: "replace".to_string(),
                assistant: skill.assistant.clone(),
                source: skill.source.clone(),
                target_name,
                message: preview.message.clone(),
            });
        } else if disposition == ExistingTargetDisposition::Drifted {
            preview.can_apply = false;
            preview.can_install = false;
            preview.message = "受管 Skill 包含本地修改，拒绝自动覆盖".to_string();
            preview.conflicts.clear();
            preview.conflicts.push(PreviewConflict {
                target: target_path.to_string_lossy().to_string(),
                reason: "managed_content_changed".to_string(),
            });
            conflicts.push(SkillMateManifestConflict {
                assistant: skill.assistant.clone(),
                source: skill.source.clone(),
                reason: preview.message.clone(),
            });
        } else if preview.can_apply {
            actions.push(SkillMateManifestAction {
                kind: "install".to_string(),
                assistant: skill.assistant.clone(),
                source: skill.source.clone(),
                target_name,
                message: preview.message.clone(),
            });
        } else {
            if disposition == ExistingTargetDisposition::Unmanaged {
                preview.message = "目标目录已存在且不属于 SkillMate，拒绝覆盖".to_string();
            }
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
        plan_token: String::new(),
    })
}

pub fn manifest_target_root(skill: &SkillMateManifestSkill) -> Result<std::path::PathBuf, String> {
    match skill.scope.as_deref().filter(|scope| !scope.is_empty()) {
        None | Some("global") => assistant_root_by_name(&skill.assistant),
        Some("project") => {
            let project_path = skill
                .project_path
                .as_deref()
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .ok_or_else(|| "项目级 Skill 缺少 project_path".to_string())?;
            Ok(project_skill_root_by_name(
                &skill.assistant,
                &expand_path(project_path),
            )?)
        }
        Some(_) => Err("scope 仅支持 global/project".to_string()),
    }
}

pub fn resolved_manifest_source(skill: &SkillMateManifestSkill) -> Result<String, String> {
    if !is_git_install_source(&skill.source_kind)
        || skill.source.contains('#')
        || (skill.reference.is_none() && skill.resolved_ref.is_none() && skill.subdir.is_none())
    {
        return Ok(skill.source.clone());
    }
    let reference = skill
        .resolved_ref
        .as_deref()
        .or(skill.reference.as_deref())
        .unwrap_or("");
    let subdir = skill.subdir.as_deref().unwrap_or("");
    if reference.is_empty() && !subdir.is_empty() {
        return Err("Git 子目录需要同时声明 reference 或 resolved_ref".to_string());
    }
    Ok(if subdir.is_empty() {
        format!("{}#{}", skill.source, reference)
    } else {
        format!("{}#{}:{}", skill.source, reference, subdir)
    })
}

pub fn validate_skillmate_manifest(manifest: &SkillMateManifest) -> Vec<SkillMateManifestIssue> {
    let mut issues = Vec::new();
    if !matches!(manifest.version, 1 | 2) {
        issues.push(issue(
            0,
            "unsupported_manifest_version",
            "不支持的 SkillMate manifest 版本",
        ));
    }
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
        match skill.scope.as_deref().filter(|scope| !scope.is_empty()) {
            None | Some("global") => {}
            Some("project") => {
                if skill
                    .project_path
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
                {
                    issues.push(issue(
                        index,
                        "missing_project_path",
                        "项目级 Skill 缺少 project_path",
                    ));
                }
            }
            Some(_) => issues.push(issue(index, "invalid_scope", "scope 仅支持 global/project")),
        }
        match skill
            .install_mode
            .as_deref()
            .filter(|mode| !mode.is_empty())
        {
            None | Some("copy") => {}
            Some("symlink") if skill.source_kind == "local" => {}
            Some("symlink") => issues.push(issue(
                index,
                "invalid_symlink_source",
                "symlink 安装只支持本地来源",
            )),
            Some(_) => issues.push(issue(
                index,
                "invalid_install_mode",
                "install_mode 仅支持 copy/symlink",
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
    fn manifest_write_is_deterministic_and_project_relative() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-portable-manifest-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let source = root.join("skills/writer");
        let path = root.join("skillmate.toml");
        fs::create_dir_all(&source).unwrap();
        let manifest = SkillMateManifest {
            version: 2,
            reconcile: true,
            skills: vec![
                SkillMateManifestSkill {
                    assistant: "Gemini CLI".to_string(),
                    source: source.to_string_lossy().to_string(),
                    source_kind: "local".to_string(),
                    target_name: Some("writer".to_string()),
                    scope: Some("project".to_string()),
                    project_path: Some(root.to_string_lossy().to_string()),
                    ..Default::default()
                },
                SkillMateManifestSkill {
                    assistant: "Codex".to_string(),
                    source: "owner/repo".to_string(),
                    source_kind: "git".to_string(),
                    target_name: Some("reviewer".to_string()),
                    scope: Some("project".to_string()),
                    project_path: Some(root.to_string_lossy().to_string()),
                    ..Default::default()
                },
            ],
        };

        write_skillmate_manifest(&path, &manifest).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.find("Codex").unwrap() < content.find("Gemini CLI").unwrap());
        assert!(content.contains("project_path = \".\""));

        let restored = read_skillmate_manifest(&path).unwrap();
        assert_eq!(restored.skills[0].assistant, "Codex");
        assert_eq!(
            restored.skills[0].project_path.as_deref(),
            Some(root.to_string_lossy().as_ref())
        );
        assert_eq!(restored.skills[1].source, source.to_string_lossy());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn validates_manifest_records() {
        let manifest = SkillMateManifest {
            version: 1,
            reconcile: false,
            skills: vec![SkillMateManifestSkill {
                assistant: "".to_string(),
                source: "/definitely/missing/skill".to_string(),
                source_kind: "local".to_string(),
                target_name: Some("../bad".to_string()),
                ..Default::default()
            }],
        };

        let issues = validate_skillmate_manifest(&manifest);

        assert!(issues.iter().any(|issue| issue.code == "missing_assistant"));
        assert!(issues.iter().any(|issue| issue.code == "source_missing"));
        assert!(issues
            .iter()
            .any(|issue| issue.code == "invalid_target_name"));
    }

    #[test]
    fn rejects_unknown_manifest_version_before_reconcile() {
        let manifest = SkillMateManifest {
            version: 999,
            reconcile: true,
            skills: vec![],
        };

        let issues = validate_skillmate_manifest(&manifest);

        assert!(issues
            .iter()
            .any(|issue| issue.code == "unsupported_manifest_version"));
    }

    #[test]
    fn existing_matching_target_is_an_idempotent_keep_action() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-manifest-idempotent-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let source = root.join("writer");
        let project = root.join("project");
        let target = project_skill_root_by_name("Codex", &project)
            .unwrap()
            .join("writer");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: writer\ndescription: 写作\n---\n",
        )
        .unwrap();
        fs::write(
            target.join("SKILL.md"),
            "---\nname: writer\ndescription: 写作\n---\n",
        )
        .unwrap();
        let manifest = SkillMateManifest {
            version: 2,
            reconcile: false,
            skills: vec![SkillMateManifestSkill {
                assistant: "Codex".to_string(),
                source: source.to_string_lossy().to_string(),
                source_kind: "local".to_string(),
                target_name: Some("writer".to_string()),
                scope: Some("project".to_string()),
                install_mode: Some("copy".to_string()),
                project_path: Some(project.to_string_lossy().to_string()),
                ..Default::default()
            }],
        };

        let preview = preview_skillmate_manifest_with_existing(&manifest, |_, path| {
            Ok(if path == target {
                ExistingTargetDisposition::Matching
            } else {
                ExistingTargetDisposition::Missing
            })
        })
        .unwrap();

        assert!(preview.can_apply);
        assert_eq!(preview.actions[0].kind, "keep");
        assert!(preview.conflicts.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_rejects_ambiguous_multi_skill_source() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-manifest-multi-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        for name in ["writer", "reviewer"] {
            let skill = root.join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: test\n---\n"),
            )
            .unwrap();
        }
        let manifest = SkillMateManifest {
            version: 2,
            reconcile: false,
            skills: vec![SkillMateManifestSkill {
                assistant: "Codex".to_string(),
                source: root.to_string_lossy().to_string(),
                source_kind: "local".to_string(),
                ..Default::default()
            }],
        };

        let preview = preview_skillmate_manifest_with_existing(&manifest, |_, _| {
            Ok(ExistingTargetDisposition::Missing)
        })
        .unwrap();

        assert!(!preview.can_apply);
        assert!(preview.conflicts[0].reason.contains("精确解析为一个 Skill"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_rejects_target_name_that_breaks_skill_identity() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-manifest-name-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let source = root.join("writer");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: writer\ndescription: test\n---\n",
        )
        .unwrap();
        let manifest = SkillMateManifest {
            version: 2,
            reconcile: false,
            skills: vec![SkillMateManifestSkill {
                assistant: "Codex".to_string(),
                source: source.to_string_lossy().to_string(),
                source_kind: "local".to_string(),
                target_name: Some("renamed".to_string()),
                ..Default::default()
            }],
        };

        let preview = preview_skillmate_manifest_with_existing(&manifest, |_, _| {
            Ok(ExistingTargetDisposition::Missing)
        })
        .unwrap();

        assert!(!preview.can_apply);
        assert!(preview.conflicts[0].reason.contains("target_name"));
        let _ = fs::remove_dir_all(root);
    }
}
