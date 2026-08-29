use crate::app_core::{assistant_definitions, is_managed_skill_path, managed_skill_roots};
use crate::database::open_db_connection;
use crate::managed_installation::{
    backfill_managed_roots, find_managed_installation, list_managed_roots,
    prune_missing_managed_installations, record_managed_root, register_managed_root,
};
use crate::skill_library::{copy_origin_to_deployment, resolve_library_path};
use crate::skill_origin::{
    persist_prepared_skill_probe, prepare_skill_probe, prepare_skill_probes, SkillSyncInfo,
};
use crate::skill_reconcile::recover_pending_transactions;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LockResult, Mutex, MutexGuard};

static OPERATION_LOCK: Mutex<()> = Mutex::new(());
pub(crate) type SkillCheckResult = (String, Result<SkillSyncInfo, String>);

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct StartupMaintenanceReport {
    pub(crate) recovered_transactions: usize,
    pub(crate) refreshed_installations: usize,
    pub(crate) pruned_installations: usize,
    pub(crate) warnings: Vec<String>,
}

fn acquire_operation_lock() -> Result<MutexGuard<'static, ()>, String> {
    map_operation_lock(OPERATION_LOCK.lock())
}

fn map_operation_lock<T>(lock: LockResult<T>) -> Result<T, String> {
    lock.map_err(|_| "同步锁已中毒，请重启应用后重试".to_string())
}

pub(crate) fn run_exclusive_operation<T, F>(operation: F) -> Result<T, String>
where
    F: FnOnce(&Connection) -> Result<T, String>,
{
    run_exclusive_operation_with(
        acquire_operation_lock,
        open_db_connection,
        recover_pending_transactions,
        operation,
    )
}

fn run_exclusive_operation_with<T, G, L, D, R, F>(
    acquire_lock: L,
    open_db: D,
    recover: R,
    operation: F,
) -> Result<T, String>
where
    L: FnOnce() -> Result<G, String>,
    D: FnOnce() -> Result<Connection, String>,
    R: FnOnce(&Connection) -> Result<usize, String>,
    F: FnOnce(&Connection) -> Result<T, String>,
{
    let _guard = acquire_lock()?;
    let db = open_db()?;
    recover(&db).map_err(|error| format!("恢复未完成事务失败，已阻止本次操作: {}", error))?;
    operation(&db)
}

pub(crate) fn is_known_skill_path(db: &Connection, path: &Path) -> Result<bool, String> {
    if !path.is_dir() {
        return Ok(false);
    }
    if is_managed_skill_path(path, &managed_skill_roots()) {
        return Ok(true);
    }
    Ok(find_managed_installation(db, path)?.is_some())
}

pub(crate) fn check_skill_update(path: &Path, force: bool) -> Result<SkillSyncInfo, String> {
    let initial_db = open_db_connection()?;
    let requested_path = path.to_path_buf();
    let probe_path = resolve_probe_path(&initial_db, &requested_path)?;
    let prepared = prepare_skill_probe(&initial_db, &probe_path, force)?;
    drop(initial_db);

    run_exclusive_operation(move |db| {
        ensure_known_skill_path(db, &requested_path)?;
        ensure_known_skill_path(db, prepared.path())?;
        let info = persist_prepared_skill_probe(db, prepared)?;
        if requested_path != probe_path {
            copy_origin_to_deployment(db, &probe_path, &requested_path)?;
        }
        Ok(info)
    })
}

pub(crate) fn check_skill_updates(
    paths: &[PathBuf],
    force: bool,
) -> Result<Vec<SkillCheckResult>, String> {
    let initial_db = open_db_connection()?;
    let mut valid_requests = Vec::new();
    let mut invalid = Vec::new();
    for path in paths {
        match resolve_probe_path(&initial_db, path) {
            Ok(probe_path) => valid_requests.push((path.clone(), probe_path)),
            Err(error) => invalid.push((
                path.to_string_lossy().to_string(),
                Err(format!("检查受管路径失败: {}", error)),
            )),
        }
    }
    let mut seen = HashSet::new();
    let probe_paths = valid_requests
        .iter()
        .map(|(_, probe_path)| probe_path.clone())
        .filter(|probe_path| seen.insert(probe_path.clone()))
        .collect::<Vec<_>>();
    let prepared = prepare_skill_probes(&initial_db, &probe_paths, force);
    drop(initial_db);

    run_exclusive_operation(move |db| {
        let probe_results = prepared
            .into_iter()
            .map(|(path, result)| {
                let result = result.and_then(|prepared| {
                    ensure_known_skill_path(db, prepared.path())?;
                    persist_prepared_skill_probe(db, prepared)
                });
                (PathBuf::from(path), result)
            })
            .collect::<HashMap<_, _>>();
        let mut results = valid_requests
            .into_iter()
            .map(|(requested_path, probe_path)| {
                let result = probe_results
                    .get(&probe_path)
                    .cloned()
                    .unwrap_or_else(|| Err("未生成 Skill 检查结果".to_string()));
                let result = result.and_then(|info| {
                    ensure_known_skill_path(db, &requested_path)?;
                    if requested_path != probe_path {
                        copy_origin_to_deployment(db, &probe_path, &requested_path)?;
                    }
                    Ok(info)
                });
                (requested_path.to_string_lossy().to_string(), result)
            })
            .collect::<Vec<_>>();
        results.extend(invalid);
        Ok(results)
    })
}

