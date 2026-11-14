#[cfg(any(windows, test))]
use rusqlite::OptionalExtension;
use rusqlite::{params, Connection, OpenFlags};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

const DB_SCHEMA_VERSION: i64 = 8;
static DATABASE_INITIALIZATION_ERROR: OnceLock<String> = OnceLock::new();

#[derive(Clone, Copy)]
pub enum PathColumn {
    SkillTags,
    SkillOrigin,
    ManagedInstallation,
    ManagedRoot,
    LibrarySkill,
    DeploymentTarget,
    SkillIgnores,
}

impl PathColumn {
    #[cfg(any(windows, test))]
    fn identifiers(self) -> (&'static str, &'static str) {
        match self {
            Self::SkillTags => ("skill_tags", "skill_path"),
            Self::SkillOrigin => ("skill_origin_meta", "skill_path"),
            Self::ManagedInstallation => ("managed_installations", "skill_path"),
            Self::ManagedRoot => ("managed_roots", "root_path"),
            Self::LibrarySkill => ("library_skills", "library_path"),
            Self::DeploymentTarget => ("skill_deployments", "target_path"),
            Self::SkillIgnores => ("skill_ignores", "skill_path"),
        }
    }
}

pub fn database_path_key(
    connection: &Connection,
    column: PathColumn,
    path: &Path,
) -> Result<String, String> {
    let candidate = path.to_string_lossy().into_owned();
    #[cfg(not(windows))]
    {
        let _ = (connection, column);
        Ok(candidate)
    }
    #[cfg(windows)]
    {
        find_windows_path_key(connection, column, &candidate)
            .map(|stored| stored.unwrap_or_else(|| candidate.replace('/', "\\")))
    }
}

#[cfg(any(windows, test))]
fn find_windows_path_key(
    connection: &Connection,
    column: PathColumn,
    candidate: &str,
) -> Result<Option<String>, String> {
    let (table, column) = column.identifiers();
    let sql = format!(
        "SELECT {column} FROM {table}
         WHERE replace({column}, '/', '\\') = replace(?, '/', '\\') COLLATE NOCASE
         LIMIT 1"
    );
    connection
        .query_row(&sql, [candidate], |row| row.get(0))
        .optional()
        .map_err(|error| error.to_string())
}

pub(crate) fn data_directory_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("skillmate")
}

pub(crate) fn data_directory() -> Result<PathBuf, String> {
    let directory = data_directory_path();
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory)
}

fn database_path() -> Result<PathBuf, String> {
    Ok(data_directory()?.join("data.db"))
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
    prepare_connection(Connection::open(database_path()?).map_err(|error| error.to_string())?)
}

