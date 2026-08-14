use crate::app_core::atomic_write;
use crate::managed_installation::ManagedMetadataCheckpoint;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const JOURNAL_VERSION: u32 = 3;
const PREPARED_SUFFIX: &str = ".prepared.json";
const COMMITTED_SUFFIX: &str = ".committed.json";
static TRANSACTION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
thread_local! {
    static DIRECTORY_SYNC_FAILURE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReconcileJournal {
    version: u32,
    created_targets: Vec<PathBuf>,
    moved_targets: Vec<(PathBuf, PathBuf)>,
    #[serde(default)]
    metadata_checkpoint: Option<ManagedMetadataCheckpoint>,
    #[serde(default)]
    file_checkpoints: Vec<FileCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileCheckpoint {
    path: PathBuf,
    previous_contents: Option<Vec<u8>>,
}

impl FileCheckpoint {
    fn capture(path: &Path) -> Result<Self, String> {
        let previous_contents = match fs::read(path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !path_exists(path) => {
                None
            }
            Err(error) => {
                return Err(format!(
                    "无法读取文件检查点 {}: {}",
                    path.to_string_lossy(),
                    error
                ));
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            previous_contents,
        })
    }

    fn restore(&self) -> Result<(), String> {
        match &self.previous_contents {
            Some(contents) => {
                if path_exists(&self.path)
                    && fs::symlink_metadata(&self.path)
                        .map_err(|error| error.to_string())?
                        .is_dir()
                {
                    remove_path(&self.path)?;
                }
                atomic_write(&self.path, contents)?;
                sync_parent(&self.path).map_err(|error| error.to_string())
            }
            None => {
                if path_exists(&self.path) {
                    remove_path(&self.path)?;
                }
                sync_existing_parent(&self.path)
            }
        }
        .map_err(|error| {
            format!(
                "恢复文件检查点 {} 失败: {}",
                self.path.to_string_lossy(),
                error
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransactionState {
    Prepared,
    Committed,
}

#[derive(Debug)]
pub struct ReconcileTransaction<'db> {
    journal: ReconcileJournal,
    journal_path: Option<PathBuf>,
    state: TransactionState,
    finalized: bool,
    db: Option<&'db Connection>,
}

#[cfg(test)]
impl ReconcileTransaction<'static> {
    fn prepare_in(
        removals: &[PathBuf],
        install_targets: &[PathBuf],
        journal_directory: &Path,
    ) -> Result<Self, String> {
        prepare_transaction(
            removals,
            install_targets,
            None,
            Vec::new(),
            None,
            journal_directory,
        )
    }

    fn prepare_in_with_files(
        removals: &[PathBuf],
        install_targets: &[PathBuf],
        checkpoint_paths: &[PathBuf],
        journal_directory: &Path,
    ) -> Result<Self, String> {
        prepare_transaction(
            removals,
            install_targets,
            None,
            capture_file_checkpoints(checkpoint_paths)?,
            None,
            journal_directory,
        )
    }
}

impl<'db> ReconcileTransaction<'db> {
    pub fn prepare_managed(
        db: &'db Connection,
        removals: &[PathBuf],
        install_targets: &[PathBuf],
    ) -> Result<Self, String> {
        Self::prepare_managed_in(db, removals, install_targets, &journal_directory())
    }

    pub fn prepare_managed_with_files(
        db: &'db Connection,
        removals: &[PathBuf],
        install_targets: &[PathBuf],
        checkpoint_paths: &[PathBuf],
    ) -> Result<Self, String> {
        Self::prepare_managed_with_files_in(
            db,
            removals,
            install_targets,
            checkpoint_paths,
            &journal_directory(),
        )
    }

    fn prepare_managed_in(
        db: &'db Connection,
        removals: &[PathBuf],
        install_targets: &[PathBuf],
        journal_directory: &Path,
    ) -> Result<Self, String> {
        Self::prepare_managed_with_files_in(db, removals, install_targets, &[], journal_directory)
    }

    fn prepare_managed_with_files_in(
        db: &'db Connection,
        removals: &[PathBuf],
        install_targets: &[PathBuf],
        checkpoint_paths: &[PathBuf],
        journal_directory: &Path,
    ) -> Result<Self, String> {
        let mut metadata_paths = removals.to_vec();
        metadata_paths.extend_from_slice(install_targets);
        let checkpoint = ManagedMetadataCheckpoint::capture(db, &metadata_paths)?;
        prepare_transaction(
            removals,
            install_targets,
            Some(checkpoint),
            capture_file_checkpoints(checkpoint_paths)?,
            Some(db),
            journal_directory,
        )
    }

    pub fn commit(mut self) -> Result<Option<String>, String> {
        if let Err(error) = self.sync_commit_state() {
            return Err(self.rollback_precommit_failure(error));
        }
        let mut warnings = Vec::new();
        match self.mark_committed() {
            Ok(Some(warning)) => warnings.push(warning),
            Ok(None) => {}
            Err(error) => return Err(self.rollback_precommit_failure(error)),
        }
        if let Err(warning) = self.finish_commit() {
            warnings.push(warning);
        }
        Ok((!warnings.is_empty()).then(|| warnings.join("；")))
    }

    pub fn rollback(&mut self) -> Result<(), String> {
        if self.finalized {
            return Ok(());
        }
        rollback_prepared_journal(&self.journal, self.db)?;
        self.remove_journal()?;
        self.finalized = true;
        Ok(())
    }

    fn sync_commit_state(&self) -> Result<(), String> {
        sync_journal_commit_state(&self.journal)
    }

    fn mark_committed(&mut self) -> Result<Option<String>, String> {
        if self.state == TransactionState::Committed {
            return Ok(None);
        }
        let Some(prepared_path) = self.journal_path.as_ref() else {
            self.state = TransactionState::Committed;
            return Ok(None);
        };
        let committed_path = replace_journal_suffix(prepared_path, COMMITTED_SUFFIX)?;
        fs::rename(prepared_path, &committed_path)
            .map_err(|error| format!("无法标记文件事务已提交: {}", error))?;
        self.journal_path = Some(committed_path.clone());
        self.state = TransactionState::Committed;
        Ok(committed_path.parent().and_then(|directory| {
            sync_directory(directory)
                .err()
                .map(|error| format!("文件事务已提交，但提交标记目录同步失败: {}", error))
        }))
    }

    fn rollback_precommit_failure(&mut self, error: String) -> String {
        match self.rollback() {
            Ok(()) => format!("提交受管事务失败，已回滚: {}", error),
            Err(rollback_error) => format!(
                "提交受管事务失败: {}；回滚不完整: {}",
                error, rollback_error
            ),
        }
    }

    fn finish_commit(&mut self) -> Result<(), String> {
        if self.finalized {
            return Ok(());
        }
        let result = cleanup_committed_journal(&self.journal);
        if result.is_ok() {
            self.remove_journal()?;
            self.finalized = true;
        }
        result
    }

    fn remove_journal(&mut self) -> Result<(), String> {
        let Some(path) = self.journal_path.take() else {
            return Ok(());
        };
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                self.journal_path = Some(path);
                return Err(format!("无法删除文件事务日志: {}", error));
            }
        }
        if let Some(directory) = path.parent() {
            sync_directory(directory)?;
        }
        Ok(())
    }
}

fn prepare_transaction<'db>(
    removals: &[PathBuf],
    install_targets: &[PathBuf],
    metadata_checkpoint: Option<ManagedMetadataCheckpoint>,
    file_checkpoints: Vec<FileCheckpoint>,
    db: Option<&'db Connection>,
    journal_directory: &Path,
) -> Result<ReconcileTransaction<'db>, String> {
    let created_targets = unique_paths(
        install_targets
            .iter()
            .filter(|path| !path_exists(path))
            .cloned(),
    );
    let existing_removals = unique_paths(removals.iter().filter(|path| path_exists(path)).cloned());
    let transaction_id = transaction_id();
    let moved_targets = existing_removals
        .iter()
        .enumerate()
        .map(|(index, target)| (target.clone(), backup_path(target, index, &transaction_id)))
        .collect::<Vec<_>>();

    for (_, backup) in &moved_targets {
        if path_exists(backup) {
            return Err(format!("回滚暂存路径已存在: {}", backup.to_string_lossy()));
        }
    }

    let journal = ReconcileJournal {
        version: JOURNAL_VERSION,
        created_targets,
        moved_targets,
        metadata_checkpoint,
        file_checkpoints,
    };
    let journal_path = if journal.created_targets.is_empty()
        && journal.moved_targets.is_empty()
        && journal.file_checkpoints.is_empty()
    {
        None
    } else {
        Some(write_new_journal(
            journal_directory,
            &transaction_id,
            &journal,
        )?)
    };
    let mut transaction = ReconcileTransaction {
        journal,
        journal_path,
        state: TransactionState::Prepared,
        finalized: false,
        db,
    };

    for (target, backup) in transaction.journal.moved_targets.clone() {
        if let Err(error) = fs::rename(&target, &backup).and_then(|_| sync_parent(&target)) {
            let rollback_error = transaction.rollback().err();
            return Err(prepare_error(
                format!(
                    "无法暂存待移除 Skill {}: {}",
                    target.to_string_lossy(),
                    error
                ),
                rollback_error,
            ));
        }
    }
    Ok(transaction)
}

impl Drop for ReconcileTransaction<'_> {
    fn drop(&mut self) {
        if self.finalized {
            return;
        }
        match self.state {
            TransactionState::Prepared => {
                let _ = self.rollback();
            }
            TransactionState::Committed => {
                let _ = self.finish_commit();
            }
        }
    }
}

