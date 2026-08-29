use crate::app_core::{assistant_definitions, generate_id};
use crate::database::{database_path_key, PathColumn};
use crate::managed_installation::{
    prune_missing_managed_installations, refresh_managed_installation,
};
use crate::managed_state::content_fingerprint;
use crate::managed_state::refresh_managed_skill_fingerprint;
use crate::skill_install::{InstallPreview, PreviewAction, PreviewConflict};
use crate::skill_inventory::{build_skill, ManagedSkill};
use crate::skill_origin::OriginInferenceCache;
use crate::Skill;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
thread_local! {
    static TEST_LIBRARY_ROOT: std::cell::RefCell<Option<PathBuf>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
pub struct TestLibraryRootGuard(Option<PathBuf>);

#[cfg(test)]
impl Drop for TestLibraryRootGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        TEST_LIBRARY_ROOT.with(|root| *root.borrow_mut() = previous);
    }
}

#[cfg(test)]
pub fn use_test_library_root(path: PathBuf) -> TestLibraryRootGuard {
    let previous = TEST_LIBRARY_ROOT.with(|root| root.replace(Some(path)));
    TestLibraryRootGuard(previous)
}

#[derive(Debug, Clone)]
pub struct SkillDeployment {
    pub library_path: PathBuf,
}

pub fn reuse_library_preview(mut preview: InstallPreview) -> InstallPreview {
    let mut reused_targets = Vec::new();
    for action in &mut preview.target_actions {
        if action.action == "skip"
            && action.reason == "目标目录已存在"
            && Path::new(&action.target).is_dir()
        {
            reused_targets.push(action.target.clone());
            action.action = "keep".to_string();
            action.source = action.target.clone();
            action.reason = "已在 SkillMate 库中".to_string();
        }
    }
    preview.conflicts.retain(|conflict| {
        conflict.reason != "target_exists" || !reused_targets.contains(&conflict.target)
    });
    if !preview
        .conflicts
        .iter()
        .any(|conflict| conflict.reason == "target_exists")
    {
        preview
            .package_detection
            .warnings
            .retain(|warning| warning != "target_exists");
        preview
            .structure_warnings
            .retain(|warning| warning != "target_exists");
    }
    preview.can_install = preview.conflicts.is_empty() && !preview.target_actions.is_empty();
    preview.can_apply = preview.can_install;
    if preview.can_apply {
        preview.message = "Skill 已在 SkillMate 库中，将创建新的启用位置".to_string();
    }
    preview
}

pub fn add_deployment_to_preview(
    mut preview: InstallPreview,
    deployment_root: &Path,
    assistant_name: &str,
    scope: &str,
    replace_existing: bool,
) -> InstallPreview {
    if preview.selection_required {
        return preview;
    }
    let mut deployment_actions = Vec::new();
    for action in preview
        .target_actions
        .iter()
        .filter(|action| matches!(action.action.as_str(), "copy" | "keep"))
    {
        let source = PathBuf::from(&action.target);
        let Some(name) = source.file_name() else {
            continue;
        };
        let target = deployment_root.join(name);
        let alternate = (scope == "global")
            .then(|| {
                assistant_definitions()
                    .iter()
                    .find(|assistant| assistant.name == assistant_name)
                    .into_iter()
                    .flat_map(|assistant| assistant.global_discovery_roots())
                    .map(|root| root.join(name))
                    .collect::<Vec<_>>()
            })
            .and_then(|candidates| find_existing_alternate_target(&target, &candidates));
        if let Some(candidate) = alternate {
            preview.can_install = false;
            preview.can_apply = false;
            preview.conflicts.push(PreviewConflict {
                target: candidate.to_string_lossy().to_string(),
                reason: "assistant_discovery_target_exists".to_string(),
            });
            deployment_actions.push(PreviewAction {
                action: "skip".to_string(),
                source: source.to_string_lossy().to_string(),
                target: candidate.to_string_lossy().to_string(),
                reason: "该平台的其他发现目录已有同名 Skill，不会创建重复启用位置".to_string(),
            });
        } else if replace_existing {
            deployment_actions.push(PreviewAction {
                action: "replace".to_string(),
                source: source.to_string_lossy().to_string(),
                target: target.to_string_lossy().to_string(),
                reason: "替换现有受管启用位置".to_string(),
            });
        } else if target.exists() || fs::symlink_metadata(&target).is_ok() {
            preview.can_install = false;
            preview.can_apply = false;
            preview.conflicts.push(PreviewConflict {
                target: target.to_string_lossy().to_string(),
                reason: "external_target_exists".to_string(),
            });
            deployment_actions.push(PreviewAction {
                action: "skip".to_string(),
                source: source.to_string_lossy().to_string(),
                target: target.to_string_lossy().to_string(),
                reason: "启用位置已被现有 Skill 占用，不会覆盖".to_string(),
            });
        } else if scope == "global" {
            deployment_actions.push(PreviewAction {
                action: "symlink".to_string(),
                source: source.to_string_lossy().to_string(),
                target: target.to_string_lossy().to_string(),
                reason: "在所有项目启用".to_string(),
            });
        } else {
            deployment_actions.push(PreviewAction {
                action: "symlink".to_string(),
                source: source.to_string_lossy().to_string(),
                target: target.to_string_lossy().to_string(),
                reason: "在当前项目启用".to_string(),
            });
        }
    }
    preview.target_actions.extend(deployment_actions);
    if preview.can_apply {
        preview.message = format!(
            "将在{}中启用 {} 个 Skill",
            if scope == "project" {
                "当前项目"
            } else {
                "所选平台"
            },
            preview.package_detection.detected_skills.len(),
        );
    }
    preview
}

