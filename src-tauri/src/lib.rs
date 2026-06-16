#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_core;
mod library_manifest;
mod managed_state;
mod scenario_manifest;
mod skill_install;
mod skill_install_source;
mod skill_inventory;
mod skill_origin;
mod skill_package;
mod skill_profile;
mod skill_structure;
mod skillmate_manifest;

use app_core::{
    assistant_root_by_name, assistant_slug, expand_path, generate_id, git_output,
    managed_skill_roots, now_ms, project_skill_root_by_name, run_command_with_timeout,
};
use library_manifest::{
    build_library_export, merge_imported_library, preview_imported_library, read_library_export,
    write_library_export, ImportPreview,
};
use managed_state::is_managed_by_state;
use rusqlite::{params, Connection};
use scenario_manifest::{
    build_scenario_manifest, merge_scenario_manifest, preview_scenario_manifest,
    read_scenario_manifest, write_scenario_manifest, ScenarioManifestPreview,
};
use serde::{Deserialize, Serialize};
use skill_install::{
    copy_dir_recursive, detect_install_source_rules, install_git_package, install_local_package,
    install_local_symlink_package, install_target_name, is_git_install_source,
    parse_git_install_spec, preview_install_source, preview_local_symlink_install,
    remove_existing_path, InstallDetection, InstallPreview, InstallResult,
};
use skill_inventory::{collect_known_skill_paths, scan_all_assistants};
use skill_origin::{
    load_origin_meta, probe_skill_state, save_installed_git_meta as save_git_origin_meta,
    sync_info_json, update_skill_from_upstream,
};
use skill_package::{detect_skill_package, PackageDetection};
use skill_profile::{
    read_skill_profiles, rollback_active_profile, set_active_profile, upsert_skill_profile,
    validate_skill_profile, SkillSetProfileDiff, SkillSetProfilePreview, SkillSetProfileStore,
};
use skill_structure::{analyze_skill_structure, read_skill_preview, SkillStructureInfo};
use skill_structure::{validate_skill_structure, SkillValidationReport};
use skillmate_manifest::{
    preview_skillmate_manifest, read_skillmate_manifest, write_skillmate_manifest,
    SkillMateManifest, SkillMateManifestPreview, SkillMateManifestSkill,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

static SYNC_LOCK: Mutex<()> = Mutex::new(());

fn get_db_path() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("skillmate");
    fs::create_dir_all(&dir).ok();
    dir.join("data.db")
}