pub fn recover_pending_transactions(db: &Connection) -> Result<usize, String> {
    recover_pending_transactions_in(Some(db), &journal_directory())
}

fn recover_pending_transactions_in(
    db: Option<&Connection>,
    directory: &Path,
) -> Result<usize, String> {
    if !directory.exists() {
        return Ok(0);
    }
    let mut entries = Vec::new();
    for entry in
        fs::read_dir(directory).map_err(|error| format!("无法读取文件事务日志目录: {}", error))?
    {
        let path = entry
            .map_err(|error| format!("无法读取文件事务日志条目: {}", error))?
            .path();
        if is_journal_path(&path) {
            entries.push(path);
        }
    }
    entries.sort();

    let mut recovered = 0;
    let mut failures = Vec::new();
    for path in entries {
        let result = recover_journal_file(db, &path);
        match result {
            Ok(()) => recovered += 1,
            Err(error) => failures.push(format!("{}: {}", path.to_string_lossy(), error)),
        }
    }
    if failures.is_empty() {
        Ok(recovered)
    } else {
        Err(format!("文件事务恢复不完整: {}", failures.join("；")))
    }
}

fn is_journal_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    name.ends_with(PREPARED_SUFFIX) || name.ends_with(COMMITTED_SUFFIX)
}

fn recover_journal_file(db: Option<&Connection>, path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|error| format!("读取事务日志失败: {}", error))?;
    let journal: ReconcileJournal =
        serde_json::from_slice(&bytes).map_err(|error| format!("解析事务日志失败: {}", error))?;
    if journal.version == 0 || journal.version > JOURNAL_VERSION {
        return Err(format!(
            "不支持的事务日志版本 {}（当前支持 {}）",
            journal.version, JOURNAL_VERSION
        ));
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if name.ends_with(COMMITTED_SUFFIX) {
        cleanup_committed_journal(&journal)?;
    } else {
        rollback_prepared_journal(&journal, db)?;
    }
    fs::remove_file(path).map_err(|error| format!("删除已恢复事务日志失败: {}", error))?;
    if let Some(directory) = path.parent() {
        sync_directory(directory)?;
    }
    Ok(())
}