pub(crate) fn find_existing_alternate_target(
    target: &Path,
    candidates: &[PathBuf],
) -> Option<PathBuf> {
    candidates.iter().find_map(|candidate| {
        (candidate != target && (candidate.exists() || fs::symlink_metadata(candidate).is_ok()))
            .then(|| candidate.clone())
    })
}

pub fn deploy_library_skill(source: &Path, target: &Path) -> Result<String, String> {
    if target.exists() || fs::symlink_metadata(target).is_ok() {
        return Err(format!(
            "启用位置已被其他 Skill 占用: {}",
            target.to_string_lossy()
        ));
    }
    let parent = target
        .parent()
        .ok_or_else(|| "启用位置缺少父目录".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target).map_err(|error| error.to_string())?;
        Ok("symlink".to_string())
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(source, target).map_err(|error| {
            format!("Windows 无法创建 Skill 启用链接，请开启开发者模式后重试: {error}")
        })?;
        Ok("symlink".to_string())
    }
}

pub fn resolve_library_path(db: &Connection, path: &Path) -> Result<PathBuf, String> {
    Ok(find_deployment(db, path)?
        .map(|deployment| deployment.library_path)
        .unwrap_or_else(|| path.to_path_buf()))
}

pub fn library_root() -> Result<PathBuf, String> {
    #[cfg(test)]
    if let Some(root) = TEST_LIBRARY_ROOT.with(|value| value.borrow().clone()) {
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        return Ok(root);
    }
    let root = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("skillmate")
        .join("skills");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    Ok(root)
}

pub fn is_library_skill_path(path: &Path) -> bool {
    let Ok(root) =
        library_root().and_then(|root| root.canonicalize().map_err(|error| error.to_string()))
    else {
        return false;
    };
    let Ok(candidate) = path.canonicalize() else {
        return false;
    };
    candidate.parent() == Some(root.as_path())
}

pub fn library_skill_id(db: &Connection, path: &Path) -> Result<String, String> {
    let path_key = database_path_key(db, PathColumn::LibrarySkill, path)?;
    db.query_row(
        "SELECT id FROM library_skills WHERE library_path = ?",
        [path_key],
        |row| row.get::<_, String>(0),
    )
    .map_err(|error| format!("SkillMate 库记录不存在: {error}"))
}

