use crate::app_core::{assistant_definitions, format_size, AssistantDefinition};
use crate::managed_installation::{list_managed_installations, ManagedInstallation};
use crate::managed_state::{content_fingerprint, managed_state_entry, STATE_FILE_NAME};
use crate::skill_origin::{build_sync_info_with_cache, OriginInferenceCache};
use crate::skill_structure::{detect_skill_entry, inspect_skill_for_inventory, SkillEntryKind};
use crate::{
    AIAssistant, Skill, SkillInventoryFields, SkillOriginFields, SkillScanDiagnostic,
    SkillStructureFields,
};
use rusqlite::{Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ManagedSkill {
    pub path: PathBuf,
    pub name: String,
}

fn inventory_cache_key(path: &Path) -> PathBuf {
    path.to_path_buf()
}

pub fn scan_all_assistants(db: &Connection) -> Result<Vec<AIAssistant>, String> {
    let managed_installations = list_managed_installations(db)?;
    let mut skill_cache = HashMap::<PathBuf, Skill>::new();
    let mut origin_cache = OriginInferenceCache::default();
    let mut assistants = Vec::new();
    for assistant in assistant_definitions() {
        let expanded = assistant.global_install_root();
        let discovery_roots = assistant.global_discovery_roots().collect::<Vec<_>>();
        let (mut entries, diagnostics) = collect_assistant_skill_entries(assistant);
        let mut seen = entries.iter().cloned().collect::<HashSet<_>>();
        append_managed_installation_paths(
            assistant.name,
            &managed_installations,
            &mut entries,
            &mut seen,
        );
        let exists = discovery_roots.iter().any(|root| root.exists()) || !entries.is_empty();
        let mut skills = Vec::new();
        for path in entries {
            // 完整 Skill 包含安装位置相关状态，不能按软连接目标复用，否则会串用其他位置的路径和受管信息。
            let identity = inventory_cache_key(&path);
            if let Some(cached) = skill_cache.get(&identity) {
                skills.push(cached.clone());
                continue;
            }
            let name = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            let managed = ManagedSkill { path, name };
            let skill = build_skill(db, &managed, &mut origin_cache);
            skill_cache.insert(identity, skill.clone());
            skills.push(skill);
        }
        skills.sort_by(|left, right| left.inventory.path.cmp(&right.inventory.path));
        let mut active_roots = discovery_roots
            .iter()
            .filter(|root| root.exists())
            .cloned()
            .collect::<Vec<_>>();
        active_roots.extend(
            managed_installations
                .iter()
                .filter(|installation| installation.skill.assistant == assistant.name)
                .filter_map(|installation| installation.path.parent().map(Path::to_path_buf)),
        );
        active_roots.sort();
        active_roots.dedup();
        if active_roots.is_empty() {
            active_roots.push(expanded.clone());
        }
        let display_root = if expanded.exists() {
            expanded
        } else {
            active_roots[0].clone()
        };
        assistants.push(AIAssistant {
            name: assistant.name.to_string(),
            path: display_root.to_string_lossy().to_string(),
            paths: active_roots
                .into_iter()
                .map(|root| root.to_string_lossy().to_string())
                .collect(),
            ai_type: assistant.ai_type.to_string(),
            icon: assistant.icon.to_string(),
            skills,
            diagnostics,
            exists,
        });
    }
    Ok(assistants)
}

fn append_managed_installation_paths(
    assistant_name: &str,
    installations: &[ManagedInstallation],
    entries: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    for installation in installations
        .iter()
        .filter(|installation| installation.skill.assistant == assistant_name)
    {
        if (installation.path.exists() || fs::symlink_metadata(&installation.path).is_ok())
            && seen.insert(installation.path.clone())
        {
            entries.push(installation.path.clone());
        }
    }
}

fn collect_assistant_skill_entries(
    assistant: &AssistantDefinition,
) -> (Vec<PathBuf>, Vec<SkillScanDiagnostic>) {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut diagnostics = Vec::new();
    for root in assistant.global_discovery_roots() {
        collect_skill_entries(
            &root,
            assistant.recursive_discovery_depth(),
            &mut entries,
            &mut seen,
            &mut diagnostics,
        );
    }
    (entries, diagnostics)
}

fn collect_skill_entries(
    root: &Path,
    remaining_depth: usize,
    output: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    diagnostics: &mut Vec<SkillScanDiagnostic>,
) {
    if remaining_depth == 0 || !root.exists() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        diagnostics.push(scan_diagnostic(
            root,
            "scan_unavailable",
            "目录无法读取，已跳过扫描",
        ));
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == STATE_FILE_NAME || !should_scan_skill_entry(&path, &name) {
            continue;
        }
        if !path.is_dir() {
            diagnostics.push(scan_diagnostic(
                &path,
                "unsupported_root_file",
                "Skills 根目录中的普通文件不会作为 Skill 展示",
            ));
            continue;
        }
        if has_entry_document(&path) {
            if seen.insert(path.clone()) {
                output.push(path);
            }
            continue;
        }
        if remaining_depth == 1 {
            diagnostics.push(scan_diagnostic(
                &path,
                "missing_entry_document",
                "目录中未识别到 SKILL.md、skill.md 或 README.md",
            ));
            continue;
        }
        let count_before = output.len();
        collect_skill_entries(&path, remaining_depth - 1, output, seen, diagnostics);
        if output.len() == count_before {
            diagnostics.push(scan_diagnostic(
                &path,
                "missing_entry_document",
                "目录及其分类子目录中未识别到 Skill",
            ));
        }
    }
}

