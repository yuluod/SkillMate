use crate::managed_installation::{
    find_managed_installation, is_explicitly_managed, refresh_managed_installation,
    verify_managed_content_unchanged,
};
use crate::managed_state::{content_fingerprint, refresh_managed_skill_fingerprint};
use crate::operation_coordinator::is_known_skill_path;
use crate::operation_plan::{operation_plan_token, verify_operation_plan};
use crate::skill_reconcile::ReconcileTransaction;
use crate::skill_structure::validate_skill_structure;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const SKIPPED_SYNC_ENTRIES: &[&str] = &[".git", ".hg", ".svn", ".skillmate-state.json"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriftSyncAction {
    pub target_path: String,
    pub assistant: String,
    pub before_hash: String,
    pub after_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriftSyncConflict {
    pub target_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriftSyncPreview {
    pub can_apply: bool,
    pub source_path: String,
    pub source_hash: String,
    pub actions: Vec<DriftSyncAction>,
    pub conflicts: Vec<DriftSyncConflict>,
    pub plan_token: String,
}

pub fn preview_sync_skill_copies(
    db: &Connection,
    source_path: &Path,
    target_paths: &[PathBuf],
) -> Result<DriftSyncPreview, String> {
    let source = source_path.to_path_buf();
    if !is_known_skill_path(db, &source)? {
        return Err("基准 Skill 不在当前盘点范围内".to_string());
    }
    let source_metadata = fs::symlink_metadata(&source).map_err(|error| error.to_string())?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err("基准 Skill 必须是实体目录".to_string());
    }
    let validation = validate_skill_structure(&source);
    if validation.structure_status != "complete" {
        return Err("基准 Skill 不符合 Agent Skills 规范，不能用于同步".to_string());
    }
    if validation
        .warnings
        .iter()
        .any(|warning| warning == "contains_symlinks" || warning == "safety_scan_incomplete")
    {
        return Err("基准 Skill 包含软连接或安全扫描不完整，不能用于同步".to_string());
    }

    let source_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "基准 Skill 目录名无效".to_string())?;
    let source_hash = content_fingerprint(&source)?;
    let mut seen = HashSet::new();
    let mut actions = Vec::new();
    let mut conflicts = Vec::new();

    for target in target_paths {
        if !seen.insert(target.clone()) || target == &source {
            continue;
        }
        let conflict = |message: String| DriftSyncConflict {
            target_path: target.to_string_lossy().to_string(),
            message,
        };
        if target.file_name().and_then(|value| value.to_str()) != Some(source_name) {
            conflicts.push(conflict("目标目录名与基准 Skill 不一致".to_string()));
            continue;
        }
        if !is_known_skill_path(db, target)? {
            conflicts.push(conflict("目标不在当前盘点范围内".to_string()));
            continue;
        }
        if !is_explicitly_managed(db, target)? {
            conflicts.push(conflict(
                "目标不是 SkillMate 受管内容，已保护手工目录".to_string(),
            ));
            continue;
        }
        if fs::symlink_metadata(target)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            conflicts.push(conflict("目标是项目软连接，不能替换为副本".to_string()));
            continue;
        }
        if let Err(error) = verify_managed_content_unchanged(db, target) {
            conflicts.push(conflict(error));
            continue;
        }
        let before_hash = content_fingerprint(target)?;
        if before_hash == source_hash {
            continue;
        }
        let assistant = find_managed_installation(db, target)?
            .map(|installation| installation.skill.assistant)
            .unwrap_or_else(|| "未知助手".to_string());
        actions.push(DriftSyncAction {
            target_path: target.to_string_lossy().to_string(),
            assistant,
            before_hash,
            after_hash: source_hash.clone(),
        });
    }

    let mut preview = DriftSyncPreview {
        can_apply: conflicts.is_empty() && !actions.is_empty(),
        source_path: source.to_string_lossy().to_string(),
        source_hash,
        actions,
        conflicts,
        plan_token: String::new(),
    };
    preview.plan_token = operation_plan_token("sync-skill-copies", &preview)?;
    Ok(preview)
}

