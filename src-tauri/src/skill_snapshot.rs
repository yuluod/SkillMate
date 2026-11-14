use rusqlite::{params, Connection};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::database::data_directory_path;

/// 每个 Skill 保留的更新快照上限：回滚只需要「上一版本」，留 2 个
/// 兼顾「回滚后再回滚（前进）」。超出上限的旧快照在每次记录后清理。
pub const KEEP_SNAPSHOTS_PER_SKILL: usize = 2;

pub const TABLE_DDL: &str = "CREATE TABLE IF NOT EXISTS skill_update_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    skill_path TEXT NOT NULL,
    snapshot_dir TEXT NOT NULL,
    snapshot_ref TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL
);";

pub fn ensure_table(db: &Connection) -> Result<(), String> {
    db.execute_batch(TABLE_DDL)
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSnapshot {
    pub snapshot_dir: String,
    #[serde(rename = "ref")]
    pub ref_: String,
    pub content_hash: String,
    pub created_at: i64,
}

#[cfg(test)]
thread_local! {
    static TEST_SNAPSHOT_ROOT: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub struct TestSnapshotRootGuard(std::option::Option<PathBuf>);

#[cfg(test)]
impl Drop for TestSnapshotRootGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        TEST_SNAPSHOT_ROOT.with(|root| *root.borrow_mut() = previous);
    }
}

#[cfg(test)]
pub fn use_test_snapshot_root(path: PathBuf) -> TestSnapshotRootGuard {
    let previous = TEST_SNAPSHOT_ROOT.with(|root| root.replace(Some(path)));
    TestSnapshotRootGuard(previous)
}

fn snapshot_store_root() -> Result<PathBuf, String> {
    #[cfg(test)]
    if let Some(root) = TEST_SNAPSHOT_ROOT.with(|value| value.borrow().clone()) {
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        return Ok(root);
    }
    let root = data_directory_path().join("skill-snapshots");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    Ok(root)
}

/// 计算一次更新的事务提交后旧主副本的落位目录：
/// `<快照库>/<skill 目录名>/<纳秒时间戳>`。时间戳仅用于排序与隔离。
pub fn next_snapshot_dir(skill_path: &Path) -> Result<PathBuf, String> {
    let name = skill_path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "skill".into());
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    Ok(snapshot_store_root()?
        .join(name)
        .join(timestamp.to_string()))
}