fn prepare_connection(connection: Connection) -> Result<Connection, String> {
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

/// 只读连接：不创建数据目录、不做 schema 迁移、不改 journal 模式。
/// 库文件已存在时按原样打开；尚不存在时返回带完整 schema 的内存连接，
/// 让 doctor 这类诊断入口按「无登记」口径读取而不在磁盘留下任何文件。
pub fn open_readonly_db_connection() -> Result<Connection, String> {
    open_readonly_db_connection_at(&data_directory_path().join("data.db"))
}

pub(crate) fn open_readonly_db_connection_at(path: &Path) -> Result<Connection, String> {
    if !path.is_file() {
        let connection =
            prepare_connection(Connection::open_in_memory().map_err(|error| error.to_string())?)?;
        migrate_database(&connection)?;
        return Ok(connection);
    }
    // SQLITE_OPEN_READ_ONLY：不触发 SQLite 自身的日志恢复与 WAL checkpoint 写操作；
    // WAL 模式下 -shm 缺失时由 SQLite 在堆内重建 wal-index，已提交数据仍可读。
    let connection = prepare_connection(
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| error.to_string())?,
    )?;
    // 需要写入恢复的库（遗留热日志等）只读连接无法自行恢复，
    // 探针读把该状态报成明确的诊断错误，而不是悄悄回滚改写。
    connection
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| {
            format!(
                "数据库需要写入恢复（可能存在遗留日志或损坏），\
                 请先运行 skillmate-cli scan 或打开应用完成恢复后重试: {error}"
            )
        })?;
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
             CREATE TABLE IF NOT EXISTS library_skills (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL UNIQUE,
                 library_path TEXT NOT NULL UNIQUE,
                 source TEXT NOT NULL,
                 source_kind TEXT NOT NULL,
                 resolved_ref TEXT,
                 content_hash TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS skill_deployments (
                 target_path TEXT PRIMARY KEY,
                 skill_id TEXT NOT NULL,
                 library_path TEXT NOT NULL,
                 assistant TEXT NOT NULL,
                 scope TEXT NOT NULL,
                 project_path TEXT,
                 deploy_mode TEXT NOT NULL,
                 deployed_at TEXT NOT NULL,
                 FOREIGN KEY(skill_id) REFERENCES library_skills(id) ON DELETE RESTRICT
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
    crate::finding_review::ensure_review_table(&transaction)?;
    crate::skill_ignore::ensure_ignore_table(&transaction)?;

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
    fn windows_path_lookup_reuses_stored_case_and_separators() {
        let connection = Connection::open_in_memory().unwrap();
        migrate_database(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO library_skills (
                    id, name, library_path, source, source_kind, content_hash,
                    created_at, updated_at
                 ) VALUES ('1', 'writer', 'C:\\Users\\Alice\\SkillMate\\writer', '', 'local',
                    'hash', 'now', 'now')",
                [],
            )
            .unwrap();

        let stored = find_windows_path_key(
            &connection,
            PathColumn::LibrarySkill,
            "c:/users/alice/skillmate/writer",
        )
        .unwrap();

        assert_eq!(
            stored.as_deref(),
            Some("C:\\Users\\Alice\\SkillMate\\writer")
        );
    }

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

    #[test]
    fn readonly_open_refuses_database_with_hot_journal() {
        if let Some(db) = std::env::var_os("SKILLMATE_READONLY_HOT_JOURNAL_DB").map(PathBuf::from) {
            let connection = Connection::open(&db).unwrap();
            connection
                .pragma_update(None, "journal_mode", "DELETE")
                .unwrap();
            connection
                .execute("CREATE TABLE IF NOT EXISTS t (x)", [])
                .unwrap();
            // 极小页缓存 + 批量写入迫使 journal 在提交前 spill，留下有效热头
            connection.pragma_update(None, "cache_size", 10).unwrap();
            connection.execute_batch("BEGIN IMMEDIATE").unwrap();
            connection
                .execute(
                    "INSERT INTO t WITH RECURSIVE c(x) AS \
                     (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 20000) SELECT x FROM c",
                    [],
                )
                .unwrap();
            // 不提交直接退出：hot journal 等待写入恢复
            std::process::exit(0);
        }
        let root = std::env::temp_dir().join(format!(
            "skillmate-readonly-hot-journal-{}-{}",
            std::process::id(),
            crate::app_core::generate_id()
        ));
        fs::create_dir_all(&root).unwrap();
        let db_file = root.join("data.db");
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "database::tests::readonly_open_refuses_database_with_hot_journal",
            ])
            .env("SKILLMATE_READONLY_HOT_JOURNAL_DB", &db_file)
            .output()
            .unwrap();
        assert!(child.status.success());
        let journal = root.join("data.db-journal");
        assert!(journal.is_file(), "子进程应遗留 hot journal");
        let db_bytes = fs::read(&db_file).unwrap();
        let journal_bytes = fs::read(&journal).unwrap();

        let Err(error) = open_readonly_db_connection_at(&db_file) else {
            panic!("带遗留热日志的数据库应拒绝只读打开");
        };
        assert!(error.contains("恢复"), "错误应提示先完成恢复: {error}");

        // 只读入口没有替 SQLite 做恢复：主库与 journal 原样保留。
        assert_eq!(fs::read(&db_file).unwrap(), db_bytes);
        assert_eq!(fs::read(&journal).unwrap(), journal_bytes);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn readonly_open_reads_committed_wal_data_without_checkpoint() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-readonly-wal-{}-{}",
            std::process::id(),
            crate::app_core::generate_id()
        ));
        fs::create_dir_all(&root).unwrap();
        let db_file = root.join("data.db");
        // writer 保持打开：-wal/-shm 存在且未 checkpoint，模拟崩溃后的现场。
        let writer = Connection::open(&db_file).unwrap();
        writer.pragma_update(None, "journal_mode", "WAL").unwrap();
        writer.execute("CREATE TABLE t (x TEXT)", []).unwrap();
        writer
            .execute("INSERT INTO t VALUES ('committed')", [])
            .unwrap();

        let reader = open_readonly_db_connection_at(&db_file).unwrap();
        assert_eq!(
            reader
                .query_row("SELECT x FROM t", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "committed"
        );
        let db_bytes = fs::read(&db_file).unwrap();
        drop(reader);
        // 只读连接关闭不触发 checkpoint，主库字节不变。
        assert_eq!(fs::read(&db_file).unwrap(), db_bytes);
        drop(writer);
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn readonly_open_recovers_wal_index_when_shm_missing() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-readonly-wal-noshm-{}-{}",
            std::process::id(),
            crate::app_core::generate_id()
        ));
        fs::create_dir_all(&root).unwrap();
        let db_file = root.join("data.db");
        let writer = Connection::open(&db_file).unwrap();
        writer.pragma_update(None, "journal_mode", "WAL").unwrap();
        writer.execute("CREATE TABLE t (x TEXT)", []).unwrap();
        writer
            .execute("INSERT INTO t VALUES ('committed')", [])
            .unwrap();
        // -shm 缺失（崩溃后被清理）：SQLite 3.22+ 在堆内重建 wal-index。
        fs::remove_file(root.join("data.db-shm")).unwrap();

        let reader = open_readonly_db_connection_at(&db_file).unwrap();
        assert_eq!(
            reader
                .query_row("SELECT x FROM t", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "committed"
        );

        drop(reader);
        drop(writer);
        let _ = fs::remove_dir_all(&root);
    }
}