pub fn apply_sync_skill_copies(
    db: &Connection,
    source_path: &Path,
    target_paths: &[PathBuf],
    plan_token: Option<&str>,
) -> Result<String, String> {
    let preview = preview_sync_skill_copies(db, source_path, target_paths)?;
    verify_operation_plan(&preview.plan_token, plan_token)?;
    if !preview.can_apply {
        return Err(
            if preview.actions.is_empty() && preview.conflicts.is_empty() {
                "所有副本已经一致".to_string()
            } else {
                format!("同步计划存在 {} 个冲突", preview.conflicts.len())
            },
        );
    }
    let targets = preview
        .actions
        .iter()
        .map(|action| PathBuf::from(&action.target_path))
        .collect::<Vec<_>>();
    let mut transaction = ReconcileTransaction::prepare_managed(db, &targets, &targets)?;
    for target in &targets {
        if let Err(error) = copy_skill_tree(source_path, target) {
            return rollback_error(&mut transaction, error);
        }
        if let Some(root) = target.parent() {
            if let Err(error) = refresh_managed_skill_fingerprint(root, target) {
                return rollback_error(&mut transaction, error);
            }
        }
        if let Err(error) = refresh_managed_installation(db, target, None) {
            return rollback_error(&mut transaction, error);
        }
    }
    match transaction.commit() {
        Ok(None) => Ok(format!("已同步 {} 个受管副本", targets.len())),
        Ok(Some(warning)) => Ok(format!("已同步 {} 个受管副本；{}", targets.len(), warning)),
        Err(error) => Err(error),
    }
}

fn rollback_error(
    transaction: &mut ReconcileTransaction<'_>,
    error: String,
) -> Result<String, String> {
    match transaction.rollback() {
        Ok(()) => Err(format!("同步失败: {error}；已恢复原内容")),
        Err(rollback_error) => Err(format!("同步失败: {error}；回滚不完整: {rollback_error}")),
    }
}

