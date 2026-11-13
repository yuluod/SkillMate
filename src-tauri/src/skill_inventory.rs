use crate::app_core::{assistant_definitions, format_size, AssistantDefinition};
use crate::database::{database_path_key, PathColumn};
use crate::managed_installation::{list_managed_installations, ManagedInstallation};
use crate::managed_state::{
    content_fingerprint, fingerprint_matches, managed_state_entry, STATE_FILE_NAME,
};
use crate::skill_library::{find_deployment, resolve_library_path};
use crate::skill_origin::{
    build_sync_info_readonly, build_sync_info_with_cache, OriginInferenceCache,
};
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

fn normalize_relative_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    normalized.components().next_back(),
                    Some(Component::RootDir | Component::Prefix(_)) | None
                ) {
                    normalized.pop();
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn is_agents_skills_root(root: &Path) -> bool {
    root.file_name().is_some_and(|name| name == "skills")
        && root
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".agents")
}

/// `npx skills` 把主副本放在 `.agents/skills/<name>`，再从各 Agent 目录建立链接。
/// 只认这个稳定结构，避免把 SkillMate 自己的统一库连接误判为外部安装。
fn skills_cli_link_target(path: &Path) -> Option<PathBuf> {
    let target = fs::read_link(path).ok()?;
    let joined = if target.is_absolute() {
        target
    } else {
        path.parent()?.join(target)
    };
    let resolved = normalize_relative_path(&joined);
    is_agents_skills_root(resolved.parent()?).then_some(resolved)
}

fn has_skills_cli_sibling_link(store_entry: &Path) -> bool {
    has_skills_cli_sibling_link_with_home(store_entry, dirs::home_dir().as_deref())
}

fn has_skills_cli_sibling_link_with_home(store_entry: &Path, home: Option<&Path>) -> bool {
    let Some(name) = store_entry.file_name() else {
        return false;
    };
    let Some(scope_root) = store_entry
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
    else {
        return false;
    };
    let global = home.is_some_and(|home| normalize_relative_path(home) == scope_root);
    let expected = normalize_relative_path(store_entry);
    let roots = assistant_definitions().iter().flat_map(|assistant| {
        if global {
            assistant
                .global_discovery_roots_in(scope_root)
                .collect::<Vec<_>>()
        } else {
            assistant
                .project_discovery_roots(scope_root)
                .collect::<Vec<_>>()
        }
    });
    let mut seen = HashSet::new();
    roots.filter(|root| seen.insert(root.clone())).any(|root| {
        let candidate = root.join(name);
        skills_cli_link_target(&candidate).is_some_and(|target| target == expected)
    })
}

/// `.agents/skills/<name>` 的实体主副本在两类证据下识别为 `skills CLI` 管理：
/// 项目根有 `skills-lock.json`，或同级 Agent 目录存在指向它的软连接。
pub(crate) fn is_skills_cli_store_entry(path: &Path) -> bool {
    let normalized = normalize_relative_path(path);
    let Some(root) = normalized.parent() else {
        return false;
    };
    if !is_agents_skills_root(root) {
        return false;
    }
    let has_lock = root
        .parent()
        .and_then(Path::parent)
        .is_some_and(|project| project.join("skills-lock.json").is_file());
    has_lock || has_skills_cli_sibling_link(&normalized)
}

pub(crate) fn is_skills_cli_path(path: &Path) -> bool {
    skills_cli_link_target(path).is_some() || is_skills_cli_store_entry(path)
}

/// Owned by one read-only scan. Never retain across scans or filesystem mutations.
#[derive(Default)]
pub(crate) struct InventoryScanCache {
    origin: OriginInferenceCache,
    fingerprints: FingerprintCache,
}

#[derive(Default)]
struct FingerprintCache {
    // Do not canonicalize: a top-level symlink hashes its link target, not its contents.
    values: HashMap<PathBuf, Result<String, String>>,
    #[cfg(test)]
    computations: usize,
}

impl FingerprintCache {
    fn get(&mut self, path: &Path) -> Result<String, String> {
        self.values
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                #[cfg(test)]
                {
                    self.computations += 1;
                }
                content_fingerprint(path)
            })
            .clone()
    }

    fn matches(&mut self, path: &Path, expected: &str) -> Result<bool, String> {
        if expected.starts_with("sha256:v2:") {
            self.get(path).map(|fingerprint| fingerprint == expected)
        } else {
            fingerprint_matches(path, expected)
        }
    }
}