fn rollback_prepared_journal(
    journal: &ReconcileJournal,
    db: Option<&Connection>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    if let Err(error) = rollback_files(journal) {
        failures.push(error);
    }
    if let Some(checkpoint) = &journal.metadata_checkpoint {
        match db {
            Some(db) => {
                if let Err(error) = checkpoint.restore(db) {
                    failures.push(format!("恢复受管元数据失败: {}", error));
                }
            }
            None => failures.push("受管事务恢复缺少数据库连接".to_string()),
        }
    }
    for checkpoint in journal.file_checkpoints.iter().rev() {
        if let Err(error) = checkpoint.restore() {
            failures.push(error);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("；"))
    }
}

fn rollback_files(journal: &ReconcileJournal) -> Result<(), String> {
    let mut failures = Vec::new();
    for target in journal.created_targets.iter().rev() {
        if path_exists(target) {
            if let Err(error) = remove_path(target)
                .and_then(|_| sync_parent(target).map_err(|error| error.to_string()))
            {
                failures.push(format!("移除 {} 失败: {}", target.to_string_lossy(), error));
            }
        }
    }
    for (target, backup) in journal.moved_targets.iter().rev() {
        if !path_exists(backup) {
            continue;
        }
        if path_exists(target) {
            if let Err(error) = remove_path(target)
                .and_then(|_| sync_parent(target).map_err(|error| error.to_string()))
            {
                failures.push(format!("移除 {} 失败: {}", target.to_string_lossy(), error));
                continue;
            }
        }
        if let Err(error) = fs::rename(backup, target).and_then(|_| sync_parent(target)) {
            failures.push(format!("恢复 {} 失败: {}", target.to_string_lossy(), error));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("；"))
    }
}