fn get_db() -> Connection {
    let db = Connection::open(get_db_path()).unwrap();
    db.execute("CREATE TABLE IF NOT EXISTS tags (id TEXT PRIMARY KEY, name TEXT NOT NULL, color TEXT NOT NULL)", []).ok();
    db.execute("CREATE TABLE IF NOT EXISTS scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT, skill_ids TEXT, created_at TEXT)", []).ok();
    db.execute(
        "CREATE TABLE IF NOT EXISTS skill_tags (skill_path TEXT PRIMARY KEY, tags TEXT)",
        [],
    )
    .ok();
    db.execute("CREATE TABLE IF NOT EXISTS git_backup (id INTEGER PRIMARY KEY, enabled INTEGER, remote_url TEXT, repo_path TEXT, branch TEXT, last_sync TEXT)", []).ok();
    db.execute(
        "CREATE TABLE IF NOT EXISTS skill_origin_meta (
            skill_path TEXT PRIMARY KEY,
            origin_kind TEXT NOT NULL DEFAULT 'unknown',
            origin_locator TEXT NOT NULL DEFAULT '',
            resolved_locator TEXT NOT NULL DEFAULT '',
            tracking_ref TEXT NOT NULL DEFAULT '',
            installed_ref TEXT NOT NULL DEFAULT '',
            latest_ref TEXT NOT NULL DEFAULT '',
            sync_state TEXT NOT NULL DEFAULT 'unprobed',
            sync_message TEXT NOT NULL DEFAULT '',
            lag_count INTEGER NOT NULL DEFAULT 0,
            last_probe_at INTEGER,
            last_sync_at INTEGER,
            managed_by_app INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )
    .ok();
    let count: i32 = db
        .query_row("SELECT COUNT(*) FROM tags", [], |r| r.get(0))
        .unwrap_or(0);
    if count == 0 {
        db.execute(
            "INSERT INTO tags (id, name, color) VALUES ('1', '前端', '#6366f1')",
            [],
        )
        .ok();
        db.execute(
            "INSERT INTO tags (id, name, color) VALUES ('2', '后端', '#10b981')",
            [],
        )
        .ok();
        db.execute(
            "INSERT INTO tags (id, name, color) VALUES ('3', 'AI', '#f59e0b')",
            [],
        )
        .ok();
    }
    db
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub path: String,
    pub skill_type: String,
    pub source: String,
    pub source_type: String,
    pub size: String,
    pub modified: String,
    pub tags: Vec<String>,
    pub description: String,
    pub readme: String,
    pub version: String,
    pub upstream_url: String,
    pub has_update: bool,
    pub compatible_with: Vec<String>,
    pub usage_count: u32,
    pub origin_kind: String,
    pub origin_locator: String,
    pub resolved_locator: String,
    pub tracking_ref: String,
    pub installed_ref: String,
    pub latest_ref: String,
    pub sync_state: String,
    pub sync_message: String,
    pub lag_count: u32,
    pub last_probe_at: Option<i64>,
    pub last_sync_at: Option<i64>,
    pub managed_by_app: bool,
    pub can_sync: bool,
    pub symlink_source: Option<String>,
    pub structure_status: String,
    pub structure_features: Vec<String>,
    pub structure_warnings: Vec<String>,
    pub manifest_title: Option<String>,
    pub manifest_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Scenario {
    pub id: String,
    pub name: String,
    pub description: String,
    pub skill_ids: Vec<String>,
    pub created_at: String,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GitBackup {
    pub enabled: bool,
    pub remote_url: String,
    pub repo_path: String,
    pub branch: String,
    pub last_sync: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelAssistRequest {
    pub input: String,
    pub local_detection: PackageDetection,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelAssistResult {
    pub suggested_kind: String,
    pub confidence: f32,
    pub explanation: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectSkillTargetPreview {
    pub assistant: String,
    pub target_path: String,
    pub exists: bool,
    pub recommended: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AIAssistant {
    pub name: String,
    pub path: String,
    pub ai_type: String,
    pub icon: String,
    pub skills: Vec<Skill>,
    pub exists: bool,
}

#[tauri::command]
fn export_library(path: String) -> Result<String, String> {
    let export = build_library_export(get_all_tags(), get_scenarios(), get_all_assistants());
    write_library_export(path, &export)
}

#[tauri::command]
fn preview_import_library(path: String, mode: Option<String>) -> Result<ImportPreview, String> {
    let export = read_library_export(path)?;
    let db = get_db();
    let replace_existing = matches!(mode.as_deref(), Some("replace"));
    preview_imported_library(&db, &export, replace_existing)
}

#[tauri::command]
fn import_library(path: String, mode: Option<String>) -> Result<String, String> {
    let export = read_library_export(path)?;
    let db = get_db();
    let replace_existing = matches!(mode.as_deref(), Some("replace"));
    let (tag_count, scenario_count) = merge_imported_library(&db, export, replace_existing)?;
    Ok(format!(
        "{}导入 {} 个标签，{} 个场景",
        if replace_existing {
            "已替换并"
        } else {
            "已"
        },
        tag_count,
        scenario_count
    ))
}

#[tauri::command]
fn export_scenario_manifest(path: String) -> Result<String, String> {
    let manifest = build_scenario_manifest(get_scenarios());
    write_scenario_manifest(path, &manifest)
}

#[tauri::command]
fn preview_import_scenario_manifest(
    path: String,
    mode: Option<String>,
) -> Result<ScenarioManifestPreview, String> {
    let manifest = read_scenario_manifest(path)?;
    let db = get_db();
    let replace_existing = matches!(mode.as_deref(), Some("replace"));
    preview_scenario_manifest(
        &db,
        &manifest,
        replace_existing,
        &collect_known_skill_paths(&db),
    )
}

#[tauri::command]
fn import_scenario_manifest(path: String, mode: Option<String>) -> Result<String, String> {
    let manifest = read_scenario_manifest(path)?;
    let db = get_db();
    let replace_existing = matches!(mode.as_deref(), Some("replace"));
    let scenario_count = merge_scenario_manifest(&db, manifest, replace_existing)?;
    Ok(format!(
        "{}导入 {} 个场景",
        if replace_existing {
            "已替换并"
        } else {
            "已"
        },
        scenario_count
    ))
}

#[tauri::command]
fn export_skillmate_manifest(path: String) -> Result<String, String> {
    let manifest = build_current_skillmate_manifest();
    write_skillmate_manifest(expand_path(path.trim()), &manifest)
}

fn build_current_skillmate_manifest() -> SkillMateManifest {
    let assistants = get_all_assistants();
    let skills = assistants
        .into_iter()
        .flat_map(|assistant| {
            assistant.skills.into_iter().map(move |skill| {
                let source_kind = if skill.origin_kind == "git" {
                    "git".to_string()
                } else {
                    "local".to_string()
                };
                let source = if let Some(symlink_source) = skill.symlink_source.clone() {
                    symlink_source
                } else if skill.origin_kind == "git" {
                    if !skill.origin_locator.trim().is_empty() {
                        skill.origin_locator
                    } else {
                        skill.resolved_locator
                    }
                } else {
                    skill.path.clone()
                };
                SkillMateManifestSkill {
                    assistant: assistant.name.clone(),
                    source,
                    source_kind,
                    target_name: Some(skill.name),
                }
            })
        })
        .collect();
    SkillMateManifest { version: 1, skills }
}

#[tauri::command]
fn preview_apply_skillmate_manifest(path: String) -> Result<SkillMateManifestPreview, String> {
    let manifest = read_skillmate_manifest(expand_path(path.trim()))?;
    preview_skillmate_manifest(&manifest)
}

#[tauri::command]
fn apply_skillmate_manifest(path: String) -> Result<String, String> {
    let manifest = read_skillmate_manifest(expand_path(path.trim()))?;
    let preview = preview_skillmate_manifest(&manifest)?;
    if !preview.can_apply {
        return Err(format!(
            "manifest 存在 {} 个冲突，请先处理预览",
            preview.conflicts.len()
        ));
    }
    let mut installed = 0usize;
    for skill in manifest.skills {
        apply_manifest_skill(skill)?;
        installed += 1;
    }
    Ok(format!("已应用 {} 条 Skill manifest 记录", installed))
}

#[tauri::command]
fn get_skill_profiles() -> SkillSetProfileStore {
    read_skill_profiles()
}

#[tauri::command]
fn save_current_skill_profile(
    name: String,
    description: String,
) -> Result<SkillSetProfileStore, String> {
    let manifest = build_current_skillmate_manifest();
    upsert_skill_profile(&name, &description, manifest.skills)
}

#[tauri::command]
fn preview_apply_skill_profile(profile_id: String) -> Result<SkillSetProfilePreview, String> {
    let store = read_skill_profiles();
    let profile = store
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .cloned()
        .ok_or_else(|| "Profile 不存在".to_string())?;
    let profile_issues = validate_skill_profile(&profile, &store.profiles);
    let current = build_current_skillmate_manifest();
    let manifest = SkillMateManifest {
        version: 1,
        skills: profile.skills.clone(),
    };
    let manifest_preview = preview_skillmate_manifest(&manifest)?;
    let diff = build_profile_diff(&current, &manifest, &manifest_preview);
    Ok(SkillSetProfilePreview {
        profile,
        profile_issues,
        diff,
        manifest_preview,
    })
}

#[tauri::command]
fn apply_skill_profile(profile_id: String) -> Result<String, String> {
    let preview = preview_apply_skill_profile(profile_id.clone())?;
    if !preview.profile_issues.is_empty() {
        return Err("Profile 格式存在问题，请先处理预览".to_string());
    }
    if !preview.manifest_preview.can_apply {
        return Err(format!(
            "Profile 存在 {} 个冲突，请先处理预览",
            preview.manifest_preview.conflicts.len()
        ));
    }
    let mut installed = 0usize;
    for skill in preview.profile.skills {
        apply_manifest_skill(skill)?;
        installed += 1;
    }
    set_active_profile(&profile_id)?;
    Ok(format!("已应用 Profile，安装 {} 条 Skill 记录", installed))
}

#[tauri::command]
fn rollback_skill_profile() -> Result<String, String> {
    let previous_profile_id = rollback_active_profile()?;
    apply_skill_profile(previous_profile_id)
}

fn build_profile_diff(
    current: &SkillMateManifest,
    target: &SkillMateManifest,
    preview: &SkillMateManifestPreview,
) -> SkillSetProfileDiff {
    let current_keys = current
        .skills
        .iter()
        .map(manifest_skill_key)
        .collect::<std::collections::HashSet<_>>();
    let conflicts = preview
        .conflicts
        .iter()
        .map(|conflict| format!("{}: {}", conflict.assistant, conflict.reason))
        .chain(
            preview
                .validation_issues
                .iter()
                .map(|issue| format!("#{}: {}", issue.index + 1, issue.message)),
        )
        .collect();
    let mut to_install = Vec::new();
    let mut already_present = Vec::new();
    for skill in &target.skills {
        let key = manifest_skill_key(skill);
        if current_keys.contains(&key) {
            already_present.push(key);
        } else {
            to_install.push(key);
        }
    }
    SkillSetProfileDiff {
        to_install,
        already_present,
        conflicts,
    }
}

fn manifest_skill_key(skill: &SkillMateManifestSkill) -> String {
    format!(
        "{}:{}:{}",
        skill.assistant,
        skill
            .target_name
            .clone()
            .unwrap_or_else(|| skill.source.clone()),
        skill.source_kind
    )
}

fn apply_manifest_skill(skill: SkillMateManifestSkill) -> Result<(), String> {
    let target_root = assistant_root_by_name(&skill.assistant)?;
    fs::create_dir_all(&target_root).map_err(|e| e.to_string())?;
    let fallback_name = match skill.target_name.as_deref() {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => install_target_name(&skill.source, &skill.source_kind)?,
    };
    match skill.source_kind.as_str() {
        source_kind if is_git_install_source(source_kind) => {
            let spec = parse_git_install_spec(&skill.source)?;
            install_git_package(
                spec,
                &target_root,
                &fallback_name,
                &skill.assistant,
                |target_path, spec, outcome| {
                    let db = get_db();
                    save_git_origin_meta(&db, target_path, spec, outcome)
                },
            )?;
        }
        "local" => {
            let source_path = expand_path(skill.source.trim());
            install_local_package(&source_path, &target_root, &fallback_name, &skill.assistant)?;
        }
        _ => return Err("当前 manifest 仅支持 Git 仓库和本地目录来源".to_string()),
    }
    Ok(())
}

fn ensure_git_repo(repo: &Path) -> Result<(), String> {
    fs::create_dir_all(repo).map_err(|e| e.to_string())?;
    if repo.join(".git").exists() {
        return Ok(());
    }
    let out = run_command_with_timeout("git", &["init"], Some(repo), Duration::from_secs(10), &[])?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn ensure_git_identity(repo: &Path) -> Result<(), String> {
    let has_name = git_output(repo, &["config", "--get", "user.name"]).unwrap_or_default();
    if has_name.is_empty() {
        let out = run_command_with_timeout(
            "git",
            &["config", "user.name", "SkillMate"],
            Some(repo),
            Duration::from_secs(5),
            &[],
        )?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
    }
    let has_email = git_output(repo, &["config", "--get", "user.email"]).unwrap_or_default();
    if has_email.is_empty() {
        let out = run_command_with_timeout(
            "git",
            &["config", "user.email", "skillmate@local"],
            Some(repo),
            Duration::from_secs(5),
            &[],
        )?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
    }
    Ok(())
}

fn checkout_git_branch(repo: &Path, branch: &str) -> Result<(), String> {
    let branch_name = if branch.trim().is_empty() {
        "main"
    } else {
        branch.trim()
    };
    let out = run_command_with_timeout(
        "git",
        &["checkout", "-B", branch_name],
        Some(repo),
        Duration::from_secs(10),
        &[],
    )?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn configure_git_remote(repo: &Path, remote_url: &str) -> Result<(), String> {
    if remote_url.trim().is_empty() {
        return Ok(());
    }
    let current = git_output(repo, &["remote", "get-url", "origin"]).unwrap_or_default();
    let args = if current.is_empty() {
        vec!["remote", "add", "origin", remote_url]
    } else if current == remote_url {
        return Ok(());
    } else {
        vec!["remote", "set-url", "origin", remote_url]
    };
    let out = run_command_with_timeout("git", &args, Some(repo), Duration::from_secs(10), &[])?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn snapshot_assistants(repo: &Path) -> Result<(), String> {
    let snapshot_root = repo.join("assistants");
    if snapshot_root.exists() {
        fs::remove_dir_all(&snapshot_root).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&snapshot_root).map_err(|e| e.to_string())?;

    let mut manifest = Vec::new();
    for assistant in app_core::assistant_definitions() {
        let source_root = expand_path(assistant.path);
        let target_root = snapshot_root.join(assistant_slug(assistant.name));
        let mut entries = 0usize;
        if source_root.exists() {
            copy_dir_recursive(&source_root, &target_root)?;
            entries = fs::read_dir(&source_root)
                .map_err(|e| e.to_string())?
                .flatten()
                .count();
        }
        manifest.push(serde_json::json!({
            "name": assistant.name,
            "sourcePath": source_root,
            "snapshotPath": target_root,
            "exists": source_root.exists(),
            "entries": entries,
        }));
    }

    let payload = serde_json::json!({
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "assistants": manifest,
    });
    fs::write(
        repo.join("skillmate-backup.json"),
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_all_assistants() -> Vec<AIAssistant> {
    let db = get_db();
    scan_all_assistants(&db)
}

#[tauri::command]
fn get_ai_list() -> Vec<serde_json::Value> {
    app_core::assistant_definitions().iter().map(|assistant| {
        let expanded = expand_path(assistant.path);
        serde_json::json!({ "name": assistant.name, "path": expanded.to_string_lossy(), "aiType": assistant.ai_type, "icon": assistant.icon, "exists": expanded.exists() })
    }).collect()
}

#[tauri::command]
fn get_all_tags() -> Vec<Tag> {
    let db = get_db();
    let mut stmt = db.prepare("SELECT id, name, color FROM tags").unwrap();
    stmt.query_map([], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

#[tauri::command]
fn add_tag(name: String, color: String) -> Tag {
    let db = get_db();
    let id = generate_id();
    db.execute(
        "INSERT INTO tags (id, name, color) VALUES (?, ?, ?)",
        params![id, name, color],
    )
    .ok();
    Tag { id, name, color }
}

#[tauri::command]
fn delete_tag(tag_id: String) -> Result<String, String> {
    let db = get_db();
    db.execute("DELETE FROM tags WHERE id = ?", params![tag_id])
        .map_err(|e| e.to_string())?;
    Ok("已删除".to_string())
}

#[tauri::command]
fn update_skill_tags(skill_path: String, tags: Vec<String>) -> Result<String, String> {
    let db = get_db();
    let tags_str = tags.join(",");
    db.execute(
        "INSERT OR REPLACE INTO skill_tags (skill_path, tags) VALUES (?, ?)",
        params![skill_path, tags_str],
    )
    .map_err(|e| e.to_string())?;
    Ok("已更新".to_string())
}

#[tauri::command]
fn get_scenarios() -> Vec<Scenario> {
    let db = get_db();
    let mut stmt = db
        .prepare("SELECT id, name, description, skill_ids, created_at FROM scenarios")
        .unwrap();
    stmt.query_map([], |row| {
        let skill_ids_str: String = row.get(3)?;
        let skill_ids: Vec<String> = if skill_ids_str.is_empty() {
            vec![]
        } else {
            skill_ids_str.split(',').map(|s| s.to_string()).collect()
        };
        Ok(Scenario {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            skill_ids,
            created_at: row.get(4)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

#[tauri::command]
fn create_scenario(name: String, description: String, skill_ids: Vec<String>) -> Scenario {
    let db = get_db();
    let id = generate_id();
    let created_at = chrono::Local::now().format("%Y-%m-%d").to_string();
    let skill_ids_str = skill_ids.join(",");
    db.execute("INSERT INTO scenarios (id, name, description, skill_ids, created_at) VALUES (?, ?, ?, ?, ?)", params![id, name, description, skill_ids_str, created_at]).ok();
    Scenario {
        id,
        name,
        description,
        skill_ids,
        created_at,
    }
}

#[tauri::command]
fn delete_scenario(scenario_id: String) -> Result<String, String> {
    let db = get_db();
    db.execute("DELETE FROM scenarios WHERE id = ?", params![scenario_id])
        .map_err(|e| e.to_string())?;
    Ok("已删除".to_string())
}

#[tauri::command]
fn get_git_backup() -> GitBackup {
    let db = get_db();
    let result = db.query_row(
        "SELECT enabled, remote_url, repo_path, branch, last_sync FROM git_backup WHERE id = 1",
        [],
        |row| {
            Ok(GitBackup {
                enabled: row.get::<_, i32>(0)? != 0,
                remote_url: row.get(1)?,
                repo_path: row.get(2)?,
                branch: row.get(3)?,
                last_sync: row.get(4)?,
            })
        },
    );
    match result {
        Ok(gb) => gb,
        Err(_) => GitBackup {
            enabled: false,
            remote_url: String::new(),
            repo_path: String::new(),
            branch: "main".to_string(),
            last_sync: String::new(),
        },
    }
}

#[tauri::command]
fn setup_git_backup(
    repo_path: String,
    remote_url: String,
    branch: String,
) -> Result<String, String> {
    let repo = expand_path(repo_path.trim());
    if repo.to_string_lossy().trim().is_empty() {
        return Err("仓库路径不能为空".to_string());
    }
    let db = get_db();
    db.execute(
        "INSERT OR REPLACE INTO git_backup (id, enabled, remote_url, repo_path, branch, last_sync) VALUES (1, 1, ?, ?, ?, COALESCE((SELECT last_sync FROM git_backup WHERE id = 1), ''))",
        params![
            remote_url.trim(),
            repo.to_string_lossy().to_string(),
            if branch.trim().is_empty() { "main".to_string() } else { branch.trim().to_string() }
        ],
    ).map_err(|e| e.to_string())?;
    Ok("已配置".to_string())
}

#[tauri::command]
fn sync_to_git(message: String) -> Result<String, String> {
    let _guard = SYNC_LOCK.lock().unwrap();
    let db = get_db();
    let git: GitBackup = match db.query_row(
        "SELECT enabled, remote_url, repo_path, branch, last_sync FROM git_backup WHERE id = 1",
        [],
        |row| {
            Ok(GitBackup {
                enabled: row.get::<_, i32>(0)? != 0,
                remote_url: row.get(1)?,
                repo_path: row.get(2)?,
                branch: row.get(3)?,
                last_sync: row.get(4)?,
            })
        },
    ) {
        Ok(g) => g,
        Err(_) => return Err("未配置 Git 备份".to_string()),
    };
    if !git.enabled {
        return Err("Git 备份未启用".to_string());
    }
    if git.repo_path.trim().is_empty() {
        return Err("未配置仓库路径".to_string());
    }
    let repo = PathBuf::from(&git.repo_path);
    ensure_git_repo(&repo)?;
    ensure_git_identity(&repo)?;
    checkout_git_branch(&repo, &git.branch)?;
    configure_git_remote(&repo, &git.remote_url)?;
    snapshot_assistants(&repo)?;

    let add = Command::new("git")
        .args(["-C", repo.to_string_lossy().as_ref(), "add", "-A"])
        .output()
        .map_err(|e| e.to_string())?;
    if !add.status.success() {
        return Err(String::from_utf8_lossy(&add.stderr).trim().to_string());
    }
    let commit = Command::new("git")
        .args([
            "-C",
            repo.to_string_lossy().as_ref(),
            "commit",
            "-m",
            &message,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !commit.status.success() {
        let stderr = String::from_utf8_lossy(&commit.stderr);
        if !stderr.contains("nothing to commit") {
            return Err(stderr.to_string());
        }
    }
    let mut result_message = "本地快照同步成功".to_string();
    if !git.remote_url.trim().is_empty() {
        let push = Command::new("git")
            .args([
                "-C",
                repo.to_string_lossy().as_ref(),
                "push",
                "-u",
                "origin",
                git.branch.as_str(),
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !push.status.success() {
            return Err(String::from_utf8_lossy(&push.stderr).trim().to_string());
        }
        result_message = "同步并推送成功".to_string();
    }
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    db.execute(
        "UPDATE git_backup SET last_sync = ? WHERE id = 1",
        params![now],
    )
    .ok();
    Ok(result_message)
}

fn install_result(
    success: bool,
    message: impl Into<String>,
    output: impl Into<String>,
    structure: Option<SkillStructureInfo>,
) -> InstallResult {
    let structure = structure.unwrap_or_default();
    InstallResult {
        success,
        message: message.into(),
        output: output.into(),
        structure_status: structure.structure_status,
        structure_features: structure.structure_features,
        structure_warnings: structure.structure_warnings,
        manifest_title: structure.manifest_title,
        manifest_description: structure.manifest_description,
    }
}

#[tauri::command]
async fn preview_install_skill(
    package: String,
    source: String,
    assistant_name: String,
    install_mode: Option<String>,
    project_path: Option<String>,
) -> InstallPreview {
    let mode = install_mode.unwrap_or_else(|| "copy".to_string());
    let target_root = match install_target_root(&assistant_name, &mode, project_path.as_deref()) {
        Ok(path) => path,
        Err(err) => {
            return InstallPreview {
                can_install: false,
                can_apply: false,
                message: err,
                target_name: String::new(),
                target_path: String::new(),
                source_kind: source,
                package_detection: PackageDetection {
                    package_kind: "unknown".to_string(),
                    detected_skills: vec![],
                    warnings: vec!["structure_preview_failed".to_string()],
                    needs_model: true,
                },
                target_actions: vec![],
                conflicts: vec![],
                structure_status: "nonstandard".to_string(),
                structure_features: vec![],
                structure_warnings: vec!["structure_preview_failed".to_string()],
                manifest_title: None,
                manifest_description: None,
            }
        }
    };
    if mode == "symlink" {
        if source != "local" {
            return install_preview_error(
                "项目软连接安装仅支持本地目录来源",
                source,
                target_root.to_string_lossy().to_string(),
            );
        }
        let skill_name = install_target_name(&package, &source).unwrap_or_default();
        return preview_local_symlink_install(
            &expand_path(package.trim()),
            &target_root,
            &skill_name,
        );
    }
    preview_install_source(&package, &source, &target_root)
}

#[tauri::command]
async fn install_skill(
    package: String,
    source: String,
    assistant_name: String,
    install_mode: Option<String>,
    project_path: Option<String>,
) -> InstallResult {
    let mode = install_mode.unwrap_or_else(|| "copy".to_string());
    let target_root = match install_target_root(&assistant_name, &mode, project_path.as_deref()) {
        Ok(path) => path,
        Err(err) => return install_result(false, err, "", None),
    };
    if let Err(err) = fs::create_dir_all(&target_root) {
        return install_result(false, format!("无法创建目标目录: {}", err), "", None);
    }

    let skill_name = match install_target_name(&package, &source) {
        Ok(name) => name,
        Err(err) => return install_result(false, err, "", None),
    };
    let target_path = target_root.join(&skill_name);

    if mode == "symlink" {
        if source != "local" {
            return install_result(false, "项目软连接安装仅支持本地目录来源", "", None);
        }
        let source_path = expand_path(package.trim());
        return match install_local_symlink_package(
            &source_path,
            &target_root,
            &skill_name,
            &assistant_name,
        ) {
            Ok(structure) => install_result(
                true,
                format!("已软连接安装到 {}", target_root.to_string_lossy()),
                "",
                Some(structure),
            ),
            Err(err) => install_result(false, err, "", None),
        };
    }

    match source.as_str() {
        source_kind if is_git_install_source(source_kind) => {
            let spec = match parse_git_install_spec(&package) {
                Ok(spec) => spec,
                Err(err) => return install_result(false, err, "", None),
            };
            match install_git_package(
                spec.clone(),
                &target_root,
                &skill_name,
                &assistant_name,
                |target_path, spec, outcome| {
                    let db = get_db();
                    save_git_origin_meta(&db, target_path, spec, outcome)
                },
            ) {
                Ok(outcome) => install_result(
                    true,
                    format!("已安装到 {}", assistant_name),
                    "",
                    Some(outcome.structure),
                ),
                Err(err) => {
                    if target_path.exists() {
                        let _ = remove_existing_path(&target_path);
                    }
                    install_result(false, "安装失败", err, None)
                }
            }
        }
        "local" => {
            let source_path = expand_path(package.trim());
            match install_local_package(&source_path, &target_root, &skill_name, &assistant_name) {
                Ok(structure) => install_result(
                    true,
                    format!("已安装到 {}", assistant_name),
                    "",
                    Some(structure),
                ),
                Err(err) => install_result(false, err, "", None),
            }
        }
        _ => install_result(false, "当前版本仅支持 Git 仓库和本地目录安装", "", None),
    }
}

fn install_target_root(
    assistant_name: &str,
    install_mode: &str,
    project_path: Option<&str>,
) -> Result<PathBuf, String> {
    if install_mode == "symlink" {
        let raw_project = project_path
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .ok_or_else(|| "项目软连接安装需要填写项目路径".to_string())?;
        let project = expand_path(raw_project);
        return project_skill_root_by_name(assistant_name, &project);
    }
    assistant_root_by_name(assistant_name)
}

fn install_preview_error(
    message: impl Into<String>,
    source_kind: String,
    target_path: String,
) -> InstallPreview {
    InstallPreview {
        can_install: false,
        can_apply: false,
        message: message.into(),
        target_name: String::new(),
        target_path,
        source_kind,
        package_detection: PackageDetection {
            package_kind: "unknown".to_string(),
            detected_skills: vec![],
            warnings: vec!["unrecognized_input".to_string()],
            needs_model: false,
        },
        target_actions: vec![],
        conflicts: vec![],
        structure_status: "nonstandard".to_string(),
        structure_features: vec![],
        structure_warnings: vec!["unrecognized_input".to_string()],
        manifest_title: None,
        manifest_description: None,
    }
}

#[tauri::command]
fn delete_skill(path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err("路径不存在".to_string());
    }
    if !app_core::is_managed_skill_path(&p, &managed_skill_roots()) {
        return Err("不允许删除".to_string());
    }
    let db = get_db();
    let db_managed = load_origin_meta(&db, &p.to_string_lossy())
        .map(|meta| meta.managed_by_app)
        .unwrap_or(false);
    let state_managed = p
        .parent()
        .map(|root| is_managed_by_state(root, &p))
        .unwrap_or(false);
    if !db_managed && !state_managed {
        return Err("只允许删除 SkillMate 管理的 Skill".to_string());
    }
    remove_existing_path(&p)?;
    Ok("已删除".to_string())
}

#[tauri::command]
fn unlink_symlink_skill(path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    if !p.exists() && fs::symlink_metadata(&p).is_err() {
        return Err("路径不存在".to_string());
    }
    let metadata = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_symlink() {
        return Err("目标不是软连接".to_string());
    }
    if !app_core::is_managed_link_entry_path(&p, &managed_skill_roots()) {
        return Err("不允许解除非受管路径".to_string());
    }
    let state_managed = p
        .parent()
        .map(|root| is_managed_by_state(root, &p))
        .unwrap_or(false);
    if !state_managed {
        return Err("只允许解除 SkillMate 创建的软连接".to_string());
    }
    fs::remove_file(&p).map_err(|e| e.to_string())?;
    Ok("已解除软连接".to_string())
}

#[tauri::command]
fn get_skill_readme(path: String) -> String {
    read_skill_preview(&PathBuf::from(path))
}

#[tauri::command]
fn inspect_skill_structure(path: String) -> SkillStructureInfo {
    analyze_skill_structure(&expand_path(path.trim()))
}

#[tauri::command]
fn inspect_skill_validation(path: String) -> SkillValidationReport {
    validate_skill_structure(&expand_path(path.trim()))
}

#[tauri::command]
fn detect_skill_package_path(path: String) -> PackageDetection {
    detect_skill_package(&expand_path(path.trim()))
}

#[tauri::command]
fn detect_install_source(input: String) -> InstallDetection {
    detect_install_source_rules(&input)
}

#[tauri::command]
fn preview_project_skill_targets(
    project_path: String,
) -> Result<Vec<ProjectSkillTargetPreview>, String> {
    let project = expand_path(project_path.trim());
    if project_path.trim().is_empty() {
        return Err("项目路径不能为空".to_string());
    }
    let mut previews = Vec::new();
    let mut any_exists = false;
    for assistant in app_core::assistant_definitions() {
        let target = project_skill_root_by_name(assistant.name, &project)?;
        let exists = target.exists();
        any_exists = any_exists || exists;
        previews.push(ProjectSkillTargetPreview {
            assistant: assistant.name.to_string(),
            target_path: target.to_string_lossy().to_string(),
            exists,
            recommended: false,
        });
    }
    if any_exists {
        for preview in &mut previews {
            preview.recommended = preview.exists;
        }
    } else if let Some(first) = previews.first_mut() {
        first.recommended = true;
    }
    Ok(previews)
}

#[tauri::command]
fn preview_model_assist(request: ModelAssistRequest) -> Result<ModelAssistResult, String> {
    let _ = request;
    Err("模型辅助尚未配置，当前仅使用本地规则识别".to_string())
}

#[tauri::command]
fn check_update(path: String, force: Option<bool>) -> serde_json::Value {
    let db = get_db();
    match probe_skill_state(&db, &PathBuf::from(&path), force.unwrap_or(false)) {
        Ok(info) => sync_info_json(&info),
        Err(err) => serde_json::json!({
            "originKind": "unknown",
            "originLocator": "",
            "resolvedLocator": "",
            "trackingRef": "",
            "installedRef": "",
            "latestRef": "",
            "syncState": "failed",
            "message": format!("检查失败: {}", err),
            "lagCount": 0,
            "lastProbeAt": now_ms(),
            "lastSyncAt": null,
            "managedByApp": false,
            "canSync": false,
            "hasUpdate": false,
            "behindCount": 0,
            "remoteUrl": ""
        }),
    }
}

#[tauri::command]
fn update_from_upstream(path: String) -> Result<String, String> {
    let _guard = SYNC_LOCK.lock().unwrap();
    let db = get_db();
    let p = PathBuf::from(&path);
    update_skill_from_upstream(&db, &p)
}

#[tauri::command]
fn open_folder(path: String) {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer").arg(&path).spawn().ok();
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(&path).spawn().ok();
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(&path).spawn().ok();
    }
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if url.is_empty()
        || url.contains(['\n', '\r', '\0'])
        || !(lower.starts_with("https://") || lower.starts_with("http://"))
    {
        return Err("只允许打开 http/https URL".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn search_marketplace(_query: String) -> Vec<serde_json::Value> {
    Vec::new()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = get_db();
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_all_assistants,
            get_ai_list,
            get_all_tags,
            add_tag,
            delete_tag,
            update_skill_tags,
            get_scenarios,
            create_scenario,
            delete_scenario,
            get_git_backup,
            setup_git_backup,
            sync_to_git,
            preview_install_skill,
            install_skill,
            delete_skill,
            unlink_symlink_skill,
            get_skill_readme,
            inspect_skill_structure,
            inspect_skill_validation,
            detect_skill_package_path,
            detect_install_source,
            preview_project_skill_targets,
            preview_model_assist,
            check_update,
            update_from_upstream,
            export_library,
            preview_import_library,
            import_library,
            export_scenario_manifest,
            preview_import_scenario_manifest,
            import_scenario_manifest,
            export_skillmate_manifest,
            preview_apply_skillmate_manifest,
            apply_skillmate_manifest,
            get_skill_profiles,
            save_current_skill_profile,
            preview_apply_skill_profile,
            apply_skill_profile,
            rollback_skill_profile,
            open_folder,
            open_url,
            search_marketplace
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_manifest::{LibraryExport, LibrarySkillRecord};
    use crate::scenario_manifest::ScenarioManifest;

    fn test_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute(
            "CREATE TABLE tags (id TEXT PRIMARY KEY, name TEXT NOT NULL, color TEXT NOT NULL)",
            [],
        )
        .unwrap();
        db.execute("CREATE TABLE scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT, skill_ids TEXT, created_at TEXT)", []).unwrap();
        db.execute(
            "CREATE TABLE skill_tags (skill_path TEXT PRIMARY KEY, tags TEXT)",
            [],
        )
        .unwrap();
        db
    }

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("skillmate-test-{}-{}", name, now_ms()))
    }

    #[test]
    fn managed_skill_path_rejects_root_and_parent_escape() {
        let base = test_dir("managed-path");
        let root = base.join("skills");
        let skill = root.join("skill-a");
        let outside = base.join("outside");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        assert!(app_core::is_managed_skill_path(&skill, &[root.clone()]));
        assert!(!app_core::is_managed_skill_path(&root, &[root.clone()]));
        assert!(!app_core::is_managed_skill_path(
            &root.join("..").join("outside"),
            &[root.clone()]
        ));

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    #[cfg(unix)]
    fn managed_link_entry_path_allows_external_symlink_target() {
        let base = test_dir("managed-link-path");
        let root = base.join("skills");
        let source = base
            .join("project")
            .join(".codex")
            .join("skills")
            .join("skill-a");
        let link = root.join("skill-a");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        std::os::unix::fs::symlink(&source, &link).unwrap();

        assert!(app_core::is_managed_link_entry_path(&link, &[root.clone()]));
        assert!(!app_core::is_managed_skill_path(&link, &[root.clone()]));
        assert!(!app_core::is_managed_link_entry_path(
            &root.join("..").join("outside-link"),
            &[root.clone()]
        ));

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn build_library_export_flattens_skills() {
        let export = build_library_export(
            vec![Tag {
                id: "1".into(),
                name: "AI".into(),
                color: "#fff".into(),
            }],
            vec![Scenario {
                id: "s1".into(),
                name: "写作".into(),
                description: "desc".into(),
                skill_ids: vec!["/tmp/a".into()],
                created_at: "2026-04-20".into(),
            }],
            vec![AIAssistant {
                name: "Codex".into(),
                path: "/Users/demo/.codex/skills".into(),
                ai_type: "skill".into(),
                icon: "📝".into(),
                exists: true,
                skills: vec![Skill {
                    id: "/tmp/a".into(),
                    name: "skill-a".into(),
                    path: "/tmp/a".into(),
                    skill_type: "skill-folder".into(),
                    source: "GitHub".into(),
                    source_type: "git".into(),
                    size: "1 KB".into(),
                    modified: "".into(),
                    tags: vec!["1".into()],
                    description: "".into(),
                    readme: "".into(),
                    version: "1.0.0".into(),
                    upstream_url: "".into(),
                    has_update: false,
                    compatible_with: vec!["Codex".into()],
                    usage_count: 0,
                    origin_kind: "git".into(),
                    origin_locator: "".into(),
                    resolved_locator: "".into(),
                    tracking_ref: "".into(),
                    installed_ref: "".into(),
                    latest_ref: "".into(),
                    sync_state: "current".into(),
                    sync_message: "".into(),
                    lag_count: 0,
                    last_probe_at: None,
                    last_sync_at: None,
                    managed_by_app: false,
                    can_sync: false,
                    symlink_source: None,
                    structure_status: "complete".into(),
                    structure_features: vec!["skill_md".into()],
                    structure_warnings: vec![],
                    manifest_title: Some("skill-a".into()),
                    manifest_description: Some("desc".into()),
                }],
            }],
        );

        assert_eq!(export.version, 1);
        assert_eq!(export.tags.len(), 1);
        assert_eq!(export.scenarios.len(), 1);
        assert_eq!(export.skills.len(), 1);
        assert_eq!(export.skills[0].assistant, "Codex");
        assert_eq!(export.skills[0].path, "/tmp/a");
    }

    #[test]
    fn merge_imported_library_upserts_tags_and_scenarios() {
        let db = test_db();
        let (tag_count, scenario_count) = merge_imported_library(
            &db,
            LibraryExport {
                version: 1,
                exported_at: "2026-04-20T00:00:00Z".into(),
                tags: vec![Tag {
                    id: "1".into(),
                    name: "AI".into(),
                    color: "#fff".into(),
                }],
                scenarios: vec![Scenario {
                    id: "s1".into(),
                    name: "写作".into(),
                    description: "desc".into(),
                    skill_ids: vec!["/tmp/a".into(), "/tmp/b".into()],
                    created_at: "2026-04-20".into(),
                }],
                skills: vec![],
            },
            false,
        )
        .unwrap();

        assert_eq!(tag_count, 1);
        assert_eq!(scenario_count, 1);

        let tag_name: String = db
            .query_row("SELECT name FROM tags WHERE id = '1'", [], |row| row.get(0))
            .unwrap();
        let skill_ids: String = db
            .query_row(
                "SELECT skill_ids FROM scenarios WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tag_name, "AI");
        assert_eq!(skill_ids, "/tmp/a,/tmp/b");
    }

    #[test]
    fn merge_imported_library_restores_skill_tag_mapping() {
        let db = test_db();
        merge_imported_library(
            &db,
            LibraryExport {
                version: 1,
                exported_at: "2026-04-20T00:00:00Z".into(),
                tags: vec![],
                scenarios: vec![],
                skills: vec![LibrarySkillRecord {
                    name: "skill-a".into(),
                    path: "/tmp/a".into(),
                    assistant: "Codex".into(),
                    source_type: "git".into(),
                    tags: vec!["1".into(), "2".into()],
                }],
            },
            false,
        )
        .unwrap();

        let stored_tags: String = db
            .query_row(
                "SELECT tags FROM skill_tags WHERE skill_path = '/tmp/a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_tags, "1,2");
    }

    #[test]
    fn replace_import_clears_existing_records_before_restore() {
        let db = test_db();
        db.execute(
            "INSERT INTO tags (id, name, color) VALUES ('old-tag', '旧标签', '#000')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO scenarios (id, name, description, skill_ids, created_at) VALUES ('old-scenario', '旧场景', '', '/tmp/old', '2026-04-19')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO skill_tags (skill_path, tags) VALUES ('/tmp/old', 'old-tag')",
            [],
        )
        .unwrap();

        merge_imported_library(
            &db,
            LibraryExport {
                version: 1,
                exported_at: "2026-04-20T00:00:00Z".into(),
                tags: vec![Tag {
                    id: "new-tag".into(),
                    name: "新标签".into(),
                    color: "#fff".into(),
                }],
                scenarios: vec![Scenario {
                    id: "new-scenario".into(),
                    name: "新场景".into(),
                    description: "".into(),
                    skill_ids: vec!["/tmp/new".into()],
                    created_at: "2026-04-20".into(),
                }],
                skills: vec![LibrarySkillRecord {
                    name: "skill-new".into(),
                    path: "/tmp/new".into(),
                    assistant: "Codex".into(),
                    source_type: "git".into(),
                    tags: vec!["new-tag".into()],
                }],
            },
            true,
        )
        .unwrap();

        let tag_count: i64 = db
            .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
            .unwrap();
        let scenario_count: i64 = db
            .query_row("SELECT COUNT(*) FROM scenarios", [], |row| row.get(0))
            .unwrap();
        let skill_tag_count: i64 = db
            .query_row("SELECT COUNT(*) FROM skill_tags", [], |row| row.get(0))
            .unwrap();
        let restored_tag: String = db
            .query_row(
                "SELECT tags FROM skill_tags WHERE skill_path = '/tmp/new'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(tag_count, 1);
        assert_eq!(scenario_count, 1);
        assert_eq!(skill_tag_count, 1);
        assert_eq!(restored_tag, "new-tag");
    }

    #[test]
    fn preview_import_library_counts_merge_changes() {
        let db = test_db();
        db.execute(
            "INSERT INTO tags (id, name, color) VALUES ('shared-tag', '共享标签', '#000')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO scenarios (id, name, description, skill_ids, created_at) VALUES ('shared-scenario', '共享场景', '', '/tmp/existing', '2026-04-20')",
            [],
        )
        .unwrap();

        let preview = preview_imported_library(
            &db,
            &LibraryExport {
                version: 1,
                exported_at: "2026-04-21T00:00:00Z".into(),
                tags: vec![
                    Tag {
                        id: "shared-tag".into(),
                        name: "共享标签".into(),
                        color: "#fff".into(),
                    },
                    Tag {
                        id: "new-tag".into(),
                        name: "新标签".into(),
                        color: "#123".into(),
                    },
                ],
                scenarios: vec![
                    Scenario {
                        id: "shared-scenario".into(),
                        name: "共享场景".into(),
                        description: "".into(),
                        skill_ids: vec!["/tmp/existing".into()],
                        created_at: "2026-04-21".into(),
                    },
                    Scenario {
                        id: "new-scenario".into(),
                        name: "新场景".into(),
                        description: "".into(),
                        skill_ids: vec!["/tmp/new".into()],
                        created_at: "2026-04-21".into(),
                    },
                ],
                skills: vec![
                    LibrarySkillRecord {
                        name: "skill-a".into(),
                        path: "/tmp/existing".into(),
                        assistant: "Codex".into(),
                        source_type: "git".into(),
                        tags: vec!["shared-tag".into()],
                    },
                    LibrarySkillRecord {
                        name: "skill-b".into(),
                        path: "/tmp/new".into(),
                        assistant: "Codex".into(),
                        source_type: "local".into(),
                        tags: vec!["new-tag".into()],
                    },
                    LibrarySkillRecord {
                        name: "skill-c".into(),
                        path: "/tmp/untagged".into(),
                        assistant: "Codex".into(),
                        source_type: "local".into(),
                        tags: vec![],
                    },
                ],
            },
            false,
        )
        .unwrap();

        assert!(!preview.replace_existing);
        assert_eq!(preview.tags_to_add, 1);
        assert_eq!(preview.tags_to_replace, 1);
        assert_eq!(preview.scenarios_to_add, 1);
        assert_eq!(preview.scenarios_to_replace, 1);
        assert_eq!(preview.skill_tag_writes, 2);
        assert_eq!(preview.existing_tags_to_remove, 0);
        assert_eq!(preview.existing_scenarios_to_remove, 0);
        assert_eq!(preview.existing_skill_tag_mappings_to_remove, 0);
    }

    #[test]
    fn preview_import_library_counts_replace_cleanup() {
        let db = test_db();
        db.execute(
            "INSERT INTO tags (id, name, color) VALUES ('shared-tag', '共享标签', '#000')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO tags (id, name, color) VALUES ('old-tag', '旧标签', '#111')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO scenarios (id, name, description, skill_ids, created_at) VALUES ('shared-scenario', '共享场景', '', '/tmp/existing', '2026-04-20')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO scenarios (id, name, description, skill_ids, created_at) VALUES ('old-scenario', '旧场景', '', '/tmp/old', '2026-04-20')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO skill_tags (skill_path, tags) VALUES ('/tmp/existing', 'shared-tag')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO skill_tags (skill_path, tags) VALUES ('/tmp/old', 'old-tag')",
            [],
        )
        .unwrap();

        let preview = preview_imported_library(
            &db,
            &LibraryExport {
                version: 1,
                exported_at: "2026-04-21T00:00:00Z".into(),
                tags: vec![
                    Tag {
                        id: "shared-tag".into(),
                        name: "共享标签".into(),
                        color: "#fff".into(),
                    },
                    Tag {
                        id: "new-tag".into(),
                        name: "新标签".into(),
                        color: "#123".into(),
                    },
                ],
                scenarios: vec![
                    Scenario {
                        id: "shared-scenario".into(),
                        name: "共享场景".into(),
                        description: "".into(),
                        skill_ids: vec!["/tmp/existing".into()],
                        created_at: "2026-04-21".into(),
                    },
                    Scenario {
                        id: "new-scenario".into(),
                        name: "新场景".into(),
                        description: "".into(),
                        skill_ids: vec!["/tmp/new".into()],
                        created_at: "2026-04-21".into(),
                    },
                ],
                skills: vec![LibrarySkillRecord {
                    name: "skill-a".into(),
                    path: "/tmp/new".into(),
                    assistant: "Codex".into(),
                    source_type: "git".into(),
                    tags: vec!["new-tag".into()],
                }],
            },
            true,
        )
        .unwrap();

        assert!(preview.replace_existing);
        assert_eq!(preview.tags_to_add, 1);
        assert_eq!(preview.tags_to_replace, 1);
        assert_eq!(preview.scenarios_to_add, 1);
        assert_eq!(preview.scenarios_to_replace, 1);
        assert_eq!(preview.skill_tag_writes, 1);
        assert_eq!(preview.existing_tags_to_remove, 2);
        assert_eq!(preview.existing_scenarios_to_remove, 2);
        assert_eq!(preview.existing_skill_tag_mappings_to_remove, 2);
    }

    #[test]
    fn scenario_manifest_preview_reports_changes_and_missing_refs() {
        let db = test_db();
        db.execute(
            "INSERT INTO scenarios (id, name, description, skill_ids, created_at) VALUES ('shared', '旧场景', '', '/tmp/known', '2026-04-20')",
            [],
        )
        .unwrap();
        let manifest = ScenarioManifest {
            version: 1,
            exported_at: "2026-04-21T00:00:00Z".into(),
            scenarios: vec![
                Scenario {
                    id: "shared".into(),
                    name: "共享场景".into(),
                    description: "".into(),
                    skill_ids: vec!["/tmp/known".into()],
                    created_at: "2026-04-21".into(),
                },
                Scenario {
                    id: "new".into(),
                    name: "新场景".into(),
                    description: "".into(),
                    skill_ids: vec!["/tmp/missing".into()],
                    created_at: "2026-04-21".into(),
                },
            ],
        };

        let preview =
            preview_scenario_manifest(&db, &manifest, false, &["/tmp/known".into()]).unwrap();

        assert!(!preview.replace_existing);
        assert_eq!(preview.scenarios_to_add, 1);
        assert_eq!(preview.scenarios_to_replace, 1);
        assert_eq!(preview.existing_scenarios_to_remove, 0);
        assert_eq!(preview.missing_skill_refs, vec!["/tmp/missing".to_string()]);
    }

    #[test]
    fn replace_scenario_manifest_clears_existing_scenarios_only() {
        let db = test_db();
        db.execute(
            "INSERT INTO scenarios (id, name, description, skill_ids, created_at) VALUES ('old', '旧场景', '', '/tmp/old', '2026-04-20')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO tags (id, name, color) VALUES ('keep-tag', '保留标签', '#fff')",
            [],
        )
        .unwrap();
        let manifest = ScenarioManifest {
            version: 1,
            exported_at: "2026-04-21T00:00:00Z".into(),
            scenarios: vec![Scenario {
                id: "new".into(),
                name: "新场景".into(),
                description: "desc".into(),
                skill_ids: vec!["/tmp/new".into()],
                created_at: "2026-04-21".into(),
            }],
        };

        let count = merge_scenario_manifest(&db, manifest, true).unwrap();

        assert_eq!(count, 1);
        let scenario_count: i64 = db
            .query_row("SELECT COUNT(*) FROM scenarios", [], |row| row.get(0))
            .unwrap();
        let tag_count: i64 = db
            .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
            .unwrap();
        assert_eq!(scenario_count, 1);
        assert_eq!(tag_count, 1);
    }
}
