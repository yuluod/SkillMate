use super::*;

use crate::app_core::{atomic_write, generate_id};
use crate::managed_installation::list_managed_installations;
use crate::operation_plan::StableHash;
use rusqlite::Connection;
use serde::Serialize;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self};
use std::path::{Path, PathBuf};

pub(super) fn validate_existing_snapshot_root(repo: &Path) -> Result<(), String> {
    let snapshot_root = repo.join("assistants");
    read_managed_snapshot_marker(&snapshot_root, "备份仓库中的 assistants")?;
    Ok(())
}

pub(super) fn read_managed_snapshot_marker(path: &Path, label: &str) -> Result<Option<String>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取 {} 失败: {}", label, error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("{} 已存在但不是普通目录", label));
    }
    let marker = path.join(BACKUP_ROOT_MARKER);
    let marker_label = format!("{} 的 SkillMate 管理标记", label);
    let Some(payload) = read_bounded_regular_file(&marker, &marker_label, 1024)? else {
        return Err(format!("{} 不是 SkillMate 管理目录，已拒绝覆盖", label));
    };
    String::from_utf8(payload)
        .map(Some)
        .map_err(|_| format!("{} 不是 UTF-8 文本", marker_label))
}

pub(super) fn transaction_snapshot_marker(transaction_id: &str) -> String {
    format!(
        "Managed by SkillMate backup transaction {}. This directory may be replaced during backup sync.\n",
        transaction_id
    )
}

#[derive(Default)]
pub(super) struct BackupCopyBudget {
    pub(super) files: usize,
    pub(super) bytes: u64,
}