pub fn scan_all_assistants(db: &Connection) -> Result<Vec<AIAssistant>, String> {
    scan_assistants_inner(db, true)
}

/// 只读变体：扫描口径与 scan_all_assistants 一致，
/// 但不持久化来源推断与受管校正，供 doctor 等诊断入口使用。
pub fn scan_all_assistants_readonly(db: &Connection) -> Result<Vec<AIAssistant>, String> {
    scan_assistants_inner(db, false)
}

fn scan_assistants_inner(db: &Connection, persist: bool) -> Result<Vec<AIAssistant>, String> {
    let managed_installations = list_managed_installations(db)?;
    let mut skill_cache = HashMap::<PathBuf, Skill>::new();
    let mut scan_cache = InventoryScanCache::default();
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
            let skill = if persist {
                build_skill_with_cache(db, &managed, &mut scan_cache)
            } else {
                build_skill_with_cache_readonly(db, &managed, &mut scan_cache)
            };
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
            supports_project_skills: assistant.supports_project_skills(),
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

pub(crate) fn collect_skill_entries(
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
    for installation in &managed_installations {
        if installation.skill.assistant == crate::managed_state::LIBRARY_OWNER_NAME
            && (installation.path.exists() || fs::symlink_metadata(&installation.path).is_ok())
        {
            let identity = installation
                .path
                .canonicalize()
                .unwrap_or_else(|_| installation.path.clone());
            paths
                .entry(identity)
                .or_insert_with(|| installation.path.to_string_lossy().to_string());
        }
    }
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

pub(crate) fn build_skill(
    db: &Connection,
    managed: &ManagedSkill,
    origin_cache: &mut OriginInferenceCache,
) -> Skill {
    build_skill_with_fingerprints(
        db,
        managed,
        origin_cache,
        &mut FingerprintCache::default(),
        true,
    )
}

/// 只读变体：不持久化来源推断与受管校正。
pub(crate) fn build_skill_readonly(
    db: &Connection,
    managed: &ManagedSkill,
    origin_cache: &mut OriginInferenceCache,
) -> Skill {
    build_skill_with_fingerprints(
        db,
        managed,
        origin_cache,
        &mut FingerprintCache::default(),
        false,
    )
}

pub(crate) fn build_skill_with_cache(
    db: &Connection,
    managed: &ManagedSkill,
    cache: &mut InventoryScanCache,
) -> Skill {
    build_skill_with_fingerprints(
        db,
        managed,
        &mut cache.origin,
        &mut cache.fingerprints,
        true,
    )
}

pub(crate) fn build_skill_with_cache_readonly(
    db: &Connection,
    managed: &ManagedSkill,
    cache: &mut InventoryScanCache,
) -> Skill {
    build_skill_with_fingerprints(
        db,
        managed,
        &mut cache.origin,
        &mut cache.fingerprints,
        false,
    )
}

fn build_skill_with_fingerprints(
    db: &Connection,
    managed: &ManagedSkill,
    origin_cache: &mut OriginInferenceCache,
    fingerprints: &mut FingerprintCache,
    persist: bool,
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
    let metadata_path = resolve_library_path(db, ep).unwrap_or_else(|_| ep.to_path_buf());
    let tag_record =
        database_path_key(db, PathColumn::SkillTags, &metadata_path).and_then(|metadata_key| {
            db.query_row(
                "SELECT tags_json, COALESCE(tags, '') FROM skill_tags WHERE skill_path = ?",
                [metadata_key],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())
        });
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
    let sync_info = if persist {
        build_sync_info_with_cache(db, ep, origin_cache)
    } else {
        build_sync_info_readonly(db, ep, origin_cache)
    };
    let deployment = find_deployment(db, ep).ok().flatten();
    let content_path = metadata_path.as_path();
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
    let content_state_result = if content_path == ep {
        Ok(None)
    } else {
        content_path
            .parent()
            .map(|root| managed_state_entry(root, content_path))
            .unwrap_or(Ok(None))
    };
    let (content_state_entry, content_state_error) = match content_state_result {
        Ok(entry) => (entry, None),
        Err(error) => (None, Some(error)),
    };
    let symlink_source = state_origin
        .as_deref()
        .and_then(|origin| origin.strip_prefix("symlink:"))
        .map(|source| source.to_string());
    let inspection = inspect_skill_for_inventory(ep);
    let mut structure = inspection.structure;
    if state_error.is_some() || content_state_error.is_some() {
        structure
            .structure_warnings
            .push("managed_state_invalid".to_string());
    }
    if let Some(warning) = tag_warning {
        structure.structure_warnings.push(warning);
    }
    let deployment_changed = state_entry.as_ref().is_some_and(|entry| {
        fingerprints
            .matches(ep, &entry.last_seen_hash)
            .map(|matches| !matches)
            .unwrap_or(true)
    });
    let library_content_changed = content_state_entry.as_ref().is_some_and(|entry| {
        fingerprints
            .matches(content_path, &entry.last_seen_hash)
            .map(|matches| !matches)
            .unwrap_or(true)
    });
    if deployment_changed || library_content_changed {
        structure
            .structure_warnings
            .push("managed_content_changed".to_string());
    }
    let upstream_url = if !sync_info.meta.resolved_locator.is_empty() {
        sync_info.meta.resolved_locator.clone()
    } else {
        sync_info.meta.origin_locator.clone()
    };
    let is_deployment = deployment.is_some();
    let managed_by_app = sync_info.meta.managed_by_app || state_managed;
    let skills_cli_link = !is_deployment && !managed_by_app && skills_cli_link_target(ep).is_some();
    let skills_cli = !is_deployment && !managed_by_app && is_skills_cli_path(ep);
    let source_type = if is_deployment {
        "deployment".to_string()
    } else if symlink_source.is_some() || skills_cli_link {
        "symlink".to_string()
    } else {
        sync_info.meta.origin_kind.clone()
    };
    let source = if skills_cli {
        "skills CLI".to_string()
    } else {
        match source_type.as_str() {
            "deployment" => "SkillMate".to_string(),
            "symlink" => "项目软连接".to_string(),
            "git" if upstream_url.contains("github.com") => "GitHub".to_string(),
            "git" => "Git".to_string(),
            "legacy_npm" | "npm" => "npm（历史）".to_string(),
            "legacy_pip" | "pip" => "PyPI（历史）".to_string(),
            "local" => "Local".to_string(),
            _ => "未托管".to_string(),
        }
    };
    let content_source = if skills_cli {
        "external_cli".to_string()
    } else {
        match sync_info.meta.origin_kind.as_str() {
            "git" if upstream_url.contains("github.com") => "github",
            "git" => "git",
            "legacy_npm" | "npm" => "external_cli",
            "legacy_pip" | "pip" => "external_cli",
            "local" => "local",
            _ => "unknown",
        }
        .to_string()
    };
    let manager = if managed_by_app {
        "skillmate"
    } else if skills_cli {
        "skills_cli"
    } else if matches!(
        sync_info.meta.origin_kind.as_str(),
        "legacy_npm" | "npm" | "legacy_pip" | "pip"
    ) {
        "external_cli"
    } else {
        "external"
    }
    .to_string();
    let update_strategy = if managed_by_app && sync_info.meta.origin_kind == "git" {
        "skillmate"
    } else if skills_cli
        || matches!(
            sync_info.meta.origin_kind.as_str(),
            "legacy_npm" | "npm" | "legacy_pip" | "pip"
        )
    {
        "external_cli"
    } else if sync_info.meta.origin_kind == "git" {
        "external"
    } else {
        "manual"
    }
    .to_string();
    let can_check = !skills_cli && sync_info.can_check;
    let can_sync = !skills_cli && sync_info.can_sync;
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
            content_source,
            manager,
            update_strategy,
            size: format_size(inspection.content_size),
            modified,
            tags,
            description: structure.manifest_description.clone().unwrap_or_default(),
            readme: inspection.preview,
            version: inspection.version.unwrap_or_else(|| "未知".to_string()),
            content_hash: fingerprints.get(content_path).unwrap_or_default(),
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
            managed_by_app,
            can_check,
            can_sync,
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
    use crate::skill_model::SkillDescriptor;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "skillmate-inventory-test-{}-{}",
            name,
            crate::app_core::now_ms()
        ))
    }

    #[test]
    fn managed_content_is_hashed_once_and_next_scan_observes_changes() {
        let root = test_dir("fingerprint-refresh");
        let path = root.join("writer");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("SKILL.md"), "# Writer").unwrap();
        crate::managed_state::mark_managed_skill(&root, "Codex", &path, "local").unwrap();
        let db = Connection::open_in_memory().unwrap();
        let managed = ManagedSkill {
            path: path.clone(),
            name: "writer".to_string(),
        };
        let mut cache = InventoryScanCache::default();
        let first = build_skill_with_cache(&db, &managed, &mut cache);
        assert_eq!(cache.fingerprints.computations, 1);
        assert!(!first
            .structure
            .structure_warnings
            .contains(&"managed_content_changed".to_string()));
        let repeated = build_skill_with_cache(&db, &managed, &mut cache);
        assert_eq!(cache.fingerprints.computations, 1);
        assert_eq!(
            first.inventory.content_hash,
            repeated.inventory.content_hash
        );

        fs::write(path.join("SKILL.md"), "# Changed").unwrap();
        let mut next_scan = InventoryScanCache::default();
        let changed = build_skill_with_cache(&db, &managed, &mut next_scan);
        assert_eq!(next_scan.fingerprints.computations, 1);
        assert_ne!(first.inventory.content_hash, changed.inventory.content_hash);
        assert!(changed
            .structure
            .structure_warnings
            .contains(&"managed_content_changed".to_string()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fingerprint_failure_is_cached_only_for_current_scan() {
        let root = test_dir("fingerprint-error");
        let mut cache = FingerprintCache::default();
        assert!(cache.get(&root).is_err());
        assert!(cache.get(&root).is_err());
        assert_eq!(cache.computations, 1);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("SKILL.md"), "# Recovered").unwrap();
        let mut next_scan = FingerprintCache::default();
        assert_eq!(
            next_scan.get(&root).unwrap(),
            content_fingerprint(&root).unwrap()
        );
        assert_eq!(next_scan.computations, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deployments_share_content_hash_but_keep_link_and_inspection_identity() {
        assert_deployment_fingerprint_reuse(true);
    }

    #[test]
    fn deployments_share_content_hash_but_keep_directory_identity() {
        assert_deployment_fingerprint_reuse(false);
    }

    fn assert_deployment_fingerprint_reuse(use_symlinks: bool) {
        let root = test_dir(if use_symlinks {
            "deployment-link-fingerprints"
        } else {
            "deployment-directory-fingerprints"
        });
        let source = root.join("library/writer");
        let first = root.join("first/writer");
        let second = root.join("second/renamed");
        let external = root.join("external/writer");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: writer\ndescription: test\n---\nbody",
        )
        .unwrap();
        for path in [&first, &second, &external] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if use_symlinks {
                if !crate::app_core::create_test_directory_symlink_or_skip(&source, path) {
                    let _ = fs::remove_dir_all(root);
                    return;
                }
            } else {
                fs::create_dir_all(path).unwrap();
                fs::copy(source.join("SKILL.md"), path.join("SKILL.md")).unwrap();
            }
        }
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE skill_deployments (target_path TEXT PRIMARY KEY, library_path TEXT, deploy_mode TEXT);").unwrap();
        for path in [&first, &second] {
            db.execute(
                "INSERT INTO skill_deployments VALUES (?, ?, ?)",
                rusqlite::params![
                    path.to_string_lossy(),
                    source.to_string_lossy(),
                    if use_symlinks { "symlink" } else { "copy" }
                ],
            )
            .unwrap();
            crate::managed_state::mark_managed_skill(
                path.parent().unwrap(),
                "Codex",
                path,
                "deployment",
            )
            .unwrap();
        }
        crate::managed_state::mark_managed_skill(
            source.parent().unwrap(),
            "SkillMate",
            &source,
            "local",
        )
        .unwrap();
        let mut cache = InventoryScanCache::default();
        let mut skills = Vec::new();
        for path in [&first, &second, &external] {
            skills.push(build_skill_with_cache(
                &db,
                &ManagedSkill {
                    path: path.clone(),
                    name: path.file_name().unwrap().to_string_lossy().to_string(),
                },
                &mut cache,
            ));
        }
        // Two deployment hashes + one shared content tree + one external path hash.
        assert_eq!(cache.fingerprints.computations, 4);
        assert_eq!(cache.fingerprints.values.len(), 4);
        let content_hash = content_fingerprint(&source).unwrap();
        let link_hash = content_fingerprint(&external).unwrap();
        if use_symlinks {
            assert_ne!(content_hash, link_hash);
        }
        for (skill, path) in skills.iter().zip([&first, &second, &external]) {
            assert_eq!(skill.inventory.path, path.to_string_lossy());
            assert_eq!(skill.inventory.id, path.to_string_lossy());
            assert!(!skill
                .structure
                .structure_warnings
                .contains(&"managed_content_changed".to_string()));
        }
        assert_eq!(skills[0].inventory.content_hash, content_hash);
        assert_eq!(skills[1].inventory.content_hash, content_hash);
        assert_eq!(skills[2].inventory.content_hash, link_hash);
        assert_eq!(skills[0].inventory.source_type, "deployment");
        assert_ne!(skills[2].inventory.source_type, "deployment");
        assert!(!skills[0]
            .structure
            .structure_warnings
            .contains(&"name_directory_mismatch".to_string()));
        assert!(skills[1]
            .structure
            .structure_warnings
            .contains(&"name_directory_mismatch".to_string()));

        fs::write(
            source.join("SKILL.md"),
            "---\nname: writer\ndescription: changed\n---\nnew body",
        )
        .unwrap();
        let mut next_scan = InventoryScanCache::default();
        for path in [&first, &second] {
            let skill = build_skill_with_cache(
                &db,
                &ManagedSkill {
                    path: path.clone(),
                    name: "writer".to_string(),
                },
                &mut next_scan,
            );
            assert_ne!(skill.inventory.content_hash, content_hash);
            assert!(skill
                .structure
                .structure_warnings
                .contains(&"managed_content_changed".to_string()));
        }
        assert_eq!(next_scan.fingerprints.computations, 3);
        assert_eq!(content_fingerprint(&external).unwrap(), link_hash);
        fs::remove_dir_all(root).unwrap();
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
            skill: SkillDescriptor {
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
        if !crate::app_core::create_test_directory_symlink_or_skip(&source, &first)
            || !crate::app_core::create_test_directory_symlink_or_skip(&source, &second)
        {
            let _ = fs::remove_dir_all(root);
            return;
        }

        assert_eq!(
            first.canonicalize().unwrap(),
            second.canonicalize().unwrap()
        );
        assert_ne!(first, second);

        assert_ne!(inventory_cache_key(&first), inventory_cache_key(&second));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agents_skills_root_requires_exact_directory_names() {
        assert!(is_agents_skills_root(Path::new("/home/u/.agents/skills")));
        assert!(!is_agents_skills_root(Path::new(
            "/home/u/.agents/skills/writer"
        )));
        assert!(!is_agents_skills_root(Path::new("/home/u/agents/skills")));
    }

    #[test]
    fn skills_cli_links_are_marked_as_external_installer() {
        let root = test_dir("skills-cli-link");
        let canonical = root.join(".agents/skills/writer");
        let agent_root = root.join(".claude/skills");
        fs::create_dir_all(&canonical).unwrap();
        fs::create_dir_all(&agent_root).unwrap();
        fs::write(
            canonical.join("SKILL.md"),
            "---\nname: writer\ndescription: test skill\n---\n",
        )
        .unwrap();
        let link = agent_root.join("writer");
        if !crate::app_core::create_test_directory_symlink_or_skip(&canonical, &link) {
            let _ = fs::remove_dir_all(root);
            return;
        }

        let db = Connection::open_in_memory().unwrap();
        let managed = ManagedSkill {
            path: link,
            name: "writer".to_string(),
        };
        let mut cache = InventoryScanCache::default();
        let skill = build_skill_with_cache(&db, &managed, &mut cache);

        assert_eq!(skill.inventory.source, "skills CLI");
        assert_eq!(skill.inventory.source_type, "symlink");
        assert_eq!(skill.inventory.content_source, "external_cli");
        assert_eq!(skill.inventory.manager, "skills_cli");
        assert_eq!(skill.inventory.update_strategy, "external_cli");
        assert!(!skill.origin.can_check);
        assert!(!skill.origin.can_sync);

        // 同级的 .agents/skills 主副本在存在外部链接时也应归属为 skills CLI。
        let store = build_skill_with_cache(
            &db,
            &ManagedSkill {
                path: canonical,
                name: "writer".to_string(),
            },
            &mut InventoryScanCache::default(),
        );
        assert_eq!(store.inventory.manager, "skills_cli");
        assert_eq!(store.inventory.source_type, "local");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn skills_cli_store_entries_require_project_lock_file() {
        let root = test_dir("skills-cli-store");
        let project = root.join("project");
        let canonical = project.join(".agents/skills/writer");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(
            canonical.join("SKILL.md"),
            "---\nname: writer\ndescription: test skill\n---\n",
        )
        .unwrap();

        let db = Connection::open_in_memory().unwrap();
        let managed = ManagedSkill {
            path: canonical.clone(),
            name: "writer".to_string(),
        };
        let mut cache = InventoryScanCache::default();
        let without_lock = build_skill_with_cache(&db, &managed, &mut cache);
        assert_eq!(without_lock.inventory.content_source, "local");
        assert_eq!(without_lock.inventory.manager, "external");

        fs::write(
            project.join("skills-lock.json"),
            "{\"version\":1,\"skills\":{}}",
        )
        .unwrap();
        let mut cache = InventoryScanCache::default();
        let with_lock = build_skill_with_cache(&db, &managed, &mut cache);
        assert_eq!(with_lock.inventory.source, "skills CLI");
        assert_eq!(with_lock.inventory.source_type, "local");
        assert_eq!(with_lock.inventory.content_source, "external_cli");
        assert_eq!(with_lock.inventory.manager, "skills_cli");
        assert!(!with_lock.origin.can_check);
        assert!(!with_lock.origin.can_sync);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn relative_parent_links_to_agents_store_are_normalized() {
        let normalized = normalize_relative_path(Path::new(
            "/work/project/.claude/skills/../../.agents/skills/writer",
        ));
        assert_eq!(
            normalized,
            PathBuf::from("/work/project/.agents/skills/writer")
        );
        assert!(is_agents_skills_root(normalized.parent().unwrap()));
    }

    #[test]
    fn project_store_ignores_global_only_sibling_directories() {
        let root = test_dir("project-global-only-link");
        let store = root.join(".agents/skills/writer");
        let link = root.join(".config/opencode/skills/writer");
        fs::create_dir_all(&store).unwrap();
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        if crate::app_core::create_test_directory_symlink_or_skip(&store, &link) {
            assert!(!is_skills_cli_store_entry(&store));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn global_store_recognizes_compatibility_links_without_project_lock() {
        for relative in [
            ".codex/skills",
            ".codeium/windsurf/skills",
            ".codeium/windsurf-next/skills",
            ".codeium/windsurf-insiders/skills",
        ] {
            let home = test_dir("global-compatibility-link");
            let store = home.join(".agents/skills/writer");
            let link = home.join(relative).join("writer");
            fs::create_dir_all(&store).unwrap();
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            assert!(!has_skills_cli_sibling_link_with_home(&store, Some(&home)));
            if crate::app_core::create_test_directory_symlink_or_skip(&store, &link) {
                assert!(has_skills_cli_sibling_link_with_home(&store, Some(&home)));
                // 同一目录布局作为项目时，不应套用全局兼容路径。
                assert!(!has_skills_cli_sibling_link_with_home(&store, None));
            }
            fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn project_store_recognizes_project_discovery_links() {
        for relative in [
            ".claude/skills",
            ".github/skills",
            ".opencode/skills",
            "skills",
        ] {
            let root = test_dir("project-discovery-link");
            let store = root.join(".agents/skills/writer");
            let link = root.join(relative).join("writer");
            fs::create_dir_all(&store).unwrap();
            fs::create_dir_all(link.parent().unwrap()).unwrap();
            if crate::app_core::create_test_directory_symlink_or_skip(&store, &link) {
                assert!(is_skills_cli_store_entry(&store));
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
}
