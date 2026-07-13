use crate::managed_state::{
    content_fingerprint, fingerprint_matches, is_managed_by_state, managed_state_entry,
    read_managed_state, unmark_managed_skill, ManagedSkillState, ManagedStateCheckpoint,
};
use crate::skill_install_source::{is_git_install_source, parse_git_install_spec};
use crate::skillmate_manifest::SkillMateManifestSkill;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ManagedInstallation {
    pub path: PathBuf,
    pub skill: SkillMateManifestSkill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRoot {
    pub path: PathBuf,
    pub scope: String,
    pub project_path: Option<String>,
}

#[derive(Debug, Clone)]
struct OriginMetadataRow {
    origin_kind: String,
    origin_locator: String,
    resolved_locator: String,
    tracking_ref: String,
    installed_ref: String,
    latest_ref: String,
    sync_state: String,
    sync_message: String,
    lag_count: i64,
    last_probe_at: Option<i64>,
    last_sync_at: Option<i64>,
    managed_by_app: i64,
}

#[derive(Debug, Clone)]
struct ManagedInstallationRow {
    assistant: String,
    source: String,
    source_kind: String,
    target_name: String,
    scope: String,
    install_mode: String,
    project_path: Option<String>,
    tracking_ref: Option<String>,
    subdir: Option<String>,
    resolved_ref: Option<String>,
    content_hash: Option<String>,
    installed_at: String,
}

#[derive(Debug, Clone)]
struct SkillTagsRow {
    tags: Option<String>,
    tags_json: String,
}

#[derive(Debug, Clone)]
struct PathMetadataCheckpoint {
    path: PathBuf,
    origin: Option<OriginMetadataRow>,
    installation: Option<ManagedInstallationRow>,
    tags: Option<SkillTagsRow>,
}

#[derive(Debug, Clone)]
pub struct ManagedMetadataCheckpoint {
    paths: Vec<PathMetadataCheckpoint>,
    roots: Vec<(PathBuf, ManagedStateCheckpoint)>,
}

impl ManagedMetadataCheckpoint {
    pub fn capture(db: &Connection, paths: &[PathBuf]) -> Result<Self, String> {
        let mut unique_paths = paths.to_vec();
        unique_paths.sort();
        unique_paths.dedup();
        let mut roots = unique_paths
            .iter()
            .filter_map(|path| path.parent().map(Path::to_path_buf))
            .collect::<Vec<_>>();
        roots.sort();
        roots.dedup();
        let roots = roots
            .into_iter()
            .map(|root| ManagedStateCheckpoint::capture(&root).map(|state| (root, state)))
            .collect::<Result<Vec<_>, _>>()?;
        let paths = unique_paths
            .into_iter()
            .map(|path| capture_path_metadata(db, path))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { paths, roots })
    }

    pub fn restore(&self, db: &Connection) -> Result<(), String> {
        let transaction = db
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        for checkpoint in &self.paths {
            restore_path_metadata(&transaction, checkpoint)?;
        }
        transaction.commit().map_err(|error| error.to_string())?;

        let mut errors = Vec::new();
        for (root, state) in &self.roots {
            if let Err(error) = state.restore(root) {
                errors.push(format!("{}: {}", root.to_string_lossy(), error));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("恢复受管状态失败: {}", errors.join("；")))
        }
    }
}

