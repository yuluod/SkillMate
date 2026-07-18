use rusqlite::{params, Connection};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

const DB_SCHEMA_VERSION: i64 = 6;
static DATABASE_INITIALIZATION_ERROR: OnceLock<String> = OnceLock::new();

fn database_path() -> Result<PathBuf, String> {
    let directory = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("skillmate");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory.join("data.db"))
}

pub fn create_db_connection() -> Result<Connection, String> {
    let connection = open_db_connection_unchecked()?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| error.to_string())?;
    migrate_database(&connection)?;
    Ok(connection)
}

pub fn open_db_connection() -> Result<Connection, String> {
    if let Some(error) = database_initialization_error() {
        return Err(error.to_string());
    }
    open_db_connection_unchecked()
}

fn open_db_connection_unchecked() -> Result<Connection, String> {
    let connection = Connection::open(database_path()?).map_err(|error| error.to_string())?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

pub fn remember_database_initialization_error(error: &str) -> String {
    DATABASE_INITIALIZATION_ERROR
        .get_or_init(|| database_initialization_message(error))
        .clone()
}

pub fn database_initialization_error() -> Option<&'static str> {
    DATABASE_INITIALIZATION_ERROR.get().map(String::as_str)
}

fn database_initialization_message(error: &str) -> String {
    format!(
        "数据库初始化失败：{}。SkillMate 已在受限模式启动，请修复数据目录后重启应用",
        error
    )
}