fn resolve_probe_path(db: &Connection, requested_path: &Path) -> Result<PathBuf, String> {
    ensure_known_skill_path(db, requested_path)?;
    let probe_path = resolve_library_path(db, requested_path)?;
    ensure_known_skill_path(db, &probe_path)?;
    Ok(probe_path)
}

pub(crate) fn run_startup_maintenance(db: &Connection) -> Result<StartupMaintenanceReport, String> {
    let _guard = acquire_operation_lock()?;
    let global_roots = assistant_definitions()
        .iter()
        .flat_map(|assistant| assistant.global_discovery_roots())
        .collect::<Vec<_>>();
    run_startup_maintenance_with(db, &global_roots, || recover_pending_transactions(db))
}

fn run_startup_maintenance_with<F>(
    db: &Connection,
    global_roots: &[PathBuf],
    recover: F,
) -> Result<StartupMaintenanceReport, String>
where
    F: FnOnce() -> Result<usize, String>,
{
    let mut report = StartupMaintenanceReport {
        recovered_transactions: recover()?,
        ..StartupMaintenanceReport::default()
    };

    for root in global_roots {
        if let Err(error) = register_managed_root(db, root, "global", None) {
            report.warnings.push(format!(
                "受管根目录登记失败 {}: {}",
                root.to_string_lossy(),
                error
            ));
        }
    }
    if let Err(error) = backfill_managed_roots(db) {
        report
            .warnings
            .push(format!("受管根目录迁移失败: {}", error));
    }
    match list_managed_roots(db) {
        Ok(roots) => {
            for root in roots {
                match record_managed_root(db, &root.path, &root.scope, root.project_path.as_deref())
                {
                    Ok(count) => report.refreshed_installations += count,
                    Err(error) => report.warnings.push(format!(
                        "受管安装索引恢复失败 {}: {}",
                        root.path.to_string_lossy(),
                        error
                    )),
                }
            }
        }
        Err(error) => report
            .warnings
            .push(format!("读取受管根目录失败: {}", error)),
    }
    match prune_missing_managed_installations(db) {
        Ok(count) => report.pruned_installations = count,
        Err(error) => report
            .warnings
            .push(format!("清理失效受管安装索引失败: {}", error)),
    }
    Ok(report)
}