fn sync_journal_commit_state(journal: &ReconcileJournal) -> Result<(), String> {
    let mut targets = journal.created_targets.clone();
    targets.extend(
        journal
            .moved_targets
            .iter()
            .map(|(target, _)| target.clone()),
    );
    for target in unique_paths(targets) {
        if path_exists(&target) {
            sync_path_tree(&target)?;
        }
        sync_parent(&target).map_err(|error| {
            format!(
                "无法同步受管目标目录 {}: {}",
                target.to_string_lossy(),
                error
            )
        })?;
    }
    if let Some(checkpoint) = &journal.metadata_checkpoint {
        for sidecar in checkpoint.sidecar_paths() {
            if path_exists(&sidecar) {
                sync_path_tree(&sidecar)?;
            }
            sync_parent(&sidecar).map_err(|error| {
                format!(
                    "无法同步受管状态目录 {}: {}",
                    sidecar.to_string_lossy(),
                    error
                )
            })?;
        }
    }
    for checkpoint in &journal.file_checkpoints {
        if path_exists(&checkpoint.path) {
            sync_path_tree(&checkpoint.path)?;
        }
        sync_existing_parent(&checkpoint.path).map_err(|error| {
            format!(
                "无法同步文件检查点目录 {}: {}",
                checkpoint.path.to_string_lossy(),
                error
            )
        })?;
    }
    Ok(())
}

fn cleanup_committed_journal(journal: &ReconcileJournal) -> Result<(), String> {
    let mut failures = Vec::new();
    for (_, backup) in &journal.moved_targets {
        if path_exists(backup) {
            if let Err(error) = remove_path(backup)
                .and_then(|_| sync_parent(backup).map_err(|error| error.to_string()))
            {
                failures.push(format!("{}: {}", backup.to_string_lossy(), error));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("清理回滚暂存失败: {}", failures.join("；")))
    }
}

fn write_new_journal(
    directory: &Path,
    transaction_id: &str,
    journal: &ReconcileJournal,
) -> Result<PathBuf, String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("无法创建文件事务日志目录: {}", error))?;
    let path = directory.join(format!("{}{}", transaction_id, PREPARED_SUFFIX));
    let temporary_path = directory.join(format!("{}.tmp", transaction_id));
    let bytes = serde_json::to_vec(journal).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .map_err(|error| format!("无法创建文件事务日志: {}", error))?;
        file.write_all(&bytes)
            .map_err(|error| format!("无法写入文件事务日志: {}", error))?;
        file.sync_all()
            .map_err(|error| format!("无法同步文件事务日志: {}", error))?;
        drop(file);
        fs::rename(&temporary_path, &path)
            .map_err(|error| format!("无法发布文件事务日志: {}", error))?;
        sync_directory(directory)?;
        Ok(path.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn prepare_error(error: String, rollback_error: Option<String>) -> String {
    match rollback_error {
        Some(rollback_error) => format!("{}；回滚不完整: {}", error, rollback_error),
        None => error,
    }
}

fn journal_directory() -> PathBuf {
    #[cfg(test)]
    {
        std::env::temp_dir().join(format!(
            "skillmate-reconcile-journals-{}",
            std::process::id()
        ))
    }
    #[cfg(not(test))]
    {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("skillmate")
            .join("reconcile-journals")
    }
}

fn transaction_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = TRANSACTION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}-{}", timestamp, std::process::id(), counter)
}

