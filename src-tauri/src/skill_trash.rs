use crate::app_core::{self, generate_id, managed_skill_roots, now_ms};
use crate::managed_installation::{
    cleanup_skill_metadata, find_managed_installation, is_explicitly_managed, list_managed_roots,
    verify_managed_content_unchanged, ManagedMetadataCheckpoint,
};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const TRASH_DIRECTORY: &str = ".skillmate-trash";
const TRASH_OWNER_FILE: &str = ".owner";
const TRASH_OWNER_VALUE: &str = "SkillMate managed trash v1\n";
const UNDO_WINDOW_MS: i64 = 120_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashReceipt {
    pub token: String,
    pub name: String,
    pub original_path: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
struct TrashEntry {
    original: PathBuf,
    stash: PathBuf,
    token_root: PathBuf,
    checkpoint: ManagedMetadataCheckpoint,
    created_at: i64,
}

#[derive(Default)]
pub struct SkillTrash {
    entries: HashMap<String, TrashEntry>,
}

impl SkillTrash {
    pub fn trash(&mut self, db: &Connection, path: &Path) -> Result<TrashReceipt, String> {
        if !path.exists() && fs::symlink_metadata(path).is_err() {
            return Err("路径不存在".to_string());
        }
        let registry_managed = find_managed_installation(db, path)?;
        if !app_core::is_managed_skill_path(path, &managed_skill_roots())
            && registry_managed.is_none()
        {
            return Err("不允许移除该路径".to_string());
        }
        if !is_explicitly_managed(db, path)? {
            return Err("只允许移除 SkillMate 管理的 Skill".to_string());
        }
        verify_managed_content_unchanged(db, path)?;
        let parent = path
            .parent()
            .ok_or_else(|| "Skill 缺少父目录".to_string())?;
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "Skill 目录名无效".to_string())?
            .to_string();
        let token = generate_id();
        let trash_root = parent.join(TRASH_DIRECTORY);
        initialize_trash_root(&trash_root)?;
        let token_root = trash_root.join(&token);
        fs::create_dir(&token_root).map_err(|error| error.to_string())?;
        let stash = token_root.join(&name);
        let checkpoint = ManagedMetadataCheckpoint::capture(db, &[path.to_path_buf()])?;
        fs::rename(path, &stash).map_err(|error| {
            let _ = fs::remove_dir_all(&token_root);
            format!("无法移入可撤销暂存区: {error}")
        })?;
        if let Err(error) = cleanup_skill_metadata(db, path) {
            let restore_file = fs::rename(&stash, path)
                .map_err(|restore_error| format!("{error}；恢复 Skill 文件失败: {restore_error}"));
            let restore_metadata = checkpoint.restore(db);
            let _ = fs::remove_dir_all(&token_root);
            restore_file?;
            restore_metadata?;
            return Err(error);
        }
        let created_at = now_ms();
        self.entries.insert(
            token.clone(),
            TrashEntry {
                original: path.to_path_buf(),
                stash,
                token_root,
                checkpoint,
                created_at,
            },
        );
        Ok(TrashReceipt {
            token,
            name,
            original_path: path.to_string_lossy().to_string(),
            expires_at: created_at + 60_000,
        })
    }

    pub fn restore(&mut self, db: &Connection, token: &str) -> Result<String, String> {
        let entry = self
            .entries
            .remove(token)
            .ok_or_else(|| "撤销记录不存在或已经过期".to_string())?;
        if now_ms() - entry.created_at > UNDO_WINDOW_MS {
            let _ = remove_entry_files(&entry);
            return Err("撤销时间已过".to_string());
        }
        if entry.original.exists() || fs::symlink_metadata(&entry.original).is_ok() {
            self.entries.insert(token.to_string(), entry);
            return Err("原位置已有新内容，已拒绝覆盖".to_string());
        }
        if let Err(error) = fs::rename(&entry.stash, &entry.original) {
            self.entries.insert(token.to_string(), entry);
            return Err(format!("恢复 Skill 文件失败: {error}"));
        }
        if let Err(error) = entry.checkpoint.restore(db) {
            let _ = fs::rename(&entry.original, &entry.stash);
            self.entries.insert(token.to_string(), entry);
            return Err(format!("恢复 Skill 元数据失败: {error}"));
        }
        cleanup_token_root(&entry.token_root);
        Ok("已恢复 Skill".to_string())
    }

    pub fn purge(&mut self, token: &str) -> Result<bool, String> {
        let Some(entry) = self.entries.remove(token) else {
            return Ok(false);
        };
        remove_entry_files(&entry)?;
        Ok(true)
    }
}

