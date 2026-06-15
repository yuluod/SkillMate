use crate::app_core::{assistant_definitions, expand_path, format_size};
use crate::managed_state::{is_managed_by_state, managed_state_origin, STATE_FILE_NAME};
use crate::skill_origin::build_sync_info;
use crate::skill_structure::{analyze_skill_structure, read_skill_preview};
use crate::{AIAssistant, Skill};
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ManagedSkill {
    pub assistant: String,
    pub path: PathBuf,
    pub name: String,
}

pub fn scan_all_assistants(db: &Connection) -> Vec<AIAssistant> {
    assistant_definitions()
        .iter()
        .map(|assistant| {
            let expanded = expand_path(assistant.path);
            let exists = expanded.exists();
            let mut skills = Vec::new();
            if exists {
                if let Ok(entries) = fs::read_dir(&expanded) {
                    for entry in entries.flatten() {
                        let ep = entry.path();
                        let nm = ep
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if nm == STATE_FILE_NAME {
                            continue;
                        }
                        let managed = ManagedSkill {
                            assistant: assistant.name.to_string(),
                            path: ep.clone(),
                            name: nm.clone(),
                        };
                        skills.push(build_skill(db, &managed));
                    }
                }
            }
            AIAssistant {
                name: assistant.name.to_string(),
                path: expanded.to_string_lossy().to_string(),
                ai_type: assistant.ai_type.to_string(),
                icon: assistant.icon.to_string(),
                skills,
                exists,
            }
        })
        .collect()
}

pub fn collect_known_skill_paths(db: &Connection) -> Vec<String> {
    scan_all_assistants(db)
        .into_iter()
        .flat_map(|assistant| assistant.skills.into_iter().map(|skill| skill.path))
        .collect()
}

fn build_skill(db: &Connection, managed: &ManagedSkill) -> Skill {
    let ep = &managed.path;
    let (modified, size) = ep
        .metadata()
        .map(|m| {
            (
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs().to_string())
                    .unwrap_or_default(),
                m.len(),
            )
        })
        .unwrap_or((String::new(), 0));
    let tags_str: String = db
        .query_row(
            "SELECT tags FROM skill_tags WHERE skill_path = ?",
            [ep.to_string_lossy().to_string()],
            |r| r.get(0),
        )
        .unwrap_or_default();
    let tags: Vec<String> = if tags_str.is_empty() {
        vec![]
    } else {
        tags_str.split(',').map(|s| s.to_string()).collect()
    };
    let sync_info = build_sync_info(db, ep);
    let state_origin = ep.parent().and_then(|root| managed_state_origin(root, ep));
    let state_managed = state_origin.is_some()
        || ep
            .parent()
            .map(|root| is_managed_by_state(root, ep))
            .unwrap_or(false);
    let symlink_source = state_origin
        .as_deref()
        .and_then(|origin| origin.strip_prefix("symlink:"))
        .map(|source| source.to_string());
    let structure = analyze_skill_structure(ep);
    let upstream_url = if !sync_info.meta.resolved_locator.is_empty() {
        sync_info.meta.resolved_locator.clone()
    } else {
        sync_info.meta.origin_locator.clone()
    };
    let source_type = if symlink_source.is_some() {
        "symlink".to_string()
    } else {
        sync_info.meta.origin_kind.clone()
    };
    let source = match source_type.as_str() {
        "symlink" => "项目软连接".to_string(),
        "git" if upstream_url.contains("github.com") => "GitHub".to_string(),
        "git" => "Git".to_string(),
        "legacy_npm" | "npm" => "npm（历史）".to_string(),
        "legacy_pip" | "pip" => "PyPI（历史）".to_string(),
        "local" => "Local".to_string(),
        _ => "未托管".to_string(),
    };
    Skill {
        id: ep.to_string_lossy().to_string(),
        name: managed.name.clone(),
        path: ep.to_string_lossy().to_string(),
        skill_type: if ep.is_dir() {
            "skill-folder".to_string()
        } else {
            "skill-file".to_string()
        },
        source,
        source_type,
        size: format_size(size),
        modified,
        tags,
        description: String::new(),
        readme: read_skill_preview(ep),
        version: "1.0.0".to_string(),
        upstream_url,
        has_update: sync_info.has_update,
        compatible_with: vec![managed.assistant.clone()],
        usage_count: 0,
        origin_kind: sync_info.meta.origin_kind,
        origin_locator: sync_info.meta.origin_locator,
        resolved_locator: sync_info.meta.resolved_locator,
        tracking_ref: sync_info.meta.tracking_ref,
        installed_ref: sync_info.meta.installed_ref,
        latest_ref: sync_info.meta.latest_ref,
        sync_state: sync_info.meta.sync_state,
        sync_message: sync_info.meta.sync_message,
        lag_count: sync_info.meta.lag_count,
        last_probe_at: sync_info.meta.last_probe_at,
        last_sync_at: sync_info.meta.last_sync_at,
        managed_by_app: sync_info.meta.managed_by_app || state_managed,
        can_sync: sync_info.can_sync,
        symlink_source,
        structure_status: structure.structure_status,
        structure_features: structure.structure_features,
        structure_warnings: structure.structure_warnings,
        manifest_title: structure.manifest_title,
        manifest_description: structure.manifest_description,
    }
}