fn scan_diagnostic(path: &Path, code: &str, message: &str) -> SkillScanDiagnostic {
    SkillScanDiagnostic {
        path: path.to_string_lossy().to_string(),
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn has_entry_document(path: &Path) -> bool {
    detect_skill_entry(path) != SkillEntryKind::Missing
}

pub fn collect_known_skill_paths(db: &Connection) -> Result<Vec<String>, String> {
    let managed_installations = list_managed_installations(db)?;
    let mut paths = HashMap::<PathBuf, String>::new();
    for assistant in assistant_definitions() {
        let (mut entries, _) = collect_assistant_skill_entries(assistant);
        let mut seen = entries.iter().cloned().collect::<HashSet<_>>();
        append_managed_installation_paths(
            assistant.name,
            &managed_installations,
            &mut entries,
            &mut seen,
        );
        for path in entries {
            let identity = path.canonicalize().unwrap_or_else(|_| path.clone());
            paths
                .entry(identity)
                .or_insert_with(|| path.to_string_lossy().to_string());
        }
    }
    let mut paths = paths.into_values().collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn build_skill(
    db: &Connection,
    managed: &ManagedSkill,
    origin_cache: &mut OriginInferenceCache,
) -> Skill {
    let ep = &managed.path;
    let modified = ep
        .metadata()
        .map(|m| {
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs().to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let tag_record = db
        .query_row(
            "SELECT tags_json, COALESCE(tags, '') FROM skill_tags WHERE skill_path = ?",
            [ep.to_string_lossy().to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional();
    let (tags, tag_warning) = match tag_record {
        Ok(Some((tags_json, legacy_tags))) => match serde_json::from_str::<Vec<String>>(&tags_json)
        {
            Ok(mut tags) => {
                if tags.is_empty() && !legacy_tags.is_empty() {
                    tags = legacy_tags.split(',').map(str::to_string).collect();
                }
                (tags, None)
            }
            Err(_) => (
                legacy_tags
                    .split(',')
                    .filter(|tag| !tag.is_empty())
                    .map(str::to_string)
                    .collect(),
                Some("skill_tags_invalid".to_string()),
            ),
        },
        Ok(None) => (Vec::new(), None),
        Err(_) => (Vec::new(), Some("skill_tags_unavailable".to_string())),
    };
    let sync_info = build_sync_info_with_cache(db, ep, origin_cache);
    let state_result = ep
        .parent()
        .map(|root| managed_state_entry(root, ep))
        .unwrap_or(Ok(None));
    let (state_entry, state_error) = match state_result {
        Ok(entry) => (entry, None),
        Err(error) => (None, Some(error)),
    };
    let state_origin = state_entry.as_ref().map(|entry| entry.origin.clone());
    let state_managed = state_entry.is_some();
    let symlink_source = state_origin
        .as_deref()
        .and_then(|origin| origin.strip_prefix("symlink:"))
        .map(|source| source.to_string());
    let inspection = inspect_skill_for_inventory(ep);
    let mut structure = inspection.structure;
    if state_error.is_some() {
        structure
            .structure_warnings
            .push("managed_state_invalid".to_string());
    }
    if let Some(warning) = tag_warning {
        structure.structure_warnings.push(warning);
    }
    if let Some(entry) = &state_entry {
        if content_fingerprint(ep)
            .map(|fingerprint| fingerprint != entry.last_seen_hash)
            .unwrap_or(true)
        {
            structure
                .structure_warnings
                .push("managed_content_changed".to_string());
        }
    }
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
        inventory: SkillInventoryFields {
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
            size: format_size(inspection.content_size),
            modified,
            tags,
            description: structure.manifest_description.clone().unwrap_or_default(),
            readme: inspection.preview,
            version: inspection.version.unwrap_or_else(|| "未知".to_string()),
        },
        origin: SkillOriginFields {
            upstream_url,
            has_update: sync_info.has_update,
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
        },
        structure: SkillStructureFields {
            structure_status: structure.structure_status,
            structure_features: structure.structure_features,
            structure_warnings: structure.structure_warnings,
            manifest_title: structure.manifest_title,
            manifest_description: structure.manifest_description,
        },
    }
}

fn should_scan_skill_entry(path: &Path, name: &str) -> bool {
    if name.starts_with('.') {
        return false;
    }
    if path.is_dir() {
        return !is_empty_dir(path);
    }
    path.is_file()
}

fn is_empty_dir(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skillmate_manifest::SkillMateManifestSkill;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "skillmate-inventory-test-{}-{}",
            name,
            crate::app_core::now_ms()
        ))
    }

    #[test]
    fn scan_filter_skips_hidden_system_container() {
        let root = test_dir("system-container");
        let system = root.join(".system");
        fs::create_dir_all(system.join("skill-installer")).unwrap();
        fs::write(system.join(".codex-system-skills.marker"), "system").unwrap();
        fs::write(system.join("skill-installer").join("SKILL.md"), "# Skill").unwrap();

        assert!(!should_scan_skill_entry(&system, ".system"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scan_filter_skips_empty_runtime_directory() {
        let root = test_dir("runtime");
        let runtime = root.join("codex-primary-runtime");
        fs::create_dir_all(&runtime).unwrap();

        assert!(!should_scan_skill_entry(&runtime, "codex-primary-runtime"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scan_filter_keeps_regular_skill_directory() {
        let root = test_dir("regular");
        let skill = root.join("diagnose");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "# Diagnose").unwrap();

        assert!(should_scan_skill_entry(&skill, "diagnose"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn content_size_includes_nested_files_and_skips_symlinks() {
        let root = test_dir("content-size");
        let nested = root.join("references");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("SKILL.md"), b"1234").unwrap();
        fs::write(nested.join("guide.md"), b"123456").unwrap();

        assert_eq!(inspect_skill_for_inventory(&root).content_size, 10);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn categorized_scan_keeps_skills_and_reports_non_skill_entries() {
        let root = test_dir("categorized-scan");
        let skill = root.join("writing/writer");
        fs::create_dir_all(&skill).unwrap();
        fs::create_dir_all(root.join("notes")).unwrap();
        fs::write(skill.join("SKILL.md"), "# Writer").unwrap();
        fs::write(root.join("notes/note.txt"), "not a skill").unwrap();
        fs::write(root.join("loose.txt"), "not a skill").unwrap();
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        let mut diagnostics = Vec::new();

        collect_skill_entries(&root, 2, &mut entries, &mut seen, &mut diagnostics);

        assert_eq!(entries, vec![skill]);
        assert!(diagnostics
            .iter()
            .any(|item| item.code == "unsupported_root_file"));
        assert!(diagnostics.iter().any(|item| item.path.ends_with("notes")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn managed_project_installation_is_added_to_assistant_inventory() {
        let root = test_dir("project-installation");
        let target = root.join("project/.codex/skills/writer");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "writer").unwrap();
        let installations = vec![ManagedInstallation {
            path: target.clone(),
            skill: SkillMateManifestSkill {
                assistant: "Codex".to_string(),
                ..Default::default()
            },
        }];
        let mut entries = Vec::new();
        let mut seen = HashSet::new();

        append_managed_installation_paths("Codex", &installations, &mut entries, &mut seen);

        assert_eq!(entries, vec![target]);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn distinct_symlink_installations_keep_distinct_inventory_identities() {
        let root = test_dir("symlink-identities");
        let source = root.join("source");
        let first = root.join("first/writer");
        let second = root.join("second/writer");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(first.parent().unwrap()).unwrap();
        fs::create_dir_all(second.parent().unwrap()).unwrap();
        fs::write(source.join("SKILL.md"), "writer").unwrap();
        std::os::unix::fs::symlink(&source, &first).unwrap();
        std::os::unix::fs::symlink(&source, &second).unwrap();

        assert_eq!(
            first.canonicalize().unwrap(),
            second.canonicalize().unwrap()
        );
        assert_ne!(first, second);

        assert_ne!(inventory_cache_key(&first), inventory_cache_key(&second));

        let _ = fs::remove_dir_all(root);
    }
}