fn unique_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn capture_file_checkpoints(paths: &[PathBuf]) -> Result<Vec<FileCheckpoint>, String> {
    unique_paths(paths.iter().cloned())
        .iter()
        .map(|path| FileCheckpoint::capture(path))
        .collect()
}

fn backup_path(target: &Path, index: usize, transaction_id: &str) -> PathBuf {
    let name = target
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "skill".into());
    target.with_file_name(format!(
        ".{}.skillmate-reconcile-{}-{}",
        name, transaction_id, index
    ))
}

fn replace_journal_suffix(path: &Path, suffix: &str) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "文件事务日志名称无效".to_string())?;
    let transaction_id = name
        .strip_suffix(PREPARED_SUFFIX)
        .ok_or_else(|| "文件事务日志状态无效".to_string())?;
    Ok(path.with_file_name(format!("{}{}", transaction_id, suffix)))
}

fn path_exists(path: &Path) -> bool {
    path.exists() || fs::symlink_metadata(path).is_ok()
}

fn remove_path(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).map_err(|error| error.to_string())
    } else {
        fs::remove_dir_all(path).map_err(|error| error.to_string())
    }
}

#[cfg(unix)]
fn sync_path_tree(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("无法读取待同步路径 {}: {}", path.to_string_lossy(), error))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        return File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("无法同步文件 {}: {}", path.to_string_lossy(), error));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)
            .map_err(|error| format!("无法读取目录 {}: {}", path.to_string_lossy(), error))?
        {
            let child = entry
                .map_err(|error| format!("无法读取目录项: {}", error))?
                .path();
            sync_path_tree(&child)?;
        }
        return File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("无法同步目录 {}: {}", path.to_string_lossy(), error));
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_path_tree(_path: &Path) -> Result<(), String> {
    // Windows 标准库没有可靠的目录 fsync；journal 仍保证进程崩溃恢复，但不承诺断电持久性。
    Ok(())
}

fn sync_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) => sync_directory_io(parent),
        None => Ok(()),
    }
}

fn sync_existing_parent(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if path_exists(parent) {
        sync_directory(parent)
    } else {
        Ok(())
    }
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(test)]
    if DIRECTORY_SYNC_FAILURE.with(|failure| {
        let mut failure = failure.borrow_mut();
        let should_fail = failure.as_deref() == Some(path);
        if should_fail {
            failure.take();
        }
        should_fail
    }) {
        return Err("测试注入的目录同步失败".to_string());
    }
    sync_directory_io(path).map_err(|error| format!("同步目录失败: {}", error))
}