pub fn scan_unassigned_library_skills(db: &Connection) -> Result<Vec<Skill>, String> {
    prune_missing_managed_installations(db)?;
    library_root()?;
    let mut statement = db
        .prepare(
            "SELECT library_path, name
             FROM library_skills
             WHERE NOT EXISTS (
                SELECT 1 FROM skill_deployments
                WHERE skill_deployments.library_path = library_skills.library_path
             )
             ORDER BY name, library_path",
        )
        .map_err(|error| error.to_string())?;
    let entries = statement
        .query_map([], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                row.get::<_, String>(1)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    let mut origin_cache = OriginInferenceCache::default();
    let mut skills = Vec::new();
    for (path, name) in entries {
        if !path.is_dir() || !is_library_skill_path(&path) {
            continue;
        }
        skills.push(build_skill(
            db,
            &ManagedSkill { path, name },
            &mut origin_cache,
        ));
    }
    Ok(skills)
}

pub fn register_library_skill(
    db: &Connection,
    path: &Path,
    source: &str,
    source_kind: &str,
    resolved_ref: Option<&str>,
) -> Result<String, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "SkillMate 库目录名无效".to_string())?;
    let path_value = database_path_key(db, PathColumn::LibrarySkill, path)?;
    let existing_id = db
        .query_row(
            "SELECT id FROM library_skills WHERE library_path = ?",
            [&path_value],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let id = existing_id.unwrap_or_else(generate_id);
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(
        "INSERT INTO library_skills (
            id, name, library_path, source, source_kind, resolved_ref, content_hash,
            created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(library_path) DO UPDATE SET
            name = excluded.name,
            source = excluded.source,
            source_kind = excluded.source_kind,
            resolved_ref = excluded.resolved_ref,
            content_hash = excluded.content_hash,
            updated_at = excluded.updated_at",
        params![
            &id,
            name,
            &path_value,
            source,
            source_kind,
            resolved_ref,
            content_fingerprint(path)?,
            &now,
            &now,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub fn register_deployment(
    db: &Connection,
    skill_id: &str,
    library_path: &Path,
    target_path: &Path,
    assistant: &str,
    scope: &str,
    project_path: Option<&str>,
    deploy_mode: &str,
) -> Result<(), String> {
    let target_key = database_path_key(db, PathColumn::DeploymentTarget, target_path)?;
    let library_key = database_path_key(db, PathColumn::LibrarySkill, library_path)?;
    db.execute(
        "INSERT INTO skill_deployments (
            target_path, skill_id, library_path, assistant, scope, project_path,
            deploy_mode, deployed_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(target_path) DO UPDATE SET
            skill_id = excluded.skill_id,
            library_path = excluded.library_path,
            assistant = excluded.assistant,
            scope = excluded.scope,
            project_path = excluded.project_path,
            deploy_mode = excluded.deploy_mode,
            deployed_at = excluded.deployed_at",
        params![
            target_key,
            skill_id,
            library_key,
            assistant,
            scope,
            project_path,
            deploy_mode,
            chrono::Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn find_deployment(
    db: &Connection,
    target_path: &Path,
) -> Result<Option<SkillDeployment>, String> {
    if !table_exists(db, "skill_deployments")? {
        return Ok(None);
    }
    let target_key = database_path_key(db, PathColumn::DeploymentTarget, target_path)?;
    db.query_row(
        "SELECT library_path, target_path, deploy_mode
         FROM skill_deployments WHERE target_path = ?",
        [target_key],
        |row| {
            Ok(SkillDeployment {
                library_path: PathBuf::from(row.get::<_, String>(0)?),
            })
        },
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn remove_deployment(db: &Connection, target_path: &Path) -> Result<(), String> {
    if !table_exists(db, "skill_deployments")? {
        return Ok(());
    }
    let target_key = database_path_key(db, PathColumn::DeploymentTarget, target_path)?;
    db.execute(
        "DELETE FROM skill_deployments WHERE target_path = ?",
        [target_key],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn copy_origin_to_deployment(
    db: &Connection,
    library_path: &Path,
    target_path: &Path,
) -> Result<(), String> {
    let target_key = database_path_key(db, PathColumn::SkillOrigin, target_path)?;
    let library_key = database_path_key(db, PathColumn::SkillOrigin, library_path)?;
    db.execute(
        "INSERT INTO skill_origin_meta (
            skill_path, origin_kind, origin_locator, resolved_locator, tracking_ref,
            installed_ref, latest_ref, sync_state, sync_message, lag_count,
            last_probe_at, last_sync_at, managed_by_app
         )
         SELECT ?, origin_kind, origin_locator, resolved_locator, tracking_ref,
            installed_ref, latest_ref, sync_state, sync_message, lag_count,
            last_probe_at, last_sync_at, 1
         FROM skill_origin_meta WHERE skill_path = ?
         ON CONFLICT(skill_path) DO UPDATE SET
            origin_kind = excluded.origin_kind,
            origin_locator = excluded.origin_locator,
            resolved_locator = excluded.resolved_locator,
            tracking_ref = excluded.tracking_ref,
            installed_ref = excluded.installed_ref,
            latest_ref = excluded.latest_ref,
            sync_state = excluded.sync_state,
            sync_message = excluded.sync_message,
            lag_count = excluded.lag_count,
            last_probe_at = excluded.last_probe_at,
            last_sync_at = excluded.last_sync_at,
            managed_by_app = 1",
        params![target_key, library_key,],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn refresh_deployment_origins(db: &Connection, library_path: &Path) -> Result<(), String> {
    let library_key = database_path_key(db, PathColumn::LibrarySkill, library_path)?;
    let origin_key = database_path_key(db, PathColumn::SkillOrigin, library_path)?;
    let registered = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM library_skills WHERE library_path = ?)",
            [&library_key],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| error.to_string())?;
    if !registered {
        return Ok(());
    }
    let updated = db
        .execute(
            "UPDATE library_skills
             SET content_hash = ?,
                 resolved_ref = COALESCE(
                    NULLIF((SELECT installed_ref FROM skill_origin_meta WHERE skill_path = ?), ''),
                    resolved_ref
                 ),
                 updated_at = ?
             WHERE library_path = ?",
            params![
                content_fingerprint(library_path)?,
                origin_key,
                chrono::Utc::now().to_rfc3339(),
                &library_key,
            ],
        )
        .map_err(|error| error.to_string())?;
    debug_assert_eq!(updated, 1);
    if !table_exists(db, "skill_deployments")? {
        return Ok(());
    }
    let targets = deployment_targets_for_library(db, library_path)?;
    for target in targets {
        let target_path = Path::new(&target);
        copy_origin_to_deployment(db, library_path, target_path)?;
        let root = target_path
            .parent()
            .ok_or_else(|| format!("启用位置缺少父目录: {target}"))?;
        if !refresh_managed_skill_fingerprint(root, target_path)? {
            return Err(format!("未找到启用位置的受管状态: {target}"));
        }
        if !refresh_managed_installation(db, target_path, None)? {
            return Err(format!("未找到启用位置的受管记录: {target}"));
        }
    }
    Ok(())
}

fn deployment_targets_for_library(
    db: &Connection,
    library_path: &Path,
) -> Result<Vec<String>, String> {
    let query = if cfg!(windows) {
        "SELECT target_path FROM skill_deployments
         WHERE replace(library_path, '/', '\\') = replace(?, '/', '\\') COLLATE NOCASE"
    } else {
        "SELECT target_path FROM skill_deployments WHERE library_path = ?"
    };
    let mut statement = db.prepare(query).map_err(|error| error.to_string())?;
    let targets = statement
        .query_map([library_path.to_string_lossy().as_ref()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(targets)
}

fn table_exists(db: &Connection, table: &str) -> Result<bool, String> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
        [table],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|error| error.to_string())
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn deployment_lookup_returns_every_windows_path_spelling() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE skill_deployments (
                target_path TEXT PRIMARY KEY, skill_id TEXT NOT NULL, library_path TEXT NOT NULL,
                assistant TEXT NOT NULL, scope TEXT NOT NULL, project_path TEXT,
                deploy_mode TEXT NOT NULL, deployed_at TEXT NOT NULL
             );
             INSERT INTO skill_deployments VALUES
                ('first', 'skill-1', 'C:\\SkillMate\\writer', 'Codex', 'global', NULL,
                 'symlink', 'now'),
                ('second', 'skill-1', 'c:/skillmate/writer', 'Claude Code', 'global', NULL,
                 'symlink', 'now');",
        )
        .unwrap();

        let mut targets =
            deployment_targets_for_library(&db, Path::new("C:/SKILLMATE/writer")).unwrap();
        targets.sort();

        assert_eq!(targets, vec!["first", "second"]);
    }

    #[test]
    fn refresh_uses_each_tables_stored_path_key() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE library_skills (
                id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, library_path TEXT NOT NULL UNIQUE,
                source TEXT NOT NULL, source_kind TEXT NOT NULL, resolved_ref TEXT,
                content_hash TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );
             CREATE TABLE skill_origin_meta (
                skill_path TEXT PRIMARY KEY, origin_kind TEXT NOT NULL, origin_locator TEXT NOT NULL,
                resolved_locator TEXT NOT NULL, tracking_ref TEXT NOT NULL, installed_ref TEXT NOT NULL,
                latest_ref TEXT NOT NULL, sync_state TEXT NOT NULL, sync_message TEXT NOT NULL,
                lag_count INTEGER NOT NULL, last_probe_at INTEGER, last_sync_at INTEGER,
                managed_by_app INTEGER NOT NULL
             );
             CREATE TABLE skill_deployments (
                target_path TEXT PRIMARY KEY, skill_id TEXT NOT NULL, library_path TEXT NOT NULL,
                assistant TEXT NOT NULL, scope TEXT NOT NULL, project_path TEXT,
                deploy_mode TEXT NOT NULL, deployed_at TEXT NOT NULL
             );",
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "skillmate-windows-origin-key-{}",
            crate::app_core::generate_id()
        ));
        let library_path = root.join("writer");
        fs::create_dir_all(&library_path).unwrap();
        fs::write(library_path.join("SKILL.md"), "writer").unwrap();
        let library_key = library_path.to_string_lossy().into_owned();
        let origin_key = library_key.replace('\\', "/").to_ascii_uppercase();
        db.execute(
            "INSERT INTO library_skills VALUES (
                'skill-1', 'writer', ?, 'source', 'git', NULL, 'old', 'now', 'now'
             )",
            [&library_key],
        )
        .unwrap();
        db.execute(
            "INSERT INTO skill_origin_meta VALUES (
                ?, 'git', 'source', 'source', 'main', 'new-ref', '', 'current', '',
                0, NULL, NULL, 1
             )",
            [&origin_key],
        )
        .unwrap();

        refresh_deployment_origins(&db, &library_path).unwrap();

        let resolved_ref: String = db
            .query_row(
                "SELECT resolved_ref FROM library_skills WHERE id = 'skill-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(resolved_ref, "new-ref");
        let _ = fs::remove_dir_all(root);
    }
}