impl BackupCopyBudget {
    pub(super) fn visit_file(&mut self, bytes: u64) -> Result<(), String> {
        self.files = self.files.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
        if self.files > MAX_BACKUP_FILES || self.bytes > MAX_BACKUP_BYTES {
            Err(format!(
                "备份超过限制（最多 {} 个文件、{} MB）",
                MAX_BACKUP_FILES,
                MAX_BACKUP_BYTES / 1024 / 1024
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct BackupSource {
    pub(super) path: PathBuf,
    pub(super) assistants: BTreeSet<String>,
    pub(super) scopes: BTreeSet<String>,
    pub(super) projects: BTreeSet<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BackupCopyReport {
    pub(super) copied_files: usize,
    pub(super) copied_bytes: u64,
    pub(super) top_level_entries: usize,
    pub(super) exclusions: Vec<BackupExclusion>,
    pub(super) exclusions_truncated: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct BackupExclusion {
    pub(super) path: String,
    pub(super) reason: &'static str,
}

pub(super) struct TemporaryBackupDirectory {
    pub(super) path: PathBuf,
    pub(super) keep: bool,
}

impl Drop for TemporaryBackupDirectory {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub(super) fn snapshot_assistants(
    connection: &Connection,
    repo: &Path,
) -> Result<BackupSnapshotTransaction, String> {
    validate_existing_snapshot_root(repo)?;
    let snapshot_root = repo.join("assistants");
    let transaction_id = generate_id();
    let transaction_root = validated_backup_git_dir(repo)?
        .join(format!("{}{}", BACKUP_TRANSACTION_PREFIX, transaction_id));
    let staging_root = transaction_root.join("assistants");
    let backup_root = transaction_root.join("previous-assistants");
    fs::create_dir(&transaction_root).map_err(|error| error.to_string())?;
    let mut temporary_directory = TemporaryBackupDirectory {
        path: transaction_root.clone(),
        keep: false,
    };
    write_backup_transaction_owner(&transaction_root, &transaction_id)?;
    fs::create_dir(&staging_root).map_err(|error| error.to_string())?;
    fs::write(
        staging_root.join(BACKUP_ROOT_MARKER),
        transaction_snapshot_marker(&transaction_id),
    )
    .map_err(|error| error.to_string())?;

    let sources = collect_backup_sources(connection)?;
    let mut manifest = Vec::new();
    let mut budget = BackupCopyBudget::default();
    for source in sources.values() {
        let root_id = backup_root_id(&source.path);
        let target_root = staging_root.join("roots").join(&root_id);
        let mut report = BackupCopyReport::default();
        let copied = snapshot_backup_source(source, &target_root, &mut budget, &mut report)?;
        manifest.push(serde_json::json!({
            "sourcePath": display_backup_path(&source.path),
            "snapshotPath": format!("assistants/roots/{}", root_id),
            "exists": source.path.exists(),
            "copied": copied,
            "assistants": &source.assistants,
            "scopes": &source.scopes,
            "projects": &source.projects,
            "report": report,
        }));
    }
    let payload = serde_json::json!({
        "version": 3,
        "kind": "managed-skill-content-snapshot",
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "roots": manifest,
        "limitations": [
            "不包含 SkillMate 数据库、标签、场景或 Profile",
            "不跟随或复制软连接",
            "仅备份 SkillMate 明确登记的受管 Skill",
            "无法完成敏感内容扫描的非核心文件会被排除；核心 SKILL.md 会中止同步",
            "尽力排除常见凭据、密钥、运行时缓存和受管 sidecar"
        ],
    });
    let manifest_path = repo.join("skillmate-backup.json");
    let previous_manifest = read_bounded_regular_file(
        &manifest_path,
        "现有备份 manifest",
        MAX_BACKUP_MANIFEST_BYTES,
    )?;
    let manifest_payload =
        serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;

    let previous_snapshot_marker =
        read_managed_snapshot_marker(&snapshot_root, "备份仓库中的 assistants")?;
    let had_snapshot = previous_snapshot_marker.is_some();
    let previous_manifest_len = previous_manifest
        .as_ref()
        .map(|content| content.len() as u64);
    let previous_manifest_sha256 = previous_manifest.as_ref().map(|content| {
        let mut hash = StableHash::new();
        hash.update(content);
        hash.finish()
    });
    let mut journal = BackupSnapshotJournal {
        version: BACKUP_JOURNAL_VERSION,
        generation: 0,
        state: BackupSnapshotState::Prepared,
        transaction_id,
        baseline_branch: Some(current_git_branch(repo)?),
        baseline_head: current_git_head(repo)?,
        expected_tree: None,
        expected_commit: None,
        previous_snapshot_marker,
        previous_manifest_len,
        previous_manifest_sha256,
        had_snapshot,
        had_manifest: previous_manifest.is_some(),
    };
    if let Some(content) = &previous_manifest {
        atomic_write(
            &transaction_root.join(BACKUP_PREVIOUS_MANIFEST_FILE),
            content,
        )?;
    }
    update_backup_snapshot_journal(&transaction_root, &mut journal, |_| {})?;
    temporary_directory.keep = true;
    let transaction = BackupSnapshotTransaction {
        repo: repo.to_path_buf(),
        transaction_root,
        journal,
        finished: false,
    };

    if had_snapshot {
        fs::rename(&snapshot_root, &backup_root).map_err(|error| error.to_string())?;
    }
    fs::rename(&staging_root, &snapshot_root)
        .map_err(|error| format!("无法启用新备份快照: {}", error))?;
    atomic_write(&manifest_path, manifest_payload.as_bytes())?;
    Ok(transaction)
}

pub(super) fn snapshot_backup_source(
    source: &BackupSource,
    target: &Path,
    budget: &mut BackupCopyBudget,
    report: &mut BackupCopyReport,
) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(&source.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() {
        record_backup_exclusion(report, &source.path, &source.path, "symlink");
        return Ok(false);
    }
    if !metadata.is_dir() {
        return Err(format!(
            "受管 Skill 不是目录: {}",
            source.path.to_string_lossy()
        ));
    }
    copy_backup_tree(&source.path, target, &source.path, 0, budget, report)?;
    Ok(true)
}

pub(super) fn copy_backup_tree(
    source: &Path,
    target: &Path,
    source_root: &Path,
    depth: usize,
    budget: &mut BackupCopyBudget,
    report: &mut BackupCopyReport,
) -> Result<(), String> {
    if depth > MAX_BACKUP_DEPTH {
        return Err(format!("备份目录层级超过 {} 层", MAX_BACKUP_DEPTH));
    }
    fs::create_dir_all(target).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        let source_path = entry.path();
        if let Some(reason) = backup_exclusion_reason(&name_text) {
            record_backup_exclusion(report, source_root, &source_path, reason);
            continue;
        }
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            record_backup_exclusion(report, source_root, &source_path, "symlink");
            continue;
        }
        let target_path = target.join(name);
        if depth == 0 {
            report.top_level_entries += 1;
        }
        if metadata.is_dir() {
            copy_backup_tree(
                &source_path,
                &target_path,
                source_root,
                depth + 1,
                budget,
                report,
            )?;
        } else if metadata.is_file() {
            let scanned = read_scanned_backup_file(&source_path, &metadata)?;
            budget.visit_file(scanned.file_size)?;
            match scanned.classification {
                SensitiveScan::Safe => {}
                SensitiveScan::Sensitive => {
                    if is_core_skill_file(source_root, &source_path) {
                        return Err(format!(
                            "核心 Skill 文件包含疑似敏感内容，已中止备份: {}",
                            display_backup_path(&source_path)
                        ));
                    }
                    record_backup_exclusion(report, source_root, &source_path, "sensitive_content");
                    continue;
                }
                SensitiveScan::Unscannable(reason) => {
                    if is_core_skill_file(source_root, &source_path) {
                        return Err(format!(
                            "核心 Skill 文件无法安全扫描（{}），已中止备份: {}",
                            unscannable_description(reason),
                            display_backup_path(&source_path)
                        ));
                    }
                    record_backup_exclusion(report, source_root, &source_path, reason);
                    continue;
                }
            }
            write_scanned_backup_file(&target_path, &scanned)?;
            report.copied_files += 1;
            report.copied_bytes = report
                .copied_bytes
                .saturating_add(scanned.bytes.len() as u64);
        } else {
            record_backup_exclusion(report, source_root, &source_path, "special_file");
        }
    }
    Ok(())
}

pub(super) fn collect_backup_sources(
    connection: &Connection,
) -> Result<BTreeMap<PathBuf, BackupSource>, String> {
    let mut sources = BTreeMap::new();
    for installation in list_managed_installations(connection)? {
        add_backup_source(
            &mut sources,
            installation.path,
            &installation.skill.assistant,
            installation.skill.scope.as_deref().unwrap_or("global"),
            installation.skill.project_path.as_deref(),
        )?;
    }
    Ok(sources)
}

pub(super) fn add_backup_source(
    sources: &mut BTreeMap<PathBuf, BackupSource>,
    path: PathBuf,
    assistant: &str,
    scope: &str,
    project: Option<&str>,
) -> Result<(), String> {
    let identity = canonicalize_for_comparison(&path)?;
    let candidate_is_symlink = fs::symlink_metadata(&path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    let source = sources.entry(identity).or_insert_with(|| BackupSource {
        path: path.clone(),
        ..BackupSource::default()
    });
    let current_is_symlink = fs::symlink_metadata(&source.path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    if current_is_symlink && !candidate_is_symlink {
        source.path = path;
    }
    source.assistants.insert(assistant.to_string());
    source.scopes.insert(scope.to_string());
    if let Some(project) = project.filter(|value| !value.trim().is_empty()) {
        source
            .projects
            .insert(display_backup_path(Path::new(project)));
    }
    Ok(())
}

pub(super) fn backup_root_id(path: &Path) -> String {
    let mut hash = StableHash::new();
    hash.update(path.to_string_lossy().as_bytes());
    let digest = hash.finish();
    format!("root-{}", &digest[..12])
}

pub(super) fn display_backup_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(home) {
            return Path::new("~").join(relative).to_string_lossy().to_string();
        }
    }
    path.to_string_lossy().to_string()
}

pub(super) fn record_backup_exclusion(
    report: &mut BackupCopyReport,
    source_root: &Path,
    path: &Path,
    reason: &'static str,
) {
    if report.exclusions.len() >= MAX_BACKUP_EXCLUSIONS {
        report.exclusions_truncated = true;
        return;
    }
    let relative = path.strip_prefix(source_root).unwrap_or(path);
    let display_path = if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative.to_string_lossy().to_string()
    };
    report.exclusions.push(BackupExclusion {
        path: display_path,
        reason,
    });
}
