use crate::app_core::{atomic_write, generate_id};
use crate::skillmate_manifest::{SkillMateManifestPreview, SkillMateManifestSkill};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const PROFILE_STORE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillSetProfileStore {
    pub version: u32,
    pub active_profile_id: Option<String>,
    pub previous_active_profile_id: Option<String>,
    pub profiles: Vec<SkillSetProfile>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillSetProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub active: bool,
    pub skills: Vec<SkillMateManifestSkill>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SkillSetProfilePreview {
    pub profile: SkillSetProfile,
    pub profile_issues: Vec<SkillSetProfileIssue>,
    pub diff: SkillSetProfileDiff,
    pub manifest_preview: SkillMateManifestPreview,
    pub plan_token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SkillSetProfileIssue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillSetProfileDiff {
    pub to_install: Vec<String>,
    pub already_present: Vec<String>,
    pub to_remove: Vec<String>,
    pub conflicts: Vec<String>,
}

pub fn read_skill_profiles() -> Result<SkillSetProfileStore, String> {
    let path = profiles_path();
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(empty_store()),
        Err(error) => return Err(format!("无法读取 Profile 状态: {}", error)),
    };
    let store: SkillSetProfileStore = serde_json::from_str(&content)
        .map_err(|error| format!("Profile 状态文件损坏 {}: {}", path.to_string_lossy(), error))?;
    ensure_supported_store_version(&store)?;
    Ok(normalize_store(store))
}

pub fn write_skill_profiles(store: &SkillSetProfileStore) -> Result<(), String> {
    ensure_supported_store_version(store)?;
    let path = profiles_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content =
        serde_json::to_string_pretty(&normalize_store(store.clone())).map_err(|e| e.to_string())?;
    atomic_write(&path, content.as_bytes())
}

pub fn upsert_skill_profile(
    name: &str,
    description: &str,
    skills: Vec<SkillMateManifestSkill>,
) -> Result<SkillSetProfileStore, String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err("Profile 名称不能为空".to_string());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let mut store = read_skill_profiles()?;
    if let Some(profile) = store
        .profiles
        .iter_mut()
        .find(|profile| profile.name == trimmed_name)
    {
        profile.description = description.trim().to_string();
        profile.skills = skills;
        profile.updated_at = now;
    } else {
        store.profiles.push(SkillSetProfile {
            id: format!("profile-{}", generate_id()),
            name: trimmed_name.to_string(),
            description: description.trim().to_string(),
            active: false,
            skills,
            created_at: now.clone(),
            updated_at: now,
        });
    }
    write_skill_profiles(&store)?;
    read_skill_profiles()
}

pub fn set_active_profile(profile_id: &str) -> Result<SkillSetProfileStore, String> {
    let mut store = read_skill_profiles()?;
    if !store
        .profiles
        .iter()
        .any(|profile| profile.id == profile_id)
    {
        return Err("Profile 不存在".to_string());
    }
    if store.active_profile_id.as_deref() != Some(profile_id) {
        store.previous_active_profile_id = store.active_profile_id.clone();
    }
    store.active_profile_id = Some(profile_id.to_string());
    for profile in &mut store.profiles {
        profile.active = profile.id == profile_id;
    }
    write_skill_profiles(&store)?;
    read_skill_profiles()
}

pub fn rollback_active_profile() -> Result<String, String> {
    let store = read_skill_profiles()?;
    let (store, previous_profile_id) = rollback_active_profile_store(store)?;
    write_skill_profiles(&store)?;
    Ok(previous_profile_id)
}

pub fn previous_active_profile_id() -> Result<String, String> {
    let store = read_skill_profiles()?;
    let previous_profile_id = store
        .previous_active_profile_id
        .ok_or_else(|| "没有可回滚的上一个 Profile".to_string())?;
    if store
        .profiles
        .iter()
        .any(|profile| profile.id == previous_profile_id)
    {
        Ok(previous_profile_id)
    } else {
        Err("上一个 Profile 不存在，无法回滚".to_string())
    }
}

fn rollback_active_profile_store(
    mut store: SkillSetProfileStore,
) -> Result<(SkillSetProfileStore, String), String> {
    let previous_profile_id = store
        .previous_active_profile_id
        .clone()
        .ok_or_else(|| "没有可回滚的上一个 Profile".to_string())?;
    if !store
        .profiles
        .iter()
        .any(|profile| profile.id == previous_profile_id)
    {
        return Err("上一个 Profile 不存在，无法回滚".to_string());
    }
    store.active_profile_id = Some(previous_profile_id.clone());
    store.previous_active_profile_id = None;
    for profile in &mut store.profiles {
        profile.active = profile.id == previous_profile_id;
    }
    Ok((normalize_store(store), previous_profile_id))
}

pub fn validate_skill_profile(
    profile: &SkillSetProfile,
    profiles: &[SkillSetProfile],
) -> Vec<SkillSetProfileIssue> {
    let mut issues = Vec::new();
    if profile.id.trim().is_empty() {
        issues.push(profile_issue("missing_id", "Profile 缺少 id"));
    }
    if profile.name.trim().is_empty() {
        issues.push(profile_issue("missing_name", "Profile 名称不能为空"));
    }
    if profile.skills.is_empty() {
        issues.push(profile_issue(
            "empty_skills",
            "Profile 至少需要包含一条 Skill 记录",
        ));
    }
    if !profile.id.trim().is_empty()
        && profiles
            .iter()
            .filter(|candidate| candidate.id == profile.id)
            .count()
            > 1
    {
        issues.push(profile_issue("duplicate_id", "Profile id 重复"));
    }
    issues
}