fn capture_path_metadata(db: &Connection, path: PathBuf) -> Result<PathMetadataCheckpoint, String> {
    let key = path.to_string_lossy().to_string();
    let origin = db
        .query_row(
            "SELECT origin_kind, origin_locator, resolved_locator, tracking_ref, installed_ref,
                    latest_ref, sync_state, sync_message, lag_count, last_probe_at, last_sync_at,
                    managed_by_app
             FROM skill_origin_meta WHERE skill_path = ?",
            [&key],
            |row| {
                Ok(OriginMetadataRow {
                    origin_kind: row.get(0)?,
                    origin_locator: row.get(1)?,
                    resolved_locator: row.get(2)?,
                    tracking_ref: row.get(3)?,
                    installed_ref: row.get(4)?,
                    latest_ref: row.get(5)?,
                    sync_state: row.get(6)?,
                    sync_message: row.get(7)?,
                    lag_count: row.get(8)?,
                    last_probe_at: row.get(9)?,
                    last_sync_at: row.get(10)?,
                    managed_by_app: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let installation = db
        .query_row(
            "SELECT assistant, source, source_kind, target_name, scope, install_mode,
                    project_path, tracking_ref, subdir, resolved_ref, content_hash, installed_at
             FROM managed_installations WHERE skill_path = ?",
            [&key],
            |row| {
                Ok(ManagedInstallationRow {
                    assistant: row.get(0)?,
                    source: row.get(1)?,
                    source_kind: row.get(2)?,
                    target_name: row.get(3)?,
                    scope: row.get(4)?,
                    install_mode: row.get(5)?,
                    project_path: row.get(6)?,
                    tracking_ref: row.get(7)?,
                    subdir: row.get(8)?,
                    resolved_ref: row.get(9)?,
                    content_hash: row.get(10)?,
                    installed_at: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let tags = db
        .query_row(
            "SELECT tags, tags_json FROM skill_tags WHERE skill_path = ?",
            [&key],
            |row| {
                Ok(SkillTagsRow {
                    tags: row.get(0)?,
                    tags_json: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(PathMetadataCheckpoint {
        path,
        origin,
        installation,
        tags,
    })
}

fn restore_path_metadata(
    db: &Connection,
    checkpoint: &PathMetadataCheckpoint,
) -> Result<(), String> {
    let key = checkpoint.path.to_string_lossy().to_string();
    db.execute("DELETE FROM skill_origin_meta WHERE skill_path = ?", [&key])
        .map_err(|error| error.to_string())?;
    db.execute(
        "DELETE FROM managed_installations WHERE skill_path = ?",
        [&key],
    )
    .map_err(|error| error.to_string())?;
    db.execute("DELETE FROM skill_tags WHERE skill_path = ?", [&key])
        .map_err(|error| error.to_string())?;
    if let Some(row) = &checkpoint.origin {
        db.execute(
            "INSERT INTO skill_origin_meta (
                skill_path, origin_kind, origin_locator, resolved_locator, tracking_ref,
                installed_ref, latest_ref, sync_state, sync_message, lag_count, last_probe_at,
                last_sync_at, managed_by_app
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                &key,
                &row.origin_kind,
                &row.origin_locator,
                &row.resolved_locator,
                &row.tracking_ref,
                &row.installed_ref,
                &row.latest_ref,
                &row.sync_state,
                &row.sync_message,
                row.lag_count,
                row.last_probe_at,
                row.last_sync_at,
                row.managed_by_app,
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    if let Some(row) = &checkpoint.installation {
        db.execute(
            "INSERT INTO managed_installations (
                skill_path, assistant, source, source_kind, target_name, scope, install_mode,
                project_path, tracking_ref, subdir, resolved_ref, content_hash, installed_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                &key,
                &row.assistant,
                &row.source,
                &row.source_kind,
                &row.target_name,
                &row.scope,
                &row.install_mode,
                &row.project_path,
                &row.tracking_ref,
                &row.subdir,
                &row.resolved_ref,
                &row.content_hash,
                &row.installed_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    if let Some(row) = &checkpoint.tags {
        db.execute(
            "INSERT INTO skill_tags (skill_path, tags, tags_json) VALUES (?, ?, ?)",
            params![&key, &row.tags, &row.tags_json],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn list_managed_installations(db: &Connection) -> Result<Vec<ManagedInstallation>, String> {
    let mut statement = db
        .prepare(
            "SELECT skill_path, assistant, source, source_kind, target_name, scope,
                    install_mode, project_path, tracking_ref, subdir, resolved_ref, content_hash
             FROM managed_installations ORDER BY skill_path",
        )
        .map_err(|error| error.to_string())?;
    let installations = statement
        .query_map([], |row| {
            Ok(ManagedInstallation {
                path: PathBuf::from(row.get::<_, String>(0)?),
                skill: SkillMateManifestSkill {
                    assistant: row.get(1)?,
                    source: row.get(2)?,
                    source_kind: row.get(3)?,
                    target_name: Some(row.get(4)?),
                    scope: Some(row.get(5)?),
                    install_mode: Some(row.get(6)?),
                    project_path: row.get(7)?,
                    reference: row.get(8)?,
                    subdir: row.get(9)?,
                    resolved_ref: row.get(10)?,
                    content_hash: row.get(11)?,
                },
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(installations)
}

pub fn register_managed_root(
    db: &Connection,
    root: &Path,
    scope: &str,
    project_path: Option<&str>,
) -> Result<(), String> {
    db.execute(
        "INSERT INTO managed_roots (root_path, scope, project_path, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(root_path) DO UPDATE SET
            scope = excluded.scope,
            project_path = COALESCE(excluded.project_path, managed_roots.project_path),
            updated_at = excluded.updated_at",
        params![
            root.to_string_lossy().to_string(),
            scope,
            project_path,
            chrono::Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn list_managed_roots(db: &Connection) -> Result<Vec<ManagedRoot>, String> {
    let mut statement = db
        .prepare("SELECT root_path, scope, project_path FROM managed_roots ORDER BY root_path")
        .map_err(|error| error.to_string())?;
    let roots = statement
        .query_map([], |row| {
            Ok(ManagedRoot {
                path: PathBuf::from(row.get::<_, String>(0)?),
                scope: row.get(1)?,
                project_path: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(roots)
}

pub fn backfill_managed_roots(db: &Connection) -> Result<usize, String> {
    let installations = list_managed_installations(db)?;
    let mut roots = std::collections::BTreeMap::new();
    for installation in installations {
        let Some(root) = installation.path.parent().map(Path::to_path_buf) else {
            continue;
        };
        roots.entry(root).or_insert_with(|| {
            (
                installation
                    .skill
                    .scope
                    .clone()
                    .unwrap_or_else(|| "global".to_string()),
                installation.skill.project_path.clone(),
            )
        });
    }
    for (root, (scope, project_path)) in &roots {
        register_managed_root(db, root, scope, project_path.as_deref())?;
    }
    Ok(roots.len())
}

pub fn find_managed_installation(
    db: &Connection,
    path: &Path,
) -> Result<Option<ManagedInstallation>, String> {
    let path = path.to_string_lossy().to_string();
    let mut statement = db
        .prepare(
            "SELECT skill_path, assistant, source, source_kind, target_name, scope,
                    install_mode, project_path, tracking_ref, subdir, resolved_ref, content_hash
             FROM managed_installations WHERE skill_path = ?",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([path]).map_err(|error| error.to_string())?;
    let Some(row) = rows.next().map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    Ok(Some(ManagedInstallation {
        path: PathBuf::from(row.get::<_, String>(0).map_err(|error| error.to_string())?),
        skill: SkillMateManifestSkill {
            assistant: row.get(1).map_err(|error| error.to_string())?,
            source: row.get(2).map_err(|error| error.to_string())?,
            source_kind: row.get(3).map_err(|error| error.to_string())?,
            target_name: Some(row.get(4).map_err(|error| error.to_string())?),
            scope: Some(row.get(5).map_err(|error| error.to_string())?),
            install_mode: Some(row.get(6).map_err(|error| error.to_string())?),
            project_path: row.get(7).map_err(|error| error.to_string())?,
            reference: row.get(8).map_err(|error| error.to_string())?,
            subdir: row.get(9).map_err(|error| error.to_string())?,
            resolved_ref: row.get(10).map_err(|error| error.to_string())?,
            content_hash: row.get(11).map_err(|error| error.to_string())?,
        },
    }))
}

pub fn is_explicitly_managed(db: &Connection, path: &Path) -> Result<bool, String> {
    if find_managed_installation(db, path)?.is_some() {
        return Ok(true);
    }
    match path.parent() {
        Some(root) => is_managed_by_state(root, path),
        None => Ok(false),
    }
}

pub fn verify_managed_content_unchanged(db: &Connection, path: &Path) -> Result<(), String> {
    let installation = find_managed_installation(db, path)?;
    let state_entry = match path.parent() {
        Some(root) => managed_state_entry(root, path)?,
        None => None,
    };
    let mut expected = Vec::new();
    if let Some(hash) = installation
        .as_ref()
        .and_then(|item| item.skill.content_hash.as_deref())
        .filter(|hash| !hash.trim().is_empty())
    {
        expected.push(("数据库", hash));
    }
    if let Some(hash) = state_entry
        .as_ref()
        .map(|item| item.last_seen_hash.as_str())
        .filter(|hash| !hash.trim().is_empty())
    {
        expected.push(("受管状态", hash));
    }
    if expected.is_empty() {
        return Err("受管 Skill 缺少内容指纹，已拒绝破坏性操作".to_string());
    }
    for (source, hash) in expected {
        if !fingerprint_matches(path, hash)? {
            return Err(format!(
                "Skill 内容已偏离安装时状态（{}指纹不一致），请先备份或重新安装",
                source
            ));
        }
    }
    Ok(())
}

pub fn record_managed_root(
    db: &Connection,
    root: &Path,
    scope: &str,
    project_path: Option<&str>,
) -> Result<usize, String> {
    register_managed_root(db, root, scope, project_path)?;
    let state = read_managed_state(root)?;
    let mut records = Vec::new();
    for entry in state.managed_skills {
        if let Some(record) = managed_record_from_state(&entry, scope, project_path)? {
            records.push(record);
        }
    }
    let transaction = db
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    for (path, skill) in &records {
        record_managed_installation(&transaction, path, skill)?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(records.len())
}

pub fn record_managed_path(
    db: &Connection,
    root: &Path,
    path: &Path,
    scope: &str,
    project_path: Option<&str>,
) -> Result<(), String> {
    register_managed_root(db, root, scope, project_path)?;
    let entry = managed_state_entry(root, path)?
        .ok_or_else(|| "安装完成后未找到对应受管状态".to_string())?;
    let (path, skill) = managed_record_from_state(&entry, scope, project_path)?
        .ok_or_else(|| "安装目标在登记前已消失".to_string())?;
    record_managed_installation(db, &path, &skill)
}

fn managed_record_from_state(
    entry: &ManagedSkillState,
    scope: &str,
    project_path: Option<&str>,
) -> Result<Option<(PathBuf, SkillMateManifestSkill)>, String> {
    let path = PathBuf::from(&entry.path);
    if !path.exists() && std::fs::symlink_metadata(&path).is_err() {
        return Ok(None);
    }
    let (source, source_kind, install_mode) = parse_state_origin(&entry.origin);
    let parsed_git = if is_git_install_source(&source_kind) {
        parse_git_install_spec(&source).ok()
    } else {
        None
    };
    let skill = SkillMateManifestSkill {
        assistant: entry.assistant.clone(),
        source,
        source_kind,
        target_name: path
            .file_name()
            .map(|value| value.to_string_lossy().to_string()),
        scope: Some(scope.to_string()),
        install_mode: Some(install_mode),
        project_path: project_path.map(str::to_string),
        reference: parsed_git.as_ref().and_then(|spec| spec.reference.clone()),
        subdir: parsed_git.as_ref().and_then(|spec| spec.subdir.clone()),
        resolved_ref: None,
        content_hash: Some(content_fingerprint(&path)?),
    };
    Ok(Some((path, skill)))
}

pub fn record_managed_installation(
    db: &Connection,
    path: &Path,
    skill: &SkillMateManifestSkill,
) -> Result<(), String> {
    let target_name = skill
        .target_name
        .as_deref()
        .or_else(|| path.file_name().and_then(|value| value.to_str()))
        .ok_or_else(|| "受管安装缺少 target_name".to_string())?;
    let content_hash = content_fingerprint(path)?;
    db.execute(
        "INSERT INTO managed_installations (
            skill_path, assistant, source, source_kind, target_name, scope, install_mode,
            project_path, tracking_ref, subdir, resolved_ref, content_hash, installed_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(skill_path) DO UPDATE SET
            assistant = excluded.assistant,
            source = excluded.source,
            source_kind = excluded.source_kind,
            target_name = excluded.target_name,
            scope = excluded.scope,
            install_mode = excluded.install_mode,
            project_path = excluded.project_path,
            tracking_ref = excluded.tracking_ref,
            subdir = excluded.subdir,
            resolved_ref = COALESCE(excluded.resolved_ref, managed_installations.resolved_ref),
            content_hash = excluded.content_hash",
        params![
            path.to_string_lossy().to_string(),
            skill.assistant,
            skill.source,
            skill.source_kind,
            target_name,
            skill.scope.as_deref().unwrap_or("global"),
            skill.install_mode.as_deref().unwrap_or("copy"),
            skill.project_path,
            skill.reference,
            skill.subdir,
            skill.resolved_ref,
            content_hash,
            chrono::Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn refresh_managed_installation(
    db: &Connection,
    path: &Path,
    resolved_ref: Option<&str>,
) -> Result<bool, String> {
    let content_hash = content_fingerprint(path)?;
    let updated = db
        .execute(
            "UPDATE managed_installations
             SET content_hash = ?, resolved_ref = COALESCE(?, resolved_ref)
             WHERE skill_path = ?",
            params![
                content_hash,
                resolved_ref.filter(|value| !value.trim().is_empty()),
                path.to_string_lossy().to_string()
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(updated > 0)
}

pub fn cleanup_skill_metadata(db: &Connection, target_path: &Path) -> Result<(), String> {
    let path = target_path.to_string_lossy().to_string();
    let state_checkpoint = target_path
        .parent()
        .map(ManagedStateCheckpoint::capture)
        .transpose()?;
    if let Some(root) = target_path.parent() {
        unmark_managed_skill(root, target_path)?;
    }
    let database_result = (|| {
        let transaction = db
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        delete_path_metadata(&transaction, &path)?;
        transaction.commit().map_err(|error| error.to_string())
    })();
    if let Err(error) = database_result {
        if let (Some(root), Some(checkpoint)) = (target_path.parent(), state_checkpoint) {
            return match checkpoint.restore(root) {
                Ok(()) => Err(error),
                Err(restore_error) => {
                    Err(format!("{}；恢复受管状态失败: {}", error, restore_error))
                }
            };
        }
        return Err(error);
    }
    Ok(())
}

pub fn prune_missing_managed_installations(db: &Connection) -> Result<usize, String> {
    let missing = list_managed_installations(db)?
        .into_iter()
        .filter(|installation| {
            !installation.path.exists() && std::fs::symlink_metadata(&installation.path).is_err()
        })
        .map(|installation| installation.path)
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(0);
    }
    let checkpoint = ManagedMetadataCheckpoint::capture(db, &missing)?;
    for path in &missing {
        if let Some(root) = path.parent() {
            unmark_managed_skill(root, path)?;
        }
    }
    let database_result = (|| {
        let transaction = db
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        for path in &missing {
            delete_path_metadata(&transaction, &path.to_string_lossy())?;
        }
        transaction.commit().map_err(|error| error.to_string())
    })();
    if let Err(error) = database_result {
        return match checkpoint.restore(db) {
            Ok(()) => Err(error),
            Err(restore_error) => Err(format!(
                "{}；恢复失效受管记录失败: {}",
                error, restore_error
            )),
        };
    }
    Ok(missing.len())
}

fn delete_path_metadata(db: &Connection, path: &str) -> Result<(), String> {
    db.execute(
        "DELETE FROM skill_origin_meta WHERE skill_path = ?",
        params![path],
    )
    .map_err(|error| error.to_string())?;
    db.execute("DELETE FROM skill_tags WHERE skill_path = ?", params![path])
        .map_err(|error| error.to_string())?;
    db.execute(
        "DELETE FROM managed_installations WHERE skill_path = ?",
        params![path],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn parse_state_origin(origin: &str) -> (String, String, String) {
    if let Some(source) = origin.strip_prefix("symlink:") {
        return (
            source.to_string(),
            "local".to_string(),
            "symlink".to_string(),
        );
    }
    if let Some(source) = origin.strip_prefix("local:") {
        return (source.to_string(), "local".to_string(), "copy".to_string());
    }
    (origin.to_string(), "git".to_string(), "copy".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn records_project_installation_from_managed_state() {
        let db = database();
        let root = std::env::temp_dir().join(format!(
            "skillmate-managed-registry-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let skill = root.join("writer");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "writer").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "local:/tmp/writer").unwrap();

        record_managed_root(&db, &root, "project", Some("/tmp/project")).unwrap();
        let installations = list_managed_installations(&db).unwrap();

        assert_eq!(installations.len(), 1);
        assert_eq!(installations[0].skill.scope.as_deref(), Some("project"));
        assert_eq!(installations[0].skill.source, "/tmp/writer");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prunes_registry_entries_when_targets_disappear() {
        let db = database();
        let root = std::env::temp_dir().join(format!(
            "skillmate-managed-prune-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let skill = root.join("writer");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "writer").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "local:/tmp/writer").unwrap();
        record_managed_root(&db, &root, "global", None).unwrap();
        let key = skill.to_string_lossy().to_string();
        db.execute(
            "INSERT INTO skill_origin_meta VALUES (?, 'local', '/tmp/writer', '/tmp/writer', '',
             '', '', 'current', '', 0, NULL, NULL, 1)",
            [&key],
        )
        .unwrap();
        db.execute(
            "INSERT INTO skill_tags (skill_path, tags, tags_json) VALUES (?, '', '[\"old\"]')",
            [&key],
        )
        .unwrap();
        std::fs::remove_dir_all(&skill).unwrap();

        assert_eq!(prune_missing_managed_installations(&db).unwrap(), 1);
        assert!(list_managed_installations(&db).unwrap().is_empty());
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM skill_origin_meta WHERE skill_path = ?",
                [&key],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM skill_tags WHERE skill_path = ?",
                [&key],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        assert!(crate::managed_state::read_managed_state(&root)
            .unwrap()
            .managed_skills
            .is_empty());
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "replacement").unwrap();
        assert!(!is_explicitly_managed(&db, &skill).unwrap());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn registered_project_root_recovers_sidecar_after_interrupted_install() {
        let db = database();
        let root = std::env::temp_dir().join(format!(
            "skillmate-managed-recover-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let skill = root.join("writer");
        register_managed_root(&db, &root, "project", Some("/tmp/project")).unwrap();
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "writer").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "local:/tmp/writer").unwrap();

        let registered = list_managed_roots(&db).unwrap();
        assert_eq!(registered.len(), 1);
        record_managed_root(
            &db,
            &registered[0].path,
            &registered[0].scope,
            registered[0].project_path.as_deref(),
        )
        .unwrap();

        let recovered = find_managed_installation(&db, &skill).unwrap().unwrap();
        assert_eq!(recovered.skill.scope.as_deref(), Some("project"));
        assert_eq!(
            recovered.skill.project_path.as_deref(),
            Some("/tmp/project")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_management_requires_registry_or_sidecar() {
        let db = database();
        let root = std::env::temp_dir().join(format!(
            "skillmate-managed-owner-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let skill = root.join("writer");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "writer").unwrap();

        assert!(!is_explicitly_managed(&db, &skill).unwrap());
        mark_managed_skill(&root, "Codex", &skill, "local:/tmp/writer").unwrap();
        assert!(is_explicitly_managed(&db, &skill).unwrap());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn refreshes_registry_hash_and_resolved_ref() {
        let db = database();
        let root = std::env::temp_dir().join(format!(
            "skillmate-managed-refresh-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let skill = root.join("writer");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "old").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "example/skills").unwrap();
        record_managed_root(&db, &root, "global", None).unwrap();
        let old_hash = find_managed_installation(&db, &skill)
            .unwrap()
            .unwrap()
            .skill
            .content_hash
            .unwrap();
        std::fs::write(skill.join("SKILL.md"), "new").unwrap();

        assert!(refresh_managed_installation(&db, &skill, Some("abc123")).unwrap());

        let installation = find_managed_installation(&db, &skill).unwrap().unwrap();
        assert_ne!(
            installation.skill.content_hash.as_deref(),
            Some(old_hash.as_str())
        );
        assert_eq!(installation.skill.resolved_ref.as_deref(), Some("abc123"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn destructive_operations_reject_managed_content_drift() {
        let db = database();
        let root = std::env::temp_dir().join(format!(
            "skillmate-managed-drift-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let skill = root.join("writer");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "original").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "local:/tmp/writer").unwrap();
        record_managed_root(&db, &root, "global", None).unwrap();
        assert!(verify_managed_content_unchanged(&db, &skill).is_ok());

        std::fs::write(skill.join("SKILL.md"), "changed").unwrap();

        let error = verify_managed_content_unchanged(&db, &skill).unwrap_err();
        assert!(error.contains("偏离安装时状态"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn metadata_checkpoint_restores_registry_origin_tags_and_sidecar() {
        let db = database();
        let root = std::env::temp_dir().join(format!(
            "skillmate-managed-checkpoint-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let skill = root.join("writer");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "writer").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "local:/old/writer").unwrap();
        record_managed_root(&db, &root, "global", None).unwrap();
        let key = skill.to_string_lossy().to_string();
        db.execute(
            "INSERT INTO skill_origin_meta VALUES (?, 'git', 'old/repo', 'old/repo', 'main',
             'old-ref', '', 'current', '', 0, NULL, NULL, 1)",
            [&key],
        )
        .unwrap();
        db.execute(
            "INSERT INTO skill_tags (skill_path, tags, tags_json) VALUES (?, '', '[\"old\"]')",
            [&key],
        )
        .unwrap();
        let checkpoint =
            ManagedMetadataCheckpoint::capture(&db, std::slice::from_ref(&skill)).unwrap();

        mark_managed_skill(&root, "Codex", &skill, "local:/new/writer").unwrap();
        db.execute(
            "UPDATE managed_installations SET source = 'new/repo' WHERE skill_path = ?",
            [&key],
        )
        .unwrap();
        db.execute(
            "UPDATE skill_origin_meta SET origin_locator = 'new/repo' WHERE skill_path = ?",
            [&key],
        )
        .unwrap();
        db.execute("DELETE FROM skill_tags WHERE skill_path = ?", [&key])
            .unwrap();

        checkpoint.restore(&db).unwrap();

        assert_eq!(
            find_managed_installation(&db, &skill)
                .unwrap()
                .unwrap()
                .skill
                .source,
            "/old/writer"
        );
        assert_eq!(
            crate::managed_state::managed_state_origin(&root, &skill)
                .unwrap()
                .as_deref(),
            Some("local:/old/writer")
        );
        assert_eq!(
            db.query_row(
                "SELECT origin_locator FROM skill_origin_meta WHERE skill_path = ?",
                [&key],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "old/repo"
        );
        assert_eq!(
            db.query_row(
                "SELECT tags_json FROM skill_tags WHERE skill_path = ?",
                [&key],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "[\"old\"]"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_restores_sidecar_when_database_delete_fails() {
        let db = database();
        let root = std::env::temp_dir().join(format!(
            "skillmate-managed-cleanup-failure-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let skill = root.join("writer");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "writer").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "local:/tmp/writer").unwrap();
        record_managed_root(&db, &root, "global", None).unwrap();
        let key = skill.to_string_lossy().to_string();
        db.execute(
            "INSERT INTO skill_origin_meta VALUES (?, 'local', '/tmp/writer', '/tmp/writer', '',
             '', '', 'current', '', 0, NULL, NULL, 1)",
            [&key],
        )
        .unwrap();
        db.execute(
            "INSERT INTO skill_tags (skill_path, tags, tags_json) VALUES (?, '', '[\"old\"]')",
            [&key],
        )
        .unwrap();
        db.execute_batch(
            "CREATE TRIGGER reject_managed_delete
             BEFORE DELETE ON managed_installations
             BEGIN
                 SELECT RAISE(FAIL, 'forced delete failure');
             END;",
        )
        .unwrap();

        let error = cleanup_skill_metadata(&db, &skill).unwrap_err();

        assert!(error.contains("forced delete failure"));
        assert!(crate::managed_state::is_managed_by_state(&root, &skill).unwrap());
        assert!(find_managed_installation(&db, &skill).unwrap().is_some());
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM skill_origin_meta WHERE skill_path = ?",
                [&key],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM skill_tags WHERE skill_path = ?",
                [&key],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert!(skill.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