fn migrate_database(connection: &Connection) -> Result<(), String> {
    let current_version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|error| error.to_string())?;
    if current_version > DB_SCHEMA_VERSION {
        return Err(format!(
            "数据库版本 {} 高于当前支持版本 {}，请升级 SkillMate 后重试",
            current_version, DB_SCHEMA_VERSION
        ));
    }
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS tags (id TEXT PRIMARY KEY, name TEXT NOT NULL, color TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT, skill_ids TEXT, skill_ids_json TEXT NOT NULL DEFAULT '[]', created_at TEXT);
             CREATE TABLE IF NOT EXISTS skill_tags (skill_path TEXT PRIMARY KEY, tags TEXT, tags_json TEXT NOT NULL DEFAULT '[]');
             CREATE TABLE IF NOT EXISTS git_backup (id INTEGER PRIMARY KEY, enabled INTEGER, remote_url TEXT, repo_path TEXT, branch TEXT, last_sync TEXT);
             CREATE TABLE IF NOT EXISTS skill_origin_meta (
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
             );
             CREATE TABLE IF NOT EXISTS managed_installations (
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
                 installed_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS managed_roots (
                 root_path TEXT PRIMARY KEY,
                 scope TEXT NOT NULL DEFAULT 'global',
                 project_path TEXT,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS install_policy (
                 id INTEGER PRIMARY KEY,
                 mode TEXT NOT NULL DEFAULT 'off',
                 block_risky_content INTEGER NOT NULL DEFAULT 0,
                 trusted_git_hosts_json TEXT NOT NULL DEFAULT '[]',
                 trusted_local_roots_json TEXT NOT NULL DEFAULT '[]'
             );",
        )
        .map_err(|error| error.to_string())?;

    let scenario_json_added = !table_has_column(&transaction, "scenarios", "skill_ids_json")?;
    if scenario_json_added {
        transaction
            .execute(
                "ALTER TABLE scenarios ADD COLUMN skill_ids_json TEXT NOT NULL DEFAULT '[]'",
                [],
            )
            .map_err(|error| error.to_string())?;
    }
    let tags_json_added = !table_has_column(&transaction, "skill_tags", "tags_json")?;
    if tags_json_added {
        transaction
            .execute(
                "ALTER TABLE skill_tags ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]'",
                [],
            )
            .map_err(|error| error.to_string())?;
    }
    for (column, definition) in [
        ("enabled", "INTEGER NOT NULL DEFAULT 0"),
        ("remote_url", "TEXT NOT NULL DEFAULT ''"),
        ("repo_path", "TEXT NOT NULL DEFAULT ''"),
        ("branch", "TEXT NOT NULL DEFAULT 'main'"),
        ("last_sync", "TEXT NOT NULL DEFAULT ''"),
    ] {
        if !table_has_column(&transaction, "git_backup", column)? {
            transaction
                .execute(
                    &format!("ALTER TABLE git_backup ADD COLUMN {column} {definition}"),
                    [],
                )
                .map_err(|error| error.to_string())?;
        }
    }

    if current_version < 2 || scenario_json_added || tags_json_added {
        migrate_legacy_json_columns(&transaction)?;
    }
    seed_default_tags(&transaction)?;
    transaction
        .pragma_update(None, "user_version", DB_SCHEMA_VERSION)
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn seed_default_tags(connection: &Connection) -> Result<(), String> {
    let count: i32 = connection
        .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if count > 0 {
        return Ok(());
    }
    for (id, name, color) in [
        ("1", "前端", "#6366f1"),
        ("2", "后端", "#10b981"),
        ("3", "AI", "#f59e0b"),
    ] {
        connection
            .execute(
                "INSERT INTO tags (id, name, color) VALUES (?, ?, ?)",
                params![id, name, color],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({})", table))
        .map_err(|error| error.to_string())?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?;
    for result in columns {
        if result.map_err(|error| error.to_string())? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_legacy_json_columns(connection: &Connection) -> Result<(), String> {
    let scenarios = {
        let mut statement = connection
            .prepare("SELECT id, COALESCE(skill_ids, '') FROM scenarios")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    for (id, legacy) in scenarios {
        let json = serde_json::to_string(&parse_legacy_list(&legacy))
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "UPDATE scenarios SET skill_ids_json = ? WHERE id = ?",
                params![json, id],
            )
            .map_err(|error| error.to_string())?;
    }

    let tag_mappings = {
        let mut statement = connection
            .prepare("SELECT skill_path, COALESCE(tags, '') FROM skill_tags")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    for (path, legacy) in tag_mappings {
        let json = serde_json::to_string(&parse_legacy_list(&legacy))
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "UPDATE skill_tags SET tags_json = ? WHERE skill_path = ?",
                params![json, path],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn parse_legacy_list(value: &str) -> Vec<String> {
    if value.is_empty() {
        Vec::new()
    } else {
        value.split(',').map(str::to_string).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_preserves_legacy_lists_and_adds_schema_version() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE tags (id TEXT PRIMARY KEY, name TEXT NOT NULL, color TEXT NOT NULL);
                 CREATE TABLE scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT, skill_ids TEXT, created_at TEXT);
                 CREATE TABLE skill_tags (skill_path TEXT PRIMARY KEY, tags TEXT);
                 INSERT INTO scenarios VALUES ('s1', '场景', '', '/tmp/a,/tmp/b', '2026-01-01');
                 INSERT INTO skill_tags VALUES ('/tmp/a', 'one,two');",
            )
            .unwrap();

        migrate_database(&connection).unwrap();
        migrate_database(&connection).unwrap();

        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        let scenario_json: String = connection
            .query_row(
                "SELECT skill_ids_json FROM scenarios WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let tags_json: String = connection
            .query_row(
                "SELECT tags_json FROM skill_tags WHERE skill_path = '/tmp/a'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(version, DB_SCHEMA_VERSION);
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&scenario_json).unwrap(),
            vec!["/tmp/a", "/tmp/b"]
        );
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&tags_json).unwrap(),
            vec!["one", "two"]
        );
    }

    #[test]
    fn migration_rejects_database_from_newer_skillmate() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 999).unwrap();

        let error = migrate_database(&connection).unwrap_err();

        assert!(error.contains("数据库版本"));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 999);
    }

    #[test]
    fn migration_adds_repo_path_to_legacy_git_backup_table() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE git_backup (
                    id INTEGER PRIMARY KEY,
                    enabled INTEGER,
                    remote_url TEXT,
                    branch TEXT,
                    last_sync TEXT
                 );
                 CREATE TABLE scenarios (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    description TEXT,
                    skill_ids TEXT,
                    created_at TEXT
                 );
                 CREATE TABLE skill_tags (skill_path TEXT PRIMARY KEY, tags TEXT);
                 INSERT INTO git_backup VALUES (1, 1, 'https://example.com/backup.git', 'main', '2026-07-13');
                 INSERT INTO scenarios VALUES ('s1', '旧场景', '', '/tmp/a,/tmp/b', '2026-07-13');
                 INSERT INTO skill_tags VALUES ('/tmp/a', 'one,two');
                 PRAGMA user_version = 5;",
            )
            .unwrap();

        migrate_database(&connection).unwrap();

        assert!(table_has_column(&connection, "git_backup", "repo_path").unwrap());
        let backup: (i32, String, String, String, String) = connection
            .query_row(
                "SELECT enabled, remote_url, repo_path, branch, last_sync FROM git_backup WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            backup,
            (
                1,
                "https://example.com/backup.git".to_string(),
                String::new(),
                "main".to_string(),
                "2026-07-13".to_string(),
            )
        );
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_SCHEMA_VERSION);
        let scenario_json: String = connection
            .query_row(
                "SELECT skill_ids_json FROM scenarios WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let tags_json: String = connection
            .query_row(
                "SELECT tags_json FROM skill_tags WHERE skill_path = '/tmp/a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&scenario_json).unwrap(),
            vec!["/tmp/a", "/tmp/b"]
        );
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&tags_json).unwrap(),
            vec!["one", "two"]
        );
    }

    #[test]
    fn initialization_error_explains_limited_mode_and_restart() {
        let message = database_initialization_message("database is locked");

        assert!(message.contains("数据库初始化失败"));
        assert!(message.contains("database is locked"));
        assert!(message.contains("受限模式"));
        assert!(message.contains("重启应用"));
    }
}