#[cfg(unix)]
fn sync_directory_io(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory_io(_path: &Path) -> std::io::Result<()> {
    // Windows 仅提供进程崩溃级恢复保证。
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_installation::{
        cleanup_skill_metadata, find_managed_installation, record_managed_root,
    };
    use crate::managed_state::{
        content_fingerprint, managed_state_entry, mark_managed_skill, STATE_FILE_NAME,
    };
    use rusqlite::params;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "skillmate-reconcile-{}-{}-{}",
            name,
            std::process::id(),
            transaction_id()
        ))
    }

    fn managed_database() -> Connection {
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

    fn seed_managed_skill(db: &Connection, root: &Path, skill: &Path) {
        fs::create_dir_all(skill).unwrap();
        fs::write(skill.join("SKILL.md"), "old content").unwrap();
        mark_managed_skill(root, "Codex", skill, "local:/tmp/writer").unwrap();
        record_managed_root(db, root, "global", None).unwrap();
        let key = skill.to_string_lossy().to_string();
        db.execute(
            "INSERT INTO skill_origin_meta VALUES (?, 'git', 'example/skills#main:writer',
             'https://github.com/example/skills.git', 'main', 'old-ref', 'old-ref', 'current',
             '已同步', 0, 1, 1, 1)",
            [&key],
        )
        .unwrap();
        db.execute(
            "INSERT INTO skill_tags (skill_path, tags, tags_json) VALUES (?, 'old', '[\"old\"]')",
            [&key],
        )
        .unwrap();
    }

    #[test]
    fn crash_during_delete_restores_files_database_and_sidecar() {
        let base = test_root("managed-delete-crash");
        let journals = base.join("journals");
        let root = base.join("skills");
        let skill = root.join("writer");
        let db = managed_database();
        seed_managed_skill(&db, &root, &skill);

        let transaction = ReconcileTransaction::prepare_managed_in(
            &db,
            std::slice::from_ref(&skill),
            &[],
            &journals,
        )
        .unwrap();
        cleanup_skill_metadata(&db, &skill).unwrap();
        std::mem::forget(transaction);

        assert!(!skill.exists());
        assert!(find_managed_installation(&db, &skill).unwrap().is_none());
        assert_eq!(
            recover_pending_transactions_in(Some(&db), &journals).unwrap(),
            1
        );
        assert_eq!(
            fs::read_to_string(skill.join("SKILL.md")).unwrap(),
            "old content"
        );
        assert!(find_managed_installation(&db, &skill).unwrap().is_some());
        assert!(managed_state_entry(&root, &skill).unwrap().is_some());
        let origin_ref: String = db
            .query_row(
                "SELECT installed_ref FROM skill_origin_meta WHERE skill_path = ?",
                [skill.to_string_lossy().to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(origin_ref, "old-ref");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn crash_during_update_restores_old_content_and_metadata() {
        let base = test_root("managed-update-crash");
        let journals = base.join("journals");
        let root = base.join("skills");
        let skill = root.join("writer");
        let profile_state = base.join("skill-profiles.json");
        let db = managed_database();
        seed_managed_skill(&db, &root, &skill);
        fs::write(&profile_state, "old profile").unwrap();
        let old_hash = content_fingerprint(&skill).unwrap();
        let root_key = root.to_string_lossy().to_string();
        let original_root: (String, Option<String>, String) = db
            .query_row(
                "SELECT scope, project_path, updated_at FROM managed_roots WHERE root_path = ?",
                [&root_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        let transaction = ReconcileTransaction::prepare_managed_with_files_in(
            &db,
            std::slice::from_ref(&skill),
            std::slice::from_ref(&skill),
            std::slice::from_ref(&profile_state),
            &journals,
        )
        .unwrap();
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "new content").unwrap();
        fs::write(&profile_state, "new profile").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "local:/tmp/writer").unwrap();
        record_managed_root(&db, &root, "project", Some("/tmp/new-project")).unwrap();
        db.execute(
            "UPDATE managed_roots SET updated_at = 'changed-after-checkpoint' WHERE root_path = ?",
            [&root_key],
        )
        .unwrap();
        db.execute(
            "UPDATE skill_origin_meta SET installed_ref = 'new-ref', latest_ref = 'new-ref'
             WHERE skill_path = ?",
            [skill.to_string_lossy().to_string()],
        )
        .unwrap();
        std::mem::forget(transaction);

        assert_eq!(
            recover_pending_transactions_in(Some(&db), &journals).unwrap(),
            1
        );
        assert_eq!(
            fs::read_to_string(skill.join("SKILL.md")).unwrap(),
            "old content"
        );
        let (installed_ref, stored_hash): (String, String) = db
            .query_row(
                "SELECT o.installed_ref, m.content_hash
                 FROM skill_origin_meta o JOIN managed_installations m ON m.skill_path = o.skill_path
                 WHERE o.skill_path = ?",
                params![skill.to_string_lossy().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(installed_ref, "old-ref");
        assert_eq!(stored_hash, old_hash);
        assert_eq!(
            managed_state_entry(&root, &skill)
                .unwrap()
                .unwrap()
                .last_seen_hash,
            old_hash
        );
        let restored_root: (String, Option<String>, String) = db
            .query_row(
                "SELECT scope, project_path, updated_at FROM managed_roots WHERE root_path = ?",
                [&root_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(restored_root, original_root);
        assert!(root.join(STATE_FILE_NAME).exists());
        assert_eq!(fs::read_to_string(&profile_state).unwrap(), "old profile");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn profile_only_prepared_transaction_is_persisted_and_restored() {
        let base = test_root("profile-only-prepared");
        let journals = base.join("journals");
        let profile_state = base.join("skill-profiles.json");
        fs::create_dir_all(&base).unwrap();
        fs::write(&profile_state, "old profile").unwrap();

        let transaction = ReconcileTransaction::prepare_in_with_files(
            &[],
            &[],
            std::slice::from_ref(&profile_state),
            &journals,
        )
        .unwrap();
        fs::write(&profile_state, "new profile").unwrap();
        assert!(fs::read_dir(&journals).unwrap().next().is_some());
        std::mem::forget(transaction);

        assert_eq!(recover_pending_transactions_in(None, &journals).unwrap(), 1);
        assert_eq!(fs::read_to_string(&profile_state).unwrap(), "old profile");
        assert!(fs::read_dir(&journals).unwrap().next().is_none());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn prepared_transaction_removes_file_created_after_absent_checkpoint() {
        let base = test_root("profile-created-after-checkpoint");
        let journals = base.join("journals");
        let profile_state = base.join("skill-profiles.json");
        fs::create_dir_all(&base).unwrap();

        let transaction = ReconcileTransaction::prepare_in_with_files(
            &[],
            &[],
            std::slice::from_ref(&profile_state),
            &journals,
        )
        .unwrap();
        fs::write(&profile_state, "new profile").unwrap();
        std::mem::forget(transaction);

        assert_eq!(recover_pending_transactions_in(None, &journals).unwrap(), 1);
        assert!(!profile_state.exists());
        assert!(fs::read_dir(&journals).unwrap().next().is_none());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn committed_profile_checkpoint_keeps_new_state() {
        let base = test_root("profile-committed");
        let journals = base.join("journals");
        let profile_state = base.join("skill-profiles.json");
        fs::create_dir_all(&base).unwrap();
        fs::write(&profile_state, "old profile").unwrap();

        let mut transaction = ReconcileTransaction::prepare_in_with_files(
            &[],
            &[],
            std::slice::from_ref(&profile_state),
            &journals,
        )
        .unwrap();
        fs::write(&profile_state, "new profile").unwrap();
        transaction.sync_commit_state().unwrap();
        assert_eq!(transaction.mark_committed().unwrap(), None);
        std::mem::forget(transaction);

        assert_eq!(recover_pending_transactions_in(None, &journals).unwrap(), 1);
        assert_eq!(fs::read_to_string(&profile_state).unwrap(), "new profile");
        assert!(fs::read_dir(&journals).unwrap().next().is_none());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn committed_marker_sync_failure_does_not_roll_back_new_state() {
        let base = test_root("committed-marker-sync-failure");
        let journals = base.join("journals");
        let skill = base.join("skills").join("writer");
        let profile_state = base.join("skill-profiles.json");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "old skill").unwrap();
        fs::write(&profile_state, "old profile").unwrap();

        let transaction = ReconcileTransaction::prepare_in_with_files(
            std::slice::from_ref(&skill),
            std::slice::from_ref(&skill),
            std::slice::from_ref(&profile_state),
            &journals,
        )
        .unwrap();
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "new skill").unwrap();
        fs::write(&profile_state, "new profile").unwrap();
        DIRECTORY_SYNC_FAILURE.with(|failure| *failure.borrow_mut() = Some(journals.clone()));

        let result = transaction.commit();
        let skill_contents = fs::read_to_string(skill.join("SKILL.md")).unwrap();
        let profile_contents = fs::read_to_string(&profile_state).unwrap();
        let journal_count = fs::read_dir(&journals).unwrap().count();
        let _ = fs::remove_dir_all(base);

        let warning = result.unwrap().unwrap();
        assert!(warning.contains("测试注入的目录同步失败"));
        assert_eq!(skill_contents, "new skill");
        assert_eq!(profile_contents, "new profile");
        assert_eq!(journal_count, 0);
    }

    #[test]
    fn prepared_recovery_can_retry_after_first_failure() {
        let base = test_root("prepared-recovery-retry");
        let journals = base.join("journals");
        let profile_state = base.join("skill-profiles.json");
        let db = managed_database();
        fs::create_dir_all(&base).unwrap();
        fs::write(&profile_state, "old profile").unwrap();

        let transaction = ReconcileTransaction::prepare_managed_with_files_in(
            &db,
            &[],
            &[],
            std::slice::from_ref(&profile_state),
            &journals,
        )
        .unwrap();
        fs::write(&profile_state, "new profile").unwrap();
        std::mem::forget(transaction);

        let first_error = recover_pending_transactions_in(None, &journals).unwrap_err();
        assert!(first_error.contains("受管事务恢复缺少数据库连接"));
        assert_eq!(fs::read_to_string(&profile_state).unwrap(), "old profile");
        assert_eq!(fs::read_dir(&journals).unwrap().count(), 1);

        assert_eq!(
            recover_pending_transactions_in(Some(&db), &journals).unwrap(),
            1
        );
        assert_eq!(fs::read_to_string(&profile_state).unwrap(), "old profile");
        assert!(fs::read_dir(&journals).unwrap().next().is_none());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn rollback_restores_removed_targets_and_deletes_new_targets() {
        let root = test_root("rollback");
        let journals = root.join("journals");
        let removed = root.join("removed");
        let created = root.join("created");
        fs::create_dir_all(&removed).unwrap();
        fs::write(removed.join("SKILL.md"), "old").unwrap();

        let mut transaction = ReconcileTransaction::prepare_in(
            std::slice::from_ref(&removed),
            std::slice::from_ref(&created),
            &journals,
        )
        .unwrap();
        fs::create_dir_all(&created).unwrap();
        fs::write(created.join("SKILL.md"), "new").unwrap();
        transaction.rollback().unwrap();

        assert_eq!(fs::read_to_string(removed.join("SKILL.md")).unwrap(), "old");
        assert!(!created.exists());
        assert!(fs::read_dir(&journals).unwrap().next().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn commit_removes_staged_targets() {
        let root = test_root("commit");
        let journals = root.join("journals");
        let removed = root.join("removed");
        fs::create_dir_all(&removed).unwrap();

        let transaction =
            ReconcileTransaction::prepare_in(std::slice::from_ref(&removed), &[], &journals)
                .unwrap();
        transaction.commit().unwrap();

        assert!(!removed.exists());
        assert!(fs::read_dir(&journals).unwrap().next().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovers_prepared_transaction_after_process_loss() {
        let root = test_root("recover-prepared");
        let journals = root.join("journals");
        let removed = root.join("removed");
        let created = root.join("created");
        fs::create_dir_all(&removed).unwrap();
        fs::write(removed.join("SKILL.md"), "old").unwrap();

        let transaction = ReconcileTransaction::prepare_in(
            std::slice::from_ref(&removed),
            std::slice::from_ref(&created),
            &journals,
        )
        .unwrap();
        fs::create_dir_all(&created).unwrap();
        fs::write(created.join("SKILL.md"), "new").unwrap();
        std::mem::forget(transaction);

        assert_eq!(recover_pending_transactions_in(None, &journals).unwrap(), 1);
        assert_eq!(fs::read_to_string(removed.join("SKILL.md")).unwrap(), "old");
        assert!(!created.exists());
        assert!(fs::read_dir(&journals).unwrap().next().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn committed_transaction_is_finished_instead_of_rolled_back() {
        let root = test_root("recover-committed");
        let journals = root.join("journals");
        let removed = root.join("removed");
        fs::create_dir_all(&removed).unwrap();
        fs::write(removed.join("SKILL.md"), "old").unwrap();

        let mut transaction =
            ReconcileTransaction::prepare_in(std::slice::from_ref(&removed), &[], &journals)
                .unwrap();
        assert_eq!(transaction.mark_committed().unwrap(), None);
        std::mem::forget(transaction);

        assert_eq!(recover_pending_transactions_in(None, &journals).unwrap(), 1);
        assert!(!removed.exists());
        assert!(fs::read_dir(&journals).unwrap().next().is_none());
        let _ = fs::remove_dir_all(root);
    }
}