pub fn purge_abandoned_trash(db: &Connection) -> Result<usize, String> {
    let mut roots = managed_skill_roots();
    roots.extend(list_managed_roots(db)?.into_iter().map(|root| root.path));
    roots.sort();
    roots.dedup();
    let mut removed = 0;
    for root in roots {
        let trash_root = root.join(TRASH_DIRECTORY);
        if !trash_root.is_dir() || !is_owned_trash_root(&trash_root) {
            continue;
        }
        fs::remove_dir_all(&trash_root).map_err(|error| {
            format!(
                "清理遗留暂存区 {} 失败: {error}",
                trash_root.to_string_lossy()
            )
        })?;
        removed += 1;
    }
    Ok(removed)
}

fn initialize_trash_root(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| error.to_string())?;
    let owner = path.join(TRASH_OWNER_FILE);
    if owner.exists() {
        if !is_owned_trash_root(path) {
            return Err("暂存目录所有权标记无效".to_string());
        }
    } else {
        fs::write(owner, TRASH_OWNER_VALUE).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn is_owned_trash_root(path: &Path) -> bool {
    fs::read_to_string(path.join(TRASH_OWNER_FILE))
        .map(|value| value == TRASH_OWNER_VALUE)
        .unwrap_or(false)
}

fn remove_entry_files(entry: &TrashEntry) -> Result<(), String> {
    if entry.token_root.exists() {
        fs::remove_dir_all(&entry.token_root).map_err(|error| error.to_string())?;
    }
    cleanup_empty_trash_root(&entry.token_root);
    Ok(())
}

fn cleanup_token_root(token_root: &Path) {
    let _ = fs::remove_dir_all(token_root);
    cleanup_empty_trash_root(token_root);
}

fn cleanup_empty_trash_root(token_root: &Path) {
    let Some(trash_root) = token_root.parent() else {
        return;
    };
    let has_entries = fs::read_dir(trash_root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name() != TRASH_OWNER_FILE);
    if !has_entries && is_owned_trash_root(trash_root) {
        let _ = fs::remove_dir_all(trash_root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_installation::{is_explicitly_managed, record_managed_root};
    use crate::managed_state::mark_managed_skill;

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

    fn managed_skill(name: &str) -> (Connection, PathBuf, PathBuf) {
        let db = database();
        let root = std::env::temp_dir().join(format!(
            "skillmate-trash-{name}-{}-{}",
            std::process::id(),
            generate_id()
        ));
        let skill = root.join("writer");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "writer").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "local:/tmp/writer").unwrap();
        record_managed_root(&db, &root, "project", Some("/tmp/project")).unwrap();
        (db, root, skill)
    }

    #[test]
    fn owned_trash_marker_is_required_for_cleanup() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-trash-marker-{}-{}",
            std::process::id(),
            generate_id()
        ));
        fs::create_dir_all(&root).unwrap();
        assert!(!is_owned_trash_root(&root));
        initialize_trash_root(&root).unwrap();
        assert!(is_owned_trash_root(&root));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn trash_and_restore_preserve_files_and_managed_metadata() {
        let (db, root, skill) = managed_skill("restore");
        let mut trash = SkillTrash::default();

        let receipt = trash.trash(&db, &skill).unwrap();
        assert!(!skill.exists());
        assert!(!is_explicitly_managed(&db, &skill).unwrap());

        trash.restore(&db, &receipt.token).unwrap();
        assert_eq!(
            fs::read_to_string(skill.join("SKILL.md")).unwrap(),
            "writer"
        );
        assert!(is_explicitly_managed(&db, &skill).unwrap());
        assert!(!root.join(TRASH_DIRECTORY).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restore_refuses_to_overwrite_new_content_at_original_path() {
        let (db, root, skill) = managed_skill("conflict");
        let mut trash = SkillTrash::default();
        let receipt = trash.trash(&db, &skill).unwrap();
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("KEEP.md"), "new").unwrap();

        let error = trash.restore(&db, &receipt.token).unwrap_err();
        assert!(error.contains("拒绝覆盖"));
        assert_eq!(fs::read_to_string(skill.join("KEEP.md")).unwrap(), "new");
        trash.purge(&receipt.token).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn startup_cleanup_removes_only_owned_abandoned_trash() {
        let (db, root, skill) = managed_skill("abandoned");
        let mut trash = SkillTrash::default();
        trash.trash(&db, &skill).unwrap();
        assert!(root.join(TRASH_DIRECTORY).exists());

        assert_eq!(purge_abandoned_trash(&db).unwrap(), 1);
        assert!(!root.join(TRASH_DIRECTORY).exists());
        let _ = fs::remove_dir_all(root);
    }
}
