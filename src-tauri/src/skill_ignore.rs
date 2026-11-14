use crate::database::{database_path_key, PathColumn};
use rusqlite::{params, Connection};
use std::path::Path;

pub const TABLE_DDL: &str = "CREATE TABLE IF NOT EXISTS skill_ignores (
    skill_path TEXT PRIMARY KEY,
    ignored_at TEXT NOT NULL
);";

pub fn ensure_ignore_table(db: &Connection) -> Result<(), String> {
    db.execute_batch(TABLE_DDL)
        .map_err(|error| error.to_string())
}

/// 「忽略」按实体路径登记：纯展示层，不删除文件、不改登记、扫描仍完整执行。
/// 只有外部（非受管）条目可以忽略——受管内容的问题应当被暴露而不是隐藏，
/// 统一库主副本与受管启用位置一律拒绝。
pub fn set_skill_ignored(db: &Connection, skill_path: &Path, ignored: bool) -> Result<(), String> {
    if crate::skill_library::is_library_skill_path(skill_path) {
        return Err("统一库内容不支持忽略".to_string());
    }
    if crate::managed_installation::find_managed_installation(db, skill_path)?.is_some() {
        return Err("受管启用位置不支持忽略".to_string());
    }
    ensure_ignore_table(db)?;
    let key = database_path_key(db, PathColumn::SkillIgnores, skill_path)?;
    if ignored {
        db.execute(
            "INSERT INTO skill_ignores (skill_path, ignored_at) VALUES (?, ?)
             ON CONFLICT(skill_path) DO UPDATE SET ignored_at = excluded.ignored_at",
            params![key, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|error| error.to_string())?;
    } else {
        db.execute(
            "DELETE FROM skill_ignores WHERE skill_path = ?",
            params![key],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn list_ignored_skill_paths(db: &Connection) -> Result<Vec<String>, String> {
    ensure_ignore_table(db)?;
    let mut statement = db
        .prepare("SELECT skill_path FROM skill_ignores ORDER BY skill_path")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ignore_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        // find_managed_installation 的守卫查询依赖受管登记表，测试夹具补齐最小 schema。
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS managed_installations (
                skill_path TEXT PRIMARY KEY,
                assistant TEXT NOT NULL,
                source TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                target_name TEXT NOT NULL,
                scope TEXT NOT NULL DEFAULT 'global',
                install_mode TEXT NOT NULL DEFAULT 'copy',
                project_path TEXT,
                tracking_ref TEXT,
                subdir TEXT,
                resolved_ref TEXT,
                content_hash TEXT,
                installed_at TEXT NOT NULL DEFAULT ''
            );",
        )
        .unwrap();
        ensure_ignore_table(&db).unwrap();
        db
    }

    fn temp_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "skillmate-ignore-test-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn ignore_is_recorded_listed_and_reversible() {
        let db = ignore_db();
        let path = temp_path("external");
        set_skill_ignored(&db, &path, true).unwrap();
        let listed = list_ignored_skill_paths(&db).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].contains("external"));

        set_skill_ignored(&db, &path, false).unwrap();
        assert!(list_ignored_skill_paths(&db).unwrap().is_empty());
    }

    #[test]
    fn managed_and_library_paths_are_rejected() {
        let db = ignore_db();
        let root = temp_path("library-root");
        let _guard = crate::skill_library::use_test_library_root(root.clone());
        let library_path = root.join("managed");
        std::fs::create_dir_all(&library_path).unwrap();
        assert!(set_skill_ignored(&db, &library_path, true).is_err());
        assert!(list_ignored_skill_paths(&db).unwrap().is_empty());
    }
}
