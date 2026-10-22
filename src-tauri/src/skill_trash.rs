use crate::app_core::{self, generate_id, managed_skill_roots, now_ms};
use crate::managed_installation::{
    cleanup_skill_metadata, find_managed_installation, is_explicitly_managed, list_managed_roots,
    verify_managed_content_unchanged, ManagedMetadataCheckpoint,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const TRASH_DIRECTORY: &str = ".skillmate-trash";
const TRASH_OWNER_FILE: &str = ".owner";
const TRASH_OWNER_VALUE: &str = "SkillMate managed trash v1\n";
const TRASH_ENTRY_FILE: &str = ".skillmate-trash-entry.json";
const UNDO_WINDOW_MS: i64 = 120_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashReceipt {
    pub token: String,
    pub name: String,
    pub original_path: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
        let created_at = now_ms();
        let entry = TrashEntry {
            original: path.to_path_buf(),
            stash,
            token_root: token_root.clone(),
            checkpoint,
            created_at,
        };
        // 先原子落盘撤销记录，再移动文件：记录缺失的暂存目录视为
        // 「移入未完成」，启动清理时只清目录本身，Skill 原位内容不受影响。
        if let Err(error) = persist_entry(&token_root.join(TRASH_ENTRY_FILE), &entry) {
            let _ = fs::remove_dir_all(&token_root);
            return Err(format!("写入撤销记录失败: {error}"));
        }
        fs::rename(path, &entry.stash).map_err(|error| {
            let _ = fs::remove_dir_all(&token_root);
            format!("无法移入可撤销暂存区: {error}")
        })?;
        if let Err(error) = cleanup_skill_metadata(db, path) {
            restore_after_failure(
                db,
                path,
                &entry.stash,
                &entry.token_root,
                entry.checkpoint.clone(),
            )?;
            return Err(error);
        }
        self.entries.insert(token.clone(), entry);
        Ok(TrashReceipt {
            token,
            name,
            original_path: path.to_string_lossy().to_string(),
            expires_at: created_at + UNDO_WINDOW_MS,
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

/// 启动时扫描各受管根目录下的撤销暂存区：撤销窗口内的记录重新加载进内存，
/// 过期或损坏的记录就地清理。进程崩溃不再把未过期的撤销机会变成永久删除。
pub fn restore_or_purge_abandoned_trash(db: &Connection) -> Result<(SkillTrash, usize), String> {
    let mut trash = SkillTrash::default();
    let mut removed = 0;
    let mut roots = managed_skill_roots();
    roots.extend(list_managed_roots(db)?.into_iter().map(|root| root.path));
    roots.sort();
    roots.dedup();
    for root in roots {
        let trash_root = root.join(TRASH_DIRECTORY);
        if !trash_root.is_dir() || !is_owned_trash_root(&trash_root) {
            continue;
        }
        let entries = fs::read_dir(&trash_root).map_err(|error| {
            format!(
                "读取撤销暂存区 {} 失败: {error}",
                trash_root.to_string_lossy()
            )
        })?;
        for entry in entries {
            let token_root = entry.map_err(|error| error.to_string())?.path();
            if !token_root.is_dir() {
                continue;
            }
            let token = token_root
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            let persisted = fs::read_to_string(token_root.join(TRASH_ENTRY_FILE))
                .ok()
                .and_then(|text| serde_json::from_str::<TrashEntry>(&text).ok());
            match persisted {
                // stash 缺失说明移入未完成（或暂存内容已被外部清走），
                // Skill 原位未动，只清理暂存目录本身。
                Some(persisted)
                    if !token.is_empty()
                        && !is_expired(&persisted)
                        && fs::symlink_metadata(&persisted.stash).is_ok() =>
                {
                    trash.entries.insert(token, persisted);
                }
                _ => {
                    fs::remove_dir_all(&token_root).map_err(|error| {
                        format!(
                            "清理过期撤销暂存区 {} 失败: {error}",
                            token_root.to_string_lossy()
                        )
                    })?;
                    removed += 1;
                }
            }
        }
        cleanup_trash_root_if_empty(&trash_root);
    }
    Ok((trash, removed))
}

fn is_expired(entry: &TrashEntry) -> bool {
    now_ms() - entry.created_at > UNDO_WINDOW_MS
}

fn persist_entry(entry_file: &Path, entry: &TrashEntry) -> Result<(), String> {
    let text = serde_json::to_string_pretty(entry).map_err(|error| error.to_string())?;
    // 撤销记录是崩溃恢复的凭证，本身必须原子落盘，撕裂写会让启动
    // 清理误删暂存区里的 Skill 正本。
    app_core::atomic_write(entry_file, text.as_bytes())
}

fn restore_after_failure(
    db: &Connection,
    path: &Path,
    stash: &Path,
    token_root: &Path,
    checkpoint: ManagedMetadataCheckpoint,
) -> Result<(), String> {
    let restore_file = fs::rename(stash, path)
        .map_err(|restore_error| format!("恢复 Skill 文件失败: {restore_error}"));
    let restore_metadata = checkpoint.restore(db);
    let _ = fs::remove_dir_all(token_root);
    restore_file?;
    restore_metadata?;
    Ok(())
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
    if let Some(trash_root) = entry.token_root.parent() {
        cleanup_trash_root_if_empty(trash_root);
    }
    Ok(())
}

fn cleanup_token_root(token_root: &Path) {
    let _ = fs::remove_dir_all(token_root);
    if let Some(trash_root) = token_root.parent() {
        cleanup_trash_root_if_empty(trash_root);
    }
}

fn cleanup_trash_root_if_empty(trash_root: &Path) {
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
    fn abandoned_trash_survives_restart_and_restores_within_window() {
        let (db, root, skill) = managed_skill("restart");
        let mut trash = SkillTrash::default();
        let receipt = trash.trash(&db, &skill).unwrap();
        // 模拟进程崩溃：丢弃内存状态后从磁盘恢复撤销记录。
        let (mut restored, removed) = restore_or_purge_abandoned_trash(&db).unwrap();
        assert_eq!(removed, 0);
        assert!(root.join(TRASH_DIRECTORY).exists());

        restored.restore(&db, &receipt.token).unwrap();
        assert_eq!(
            fs::read_to_string(skill.join("SKILL.md")).unwrap(),
            "writer"
        );
        assert!(is_explicitly_managed(&db, &skill).unwrap());
        assert!(!root.join(TRASH_DIRECTORY).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn startup_cleanup_removes_only_expired_or_unowned_trash() {
        let (db, root, skill) = managed_skill("expired");
        let mut trash = SkillTrash::default();
        let receipt = trash.trash(&db, &skill).unwrap();
        let token_root = root.join(TRASH_DIRECTORY).join(&receipt.token);
        let entry_file = token_root.join(TRASH_ENTRY_FILE);
        // 把撤销记录改写成已过期，再验证启动清理只删除过期项。
        let mut entry: TrashEntry =
            serde_json::from_str(&fs::read_to_string(&entry_file).unwrap()).unwrap();
        entry.created_at = now_ms() - UNDO_WINDOW_MS - 1_000;
        fs::write(&entry_file, serde_json::to_string(&entry).unwrap()).unwrap();

        let (restored, removed) = restore_or_purge_abandoned_trash(&db).unwrap();
        assert_eq!(removed, 1);
        assert!(restored.entries.is_empty());
        assert!(!root.join(TRASH_DIRECTORY).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn startup_purges_incomplete_trash_without_touching_original() {
        let (db, root, skill) = managed_skill("incomplete");
        // 模拟「撤销记录已落盘、Skill 尚未移入」的崩溃窗口。
        let trash_root = root.join(TRASH_DIRECTORY);
        initialize_trash_root(&trash_root).unwrap();
        let token_root = trash_root.join("partial-token");
        fs::create_dir(&token_root).unwrap();
        let entry = TrashEntry {
            original: skill.clone(),
            stash: token_root.join("writer"),
            token_root: token_root.clone(),
            checkpoint: ManagedMetadataCheckpoint::capture(&db, std::slice::from_ref(&skill))
                .unwrap(),
            created_at: now_ms(),
        };
        persist_entry(&token_root.join(TRASH_ENTRY_FILE), &entry).unwrap();

        let (restored, removed) = restore_or_purge_abandoned_trash(&db).unwrap();

        assert_eq!(removed, 1);
        assert!(restored.entries.is_empty());
        assert!(skill.join("SKILL.md").exists());
        assert!(is_explicitly_managed(&db, &skill).unwrap());
        assert!(!root.join(TRASH_DIRECTORY).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn startup_cleanup_removes_only_owned_abandoned_trash() {
        let (db, root, _skill) = managed_skill("unowned");
        let trash_root = root.join(TRASH_DIRECTORY);
        fs::create_dir_all(trash_root.join("stray")).unwrap();
        fs::write(trash_root.join(TRASH_OWNER_FILE), "not skillmate").unwrap();

        let (restored, removed) = restore_or_purge_abandoned_trash(&db).unwrap();
        assert_eq!(removed, 0);
        assert!(restored.entries.is_empty());
        assert!(trash_root.join("stray").exists());
        let _ = fs::remove_dir_all(root);
    }
}