pub fn record_snapshot(
    db: &Connection,
    skill_path: &Path,
    snapshot_dir: &Path,
    snapshot_ref: &str,
    content_hash: &str,
) -> Result<(), String> {
    if !snapshot_dir.is_dir() {
        return Err(format!(
            "快照目录未生成: {}",
            snapshot_dir.to_string_lossy()
        ));
    }
    ensure_table(db)?;
    db.execute(
        "INSERT INTO skill_update_snapshots
         (skill_path, snapshot_dir, snapshot_ref, content_hash, created_at)
         VALUES (?, ?, ?, ?, ?)",
        params![
            skill_path.to_string_lossy(),
            snapshot_dir.to_string_lossy(),
            snapshot_ref,
            content_hash,
            chrono::Utc::now().timestamp()
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn list_snapshots(db: &Connection, skill_path: &Path) -> Result<Vec<SkillSnapshot>, String> {
    ensure_table(db)?;
    let mut statement = db
        .prepare(
            "SELECT snapshot_dir, snapshot_ref, content_hash, created_at
             FROM skill_update_snapshots WHERE skill_path = ?
             ORDER BY created_at DESC, id DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([skill_path.to_string_lossy()], |row| {
            Ok(SkillSnapshot {
                snapshot_dir: row.get(0)?,
                ref_: row.get(1)?,
                content_hash: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

/// 删除指定快照（行 + 目录）。回滚把快照内容拷回主副本后调用：
/// 被恢复的版本已是当前内容，快照目录成为冗余拷贝。
pub fn delete_snapshot(db: &Connection, snapshot_dir: &Path) -> Result<(), String> {
    ensure_table(db)?;
    db.execute(
        "DELETE FROM skill_update_snapshots WHERE snapshot_dir = ?",
        [snapshot_dir.to_string_lossy()],
    )
    .map_err(|error| error.to_string())?;
    if snapshot_dir.is_dir() {
        fs::remove_dir_all(snapshot_dir).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// 只保留每个 Skill 最新的 `KEEP_SNAPSHOTS_PER_SKILL` 条：超限的行与目录一起删除。
pub fn prune_snapshots(db: &Connection, skill_path: &Path) -> Result<(), String> {
    let snapshots = list_snapshots(db, skill_path)?;
    for snapshot in snapshots.into_iter().skip(KEEP_SNAPSHOTS_PER_SKILL) {
        if let Err(error) = delete_snapshot(db, Path::new(&snapshot.snapshot_dir)) {
            // 单条清理失败不阻塞整体：下次记录后 prune 会再试
            eprintln!("清理过期更新快照失败: {error}");
        }
    }
    Ok(())
}

/// 启动兜底清理：行存在但目录消失 → 删行；目录存在但无行引用 → 删目录。
/// 保证快照库体积有界，崩溃残留（提交后未及写台账）也能收敛。
pub fn sweep_orphans(db: &Connection) -> Result<(), String> {
    ensure_table(db)?;
    let mut statement = db
        .prepare("SELECT snapshot_dir FROM skill_update_snapshots")
        .map_err(|error| error.to_string())?;
    let recorded = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;

    for dir in &recorded {
        if !Path::new(dir).is_dir() {
            db.execute(
                "DELETE FROM skill_update_snapshots WHERE snapshot_dir = ?",
                [dir],
            )
            .map_err(|error| error.to_string())?;
        }
    }
    let recorded: std::collections::HashSet<String> = recorded.into_iter().collect();

    let store = snapshot_store_root()?;
    let entries = match fs::read_dir(&store) {
        Ok(entries) => entries
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    for skill_dir in entries {
        let path = skill_dir.path();
        if !path.is_dir() {
            continue;
        }
        if let Ok(timestamps) = fs::read_dir(&path) {
            for timestamp in timestamps.flatten() {
                let snapshot_dir = timestamp.path();
                if recorded.contains(&snapshot_dir.to_string_lossy().into_owned()) {
                    continue;
                }
                let _ = fs::remove_dir_all(&snapshot_dir);
            }
        }
        // 技能目录为空时一并移除，保持快照库整洁
        let _ = fs::remove_dir(&path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        ensure_table(&db).unwrap();
        db
    }

    fn temp_skill(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "skillmate-snapshot-test-{}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn record_list_prune_and_sweep_keep_bounded_history() {
        let db = snapshot_db();
        let root = temp_skill("root");
        let _guard = use_test_snapshot_root(root.join("store"));
        let skill_path = root.join("writer");
        fs::create_dir_all(&skill_path).unwrap();

        let mut dirs = Vec::new();
        for index in 0..4 {
            let dir = next_snapshot_dir(&skill_path).unwrap();
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("SKILL.md"), format!("version {index}")).unwrap();
            record_snapshot(&db, &skill_path, &dir, &format!("ref-{index}"), "hash").unwrap();
            dirs.push(dir);
            prune_snapshots(&db, &skill_path).unwrap();
        }

        let listed = list_snapshots(&db, &skill_path).unwrap();
        assert_eq!(listed.len(), KEEP_SNAPSHOTS_PER_SKILL);
        // 只保留最新的两个
        assert_eq!(listed[0].ref_, "ref-3");
        assert_eq!(listed[1].ref_, "ref-2");
        assert!(!dirs[0].exists() && !dirs[1].exists());
        assert!(dirs[2].exists() && dirs[3].exists());

        // 无行引用的目录由启动清理收敛
        let orphan = next_snapshot_dir(&skill_path).unwrap();
        fs::create_dir_all(&orphan).unwrap();
        sweep_orphans(&db).unwrap();
        assert!(!orphan.exists());
        assert_eq!(list_snapshots(&db, &skill_path).unwrap().len(), 2);
    }

    #[test]
    fn missing_snapshot_dir_is_reported_and_stale_rows_are_swept() {
        let db = snapshot_db();
        let root = temp_skill("missing");
        let _guard = use_test_snapshot_root(root.join("store"));
        let skill_path = root.join("writer");
        let dir = next_snapshot_dir(&skill_path).unwrap();
        assert!(record_snapshot(&db, &skill_path, &dir, "ref", "hash").is_err());

        fs::create_dir_all(&dir).unwrap();
        record_snapshot(&db, &skill_path, &dir, "ref", "hash").unwrap();
        fs::remove_dir_all(&dir).unwrap();
        sweep_orphans(&db).unwrap();
        assert!(list_snapshots(&db, &skill_path).unwrap().is_empty());
    }

    #[test]
    fn delete_snapshot_removes_row_and_directory() {
        let db = snapshot_db();
        let root = temp_skill("delete");
        let _guard = use_test_snapshot_root(root.join("store"));
        let skill_path = root.join("writer");
        let dir = next_snapshot_dir(&skill_path).unwrap();
        fs::create_dir_all(&dir).unwrap();
        record_snapshot(&db, &skill_path, &dir, "ref", "hash").unwrap();
        delete_snapshot(&db, &dir).unwrap();
        assert!(!dir.exists());
        assert!(list_snapshots(&db, &skill_path).unwrap().is_empty());
    }
}