fn profile_issue(code: &str, message: &str) -> SkillSetProfileIssue {
    SkillSetProfileIssue {
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn profiles_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("skillmate")
        .join("skill-profiles.json")
}

fn empty_store() -> SkillSetProfileStore {
    SkillSetProfileStore {
        version: PROFILE_STORE_VERSION,
        active_profile_id: None,
        previous_active_profile_id: None,
        profiles: vec![],
    }
}

fn normalize_store(mut store: SkillSetProfileStore) -> SkillSetProfileStore {
    store.version = PROFILE_STORE_VERSION;
    if let Some(active_id) = store.active_profile_id.clone() {
        for profile in &mut store.profiles {
            profile.active = profile.id == active_id;
        }
    } else {
        for profile in &mut store.profiles {
            profile.active = false;
        }
    }
    if let Some(previous_id) = store.previous_active_profile_id.clone() {
        if !store
            .profiles
            .iter()
            .any(|profile| profile.id == previous_id)
        {
            store.previous_active_profile_id = None;
        }
    }
    store
}

fn ensure_supported_store_version(store: &SkillSetProfileStore) -> Result<(), String> {
    if store.version > PROFILE_STORE_VERSION {
        Err(format!(
            "Profile 状态版本 {} 高于当前支持版本 {}，请升级 SkillMate 后重试",
            store.version, PROFILE_STORE_VERSION
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_active_profile_flag() {
        let store = normalize_store(SkillSetProfileStore {
            version: 0,
            active_profile_id: Some("p2".to_string()),
            previous_active_profile_id: None,
            profiles: vec![
                SkillSetProfile {
                    id: "p1".to_string(),
                    name: "A".to_string(),
                    ..Default::default()
                },
                SkillSetProfile {
                    id: "p2".to_string(),
                    name: "B".to_string(),
                    ..Default::default()
                },
            ],
        });

        assert_eq!(store.version, 1);
        assert!(!store.profiles[0].active);
        assert!(store.profiles[1].active);
    }

    #[test]
    fn validates_profile_schema() {
        let profile = SkillSetProfile {
            id: "p1".to_string(),
            name: "".to_string(),
            skills: vec![],
            ..Default::default()
        };
        let profiles = vec![
            profile.clone(),
            SkillSetProfile {
                id: "p1".to_string(),
                name: "重复".to_string(),
                ..Default::default()
            },
        ];

        let issues = validate_skill_profile(&profile, &profiles);

        assert!(issues.iter().any(|issue| issue.code == "missing_name"));
        assert!(issues.iter().any(|issue| issue.code == "empty_skills"));
        assert!(issues.iter().any(|issue| issue.code == "duplicate_id"));
    }

    #[test]
    fn rejects_profile_store_from_newer_skillmate() {
        let store = SkillSetProfileStore {
            version: 999,
            ..Default::default()
        };

        assert!(ensure_supported_store_version(&store).is_err());
    }

    #[test]
    fn rollback_active_profile_store_restores_previous_profile() {
        let store = SkillSetProfileStore {
            version: 1,
            active_profile_id: Some("p2".to_string()),
            previous_active_profile_id: Some("p1".to_string()),
            profiles: vec![
                SkillSetProfile {
                    id: "p1".to_string(),
                    name: "A".to_string(),
                    ..Default::default()
                },
                SkillSetProfile {
                    id: "p2".to_string(),
                    name: "B".to_string(),
                    active: true,
                    ..Default::default()
                },
            ],
        };

        let (store, previous_id) = rollback_active_profile_store(store).unwrap();

        assert_eq!(previous_id, "p1");
        assert_eq!(store.active_profile_id.as_deref(), Some("p1"));
        assert_eq!(store.previous_active_profile_id, None);
        assert!(store.profiles[0].active);
        assert!(!store.profiles[1].active);
    }

    #[test]
    fn rollback_active_profile_store_requires_previous_profile() {
        let err = rollback_active_profile_store(SkillSetProfileStore {
            version: 1,
            active_profile_id: Some("p1".to_string()),
            previous_active_profile_id: None,
            profiles: vec![SkillSetProfile {
                id: "p1".to_string(),
                name: "A".to_string(),
                ..Default::default()
            }],
        })
        .unwrap_err();

        assert_eq!(err, "没有可回滚的上一个 Profile");
    }

    #[test]
    fn rollback_active_profile_store_rejects_missing_previous_profile() {
        let err = rollback_active_profile_store(SkillSetProfileStore {
            version: 1,
            active_profile_id: Some("p2".to_string()),
            previous_active_profile_id: Some("missing".to_string()),
            profiles: vec![SkillSetProfile {
                id: "p2".to_string(),
                name: "B".to_string(),
                active: true,
                ..Default::default()
            }],
        })
        .unwrap_err();

        assert_eq!(err, "上一个 Profile 不存在，无法回滚");
    }

    #[test]
    fn rollback_active_profile_store_rejects_repeated_rollback() {
        let (store, _) = rollback_active_profile_store(SkillSetProfileStore {
            version: 1,
            active_profile_id: Some("p2".to_string()),
            previous_active_profile_id: Some("p1".to_string()),
            profiles: vec![
                SkillSetProfile {
                    id: "p1".to_string(),
                    name: "A".to_string(),
                    ..Default::default()
                },
                SkillSetProfile {
                    id: "p2".to_string(),
                    name: "B".to_string(),
                    active: true,
                    ..Default::default()
                },
            ],
        })
        .unwrap();

        let err = rollback_active_profile_store(store).unwrap_err();

        assert_eq!(err, "没有可回滚的上一个 Profile");
    }
}