fn ensure_known_skill_path(db: &Connection, path: &Path) -> Result<(), String> {
    if is_known_skill_path(db, path)? {
        Ok(())
    } else {
        Err("检查期间 Skill 已被删除或不再受管，请刷新后重试".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_state::{content_fingerprint, mark_managed_skill};
    use std::fs;

    fn startup_database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE managed_roots (
                root_path TEXT PRIMARY KEY, scope TEXT NOT NULL,
                project_path TEXT, updated_at TEXT NOT NULL
            );
            CREATE TABLE managed_installations (
                skill_path TEXT PRIMARY KEY, assistant TEXT NOT NULL, source TEXT NOT NULL,
                source_kind TEXT NOT NULL, target_name TEXT NOT NULL, scope TEXT NOT NULL,
                install_mode TEXT NOT NULL, project_path TEXT, tracking_ref TEXT, subdir TEXT,
                resolved_ref TEXT, content_hash TEXT, installed_at TEXT NOT NULL
            );
            CREATE TABLE skill_origin_meta (skill_path TEXT PRIMARY KEY);
            CREATE TABLE skill_tags (skill_path TEXT PRIMARY KEY);",
        )
        .unwrap();
        db
    }

    #[test]
    fn startup_recovers_files_before_refreshing_managed_fingerprint() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-startup-order-{}-{}",
            std::process::id(),
            crate::app_core::generate_id()
        ));
        let skill = root.join("writer");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "old content").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "local:/tmp/writer").unwrap();
        let db = startup_database();
        register_managed_root(&db, &root, "global", None).unwrap();
        record_managed_root(&db, &root, "global", None).unwrap();
        let old_hash = content_fingerprint(&skill).unwrap();

        fs::write(skill.join("SKILL.md"), "temporary new content").unwrap();
        let temporary_hash = content_fingerprint(&skill).unwrap();
        let report = run_startup_maintenance_with(&db, &[], || {
            assert_eq!(content_fingerprint(&skill).unwrap(), temporary_hash);
            fs::write(skill.join("SKILL.md"), "old content").map_err(|error| error.to_string())?;
            Ok(1)
        })
        .unwrap();

        let stored_hash: String = db
            .query_row(
                "SELECT content_hash FROM managed_installations WHERE skill_path = ?",
                [skill.to_string_lossy().to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(report.recovered_transactions, 1);
        assert_eq!(report.refreshed_installations, 1);
        assert_eq!(stored_hash, old_hash);
        assert_ne!(stored_hash, temporary_hash);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_entry_does_not_make_a_missing_path_probeable() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE managed_installations (
                skill_path TEXT PRIMARY KEY, assistant TEXT NOT NULL, source TEXT NOT NULL,
                source_kind TEXT NOT NULL, target_name TEXT NOT NULL, scope TEXT NOT NULL,
                install_mode TEXT NOT NULL, project_path TEXT, tracking_ref TEXT, subdir TEXT,
                resolved_ref TEXT, content_hash TEXT, installed_at TEXT NOT NULL
            );",
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "skillmate-missing-coordinator-path-{}-{}",
            std::process::id(),
            crate::app_core::now_ms()
        ));
        db.execute(
            "INSERT INTO managed_installations VALUES (?, 'Codex', 'source', 'local', 'writer',
             'global', 'copy', NULL, NULL, NULL, NULL, NULL, 'now')",
            [path.to_string_lossy().to_string()],
        )
        .unwrap();

        assert!(!is_known_skill_path(&db, &path).unwrap());
    }

    #[test]
    fn update_probe_resolves_deployment_to_library_copy() {
        let db = startup_database();
        db.execute_batch(
            "CREATE TABLE library_skills (
                id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, library_path TEXT NOT NULL UNIQUE,
                source TEXT NOT NULL, source_kind TEXT NOT NULL, resolved_ref TEXT,
                content_hash TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );
             CREATE TABLE skill_deployments (
                target_path TEXT PRIMARY KEY, skill_id TEXT NOT NULL, library_path TEXT NOT NULL,
                assistant TEXT NOT NULL, scope TEXT NOT NULL, project_path TEXT,
                deploy_mode TEXT NOT NULL, deployed_at TEXT NOT NULL
             );",
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "skillmate-update-probe-{}-{}",
            std::process::id(),
            crate::app_core::generate_id()
        ));
        let library_path = root.join("library/writer");
        let deployment_path = root.join("deployments/writer");
        fs::create_dir_all(&library_path).unwrap();
        fs::create_dir_all(deployment_path.parent().unwrap()).unwrap();
        fs::write(library_path.join("SKILL.md"), "writer").unwrap();
        if !crate::app_core::create_test_directory_symlink_or_skip(&library_path, &deployment_path)
        {
            let _ = fs::remove_dir_all(root);
            return;
        }
        for (path, assistant, scope) in [
            (
                &library_path,
                crate::managed_state::LIBRARY_OWNER_NAME,
                "library",
            ),
            (&deployment_path, "Codex", "global"),
        ] {
            db.execute(
                "INSERT INTO managed_installations VALUES (?, ?, '/tmp/writer', 'git', 'writer',
                 ?, 'copy', NULL, NULL, NULL, NULL, 'hash', 'now')",
                rusqlite::params![path.to_string_lossy().to_string(), assistant, scope],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO skill_deployments VALUES (?, 'skill-1', ?, 'Codex', 'global', NULL,
             'symlink', 'now')",
            rusqlite::params![
                deployment_path.to_string_lossy().to_string(),
                library_path.to_string_lossy().to_string()
            ],
        )
        .unwrap();

        assert_eq!(
            resolve_probe_path(&db, &deployment_path).unwrap(),
            library_path
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn poisoned_operation_lock_maps_to_recoverable_error() {
        let lock = Mutex::new(());
        let _ = std::panic::catch_unwind(|| {
            let _guard = lock.lock().unwrap();
            panic!("poison test lock");
        });

        let error = map_operation_lock(lock.lock()).unwrap_err();

        assert_eq!(error, "同步锁已中毒，请重启应用后重试");
    }

    #[test]
    fn exclusive_operation_recovers_before_running() {
        let steps = std::cell::RefCell::new(Vec::new());

        let result = run_exclusive_operation_with(
            || {
                steps.borrow_mut().push("lock");
                Ok(())
            },
            || {
                steps.borrow_mut().push("database");
                Connection::open_in_memory().map_err(|error| error.to_string())
            },
            |_| {
                steps.borrow_mut().push("recover");
                Ok(1)
            },
            |_| {
                steps.borrow_mut().push("operation");
                Ok("done")
            },
        )
        .unwrap();

        assert_eq!(result, "done");
        assert_eq!(
            *steps.borrow(),
            vec!["lock", "database", "recover", "operation"]
        );
    }

    #[test]
    fn exclusive_operation_stops_when_recovery_fails() {
        let operation_ran = std::cell::Cell::new(false);

        let error = run_exclusive_operation_with(
            || Ok(()),
            || Connection::open_in_memory().map_err(|error| error.to_string()),
            |_| Err("journal 损坏".to_string()),
            |_| {
                operation_ran.set(true);
                Ok(())
            },
        )
        .unwrap_err();

        assert_eq!(error, "恢复未完成事务失败，已阻止本次操作: journal 损坏");
        assert!(!operation_ran.get());
    }
}