fn copy_skill_tree(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if SKIPPED_SYNC_ENTRIES.contains(&name_text.as_ref()) {
            continue;
        }
        let source_path = entry.path();
        let target_path = target.join(&name);
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "同步来源包含软连接: {}",
                source_path.to_string_lossy()
            ));
        }
        if metadata.is_dir() {
            copy_skill_tree(&source_path, &target_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &target_path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_installation::{find_managed_installation, record_managed_root};
    use crate::managed_state::mark_managed_skill;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "skillmate-drift-{name}-{}-{}",
            std::process::id(),
            crate::app_core::generate_id()
        ))
    }

    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE managed_installations (
                skill_path TEXT PRIMARY KEY, assistant TEXT NOT NULL, source TEXT NOT NULL,
                source_kind TEXT NOT NULL, target_name TEXT NOT NULL, scope TEXT NOT NULL,
                install_mode TEXT NOT NULL, project_path TEXT, tracking_ref TEXT, subdir TEXT,
                resolved_ref TEXT, content_hash TEXT, installed_at TEXT NOT NULL
            );
            CREATE TABLE skill_origin_meta (
                skill_path TEXT PRIMARY KEY, origin_kind TEXT NOT NULL, origin_locator TEXT NOT NULL,
                resolved_locator TEXT NOT NULL, tracking_ref TEXT NOT NULL, installed_ref TEXT NOT NULL,
                latest_ref TEXT NOT NULL, sync_state TEXT NOT NULL, sync_message TEXT NOT NULL,
                lag_count INTEGER NOT NULL, last_probe_at INTEGER, last_sync_at INTEGER,
                managed_by_app INTEGER NOT NULL
            );
            CREATE TABLE skill_tags (
                skill_path TEXT PRIMARY KEY, tags TEXT, tags_json TEXT NOT NULL DEFAULT '[]'
            );
            CREATE TABLE managed_roots (
                root_path TEXT PRIMARY KEY, scope TEXT NOT NULL,
                project_path TEXT, updated_at TEXT NOT NULL
            );",
        )
        .unwrap();
        db
    }

    fn write_skill(path: &Path, description: &str) {
        fs::create_dir_all(path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: writer\ndescription: {description}\n---\n"),
        )
        .unwrap();
    }

    #[test]
    fn copies_complete_skill_tree_and_skips_vcs_metadata() {
        let root = temp_dir("copy");
        let source = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(source.join("references")).unwrap();
        fs::create_dir_all(source.join(".git")).unwrap();
        fs::write(source.join("SKILL.md"), "skill").unwrap();
        fs::write(source.join("references/note.md"), "note").unwrap();
        fs::write(source.join(".git/config"), "private").unwrap();

        copy_skill_tree(&source, &target).unwrap();

        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "skill"
        );
        assert_eq!(
            fs::read_to_string(target.join("references/note.md")).unwrap(),
            "note"
        );
        assert!(!target.join(".git").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preview_and_apply_sync_only_replace_unchanged_managed_target() {
        let db = database();
        let root = temp_dir("apply");
        let source_root = root.join("source-root");
        let target_root = root.join("target-root");
        let source = source_root.join("writer");
        let target = target_root.join("writer");
        write_skill(&source, "source version");
        write_skill(&target, "target version");
        mark_managed_skill(&source_root, "Codex", &source, "local:/tmp/source").unwrap();
        mark_managed_skill(&target_root, "Cursor", &target, "local:/tmp/target").unwrap();
        record_managed_root(&db, &source_root, "project", Some("/tmp/source-project")).unwrap();
        record_managed_root(&db, &target_root, "project", Some("/tmp/target-project")).unwrap();

        let preview =
            preview_sync_skill_copies(&db, &source, std::slice::from_ref(&target)).unwrap();
        assert!(preview.can_apply);
        assert_eq!(preview.actions.len(), 1);
        apply_sync_skill_copies(
            &db,
            &source,
            std::slice::from_ref(&target),
            Some(&preview.plan_token),
        )
        .unwrap();

        assert_eq!(
            content_fingerprint(&source).unwrap(),
            content_fingerprint(&target).unwrap()
        );
        let installation = find_managed_installation(&db, &target).unwrap().unwrap();
        assert_eq!(
            installation.skill.content_hash.as_deref(),
            Some(content_fingerprint(&target).unwrap().as_str())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preview_blocks_target_after_local_content_change() {
        let db = database();
        let root = temp_dir("local-change");
        let source_root = root.join("source-root");
        let target_root = root.join("target-root");
        let source = source_root.join("writer");
        let target = target_root.join("writer");
        write_skill(&source, "source version");
        write_skill(&target, "target version");
        mark_managed_skill(&source_root, "Codex", &source, "local:/tmp/source").unwrap();
        mark_managed_skill(&target_root, "Cursor", &target, "local:/tmp/target").unwrap();
        record_managed_root(&db, &source_root, "project", Some("/tmp/source-project")).unwrap();
        record_managed_root(&db, &target_root, "project", Some("/tmp/target-project")).unwrap();
        fs::write(target.join("LOCAL.md"), "keep me").unwrap();

        let preview =
            preview_sync_skill_copies(&db, &source, std::slice::from_ref(&target)).unwrap();
        assert!(!preview.can_apply);
        assert!(preview.actions.is_empty());
        assert_eq!(preview.conflicts.len(), 1);
        assert!(preview.conflicts[0].message.contains("偏离"));
        assert_eq!(
            fs::read_to_string(target.join("LOCAL.md")).unwrap(),
            "keep me"
        );
        let _ = fs::remove_dir_all(root);
    }
}
