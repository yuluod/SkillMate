use super::*;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self};
use std::path::Path;
use std::time::Duration;

pub(super) fn scan_final_backup_paths(repo: &Path) -> Result<BTreeSet<Vec<u8>>, String> {
    let mut paths = BTreeSet::new();
    let mut files = 0usize;
    let mut bytes = 0u64;
    for relative in BACKUP_SNAPSHOT_PATHS {
        scan_final_backup_path(
            &repo.join(relative),
            Path::new(relative),
            0,
            &mut files,
            &mut bytes,
            &mut paths,
        )?;
    }
    Ok(paths)
}

pub(super) fn scan_final_backup_path(
    path: &Path,
    relative: &Path,
    depth: usize,
    files: &mut usize,
    bytes: &mut u64,
    paths: &mut BTreeSet<Vec<u8>>,
) -> Result<(), String> {
    if depth > MAX_BACKUP_DEPTH + 3 {
        return Err(format!("最终备份目录层级超过 {} 层", MAX_BACKUP_DEPTH));
    }
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if let Some(reason) = backup_exclusion_reason(name) {
            return Err(format!(
                "最终备份内容出现禁止路径（{}），已拒绝提交: {}",
                reason,
                path.to_string_lossy()
            ));
        }
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("读取最终备份内容失败 {}: {}", path.to_string_lossy(), error))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "最终备份内容包含软连接，已拒绝提交: {}",
            path.to_string_lossy()
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            scan_final_backup_path(
                &entry.path(),
                &relative.join(entry.file_name()),
                depth + 1,
                files,
                bytes,
                paths,
            )?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(format!(
            "最终备份内容包含特殊文件，已拒绝提交: {}",
            path.to_string_lossy()
        ));
    }

    let scan_limit = if relative == Path::new(BACKUP_SNAPSHOT_PATHS[1]) {
        MAX_BACKUP_MANIFEST_BYTES
    } else {
        MAX_SENSITIVE_SCAN_BYTES
    };
    let scanned = read_scanned_backup_file_with_limit(path, &metadata, scan_limit)?;
    match scanned.classification {
        SensitiveScan::Safe => {}
        SensitiveScan::Sensitive => {
            return Err(format!(
                "最终 Git tree 包含疑似敏感内容，已拒绝提交: {}",
                path.to_string_lossy()
            ));
        }
        SensitiveScan::Unscannable(reason) => {
            return Err(format!(
                "最终 Git tree 包含无法安全扫描的文件（{}），已拒绝提交: {}",
                unscannable_description(reason),
                path.to_string_lossy()
            ));
        }
    }
    *files = files.saturating_add(1);
    *bytes = bytes.saturating_add(scanned.bytes.len() as u64);
    if *files > MAX_BACKUP_FILES + BACKUP_SNAPSHOT_PATHS.len()
        || *bytes > MAX_BACKUP_BYTES + MAX_BACKUP_MANIFEST_BYTES + MAX_SENSITIVE_SCAN_BYTES
    {
        return Err("最终 Git tree 超过备份安全上限".to_string());
    }
    let key = git_path_bytes(relative)?;
    if !paths.insert(key) {
        return Err("最终 Git tree 包含重复路径".to_string());
    }
    Ok(())
}

pub(super) fn git_path_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    for component in path.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(format!("Git 相对路径无效: {}", path.to_string_lossy()));
        };
        if !result.is_empty() {
            result.push(b'/');
        }
        result.extend(os_string_bytes(part)?);
    }
    Ok(result)
}

#[cfg(unix)]
pub(super) fn os_string_bytes(value: &std::ffi::OsStr) -> Result<&[u8], String> {
    use std::os::unix::ffi::OsStrExt;
    Ok(value.as_bytes())
}

#[cfg(not(unix))]
pub(super) fn os_string_bytes(value: &std::ffi::OsStr) -> Result<&[u8], String> {
    value
        .to_str()
        .map(str::as_bytes)
        .ok_or_else(|| "Git 路径不是有效的 Unicode".to_string())
}

pub(super) fn validate_backup_tree_scope(
    repo: &Path,
    journal: &BackupSnapshotJournal,
    expected_tree: &str,
) -> Result<(), String> {
    let output = if let Some(head) = journal.baseline_head.as_deref() {
        run_git(
            repo,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "--no-renames",
                "-r",
                "-z",
                head,
                expected_tree,
            ],
            Duration::from_secs(10),
        )?
    } else {
        run_git(
            repo,
            &["ls-tree", "-r", "-z", "--name-only", expected_tree],
            Duration::from_secs(10),
        )?
    };
    if !output.status.success() {
        return Err(format!(
            "检查备份 Git tree 范围失败: {}",
            command_output(&output)
        ));
    }
    if let Some(path) = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .find(|path| !is_backup_snapshot_path(path))
    {
        return Err(format!(
            "Git tree 包含非 SkillMate 备份变更: {}",
            String::from_utf8_lossy(path)
        ));
    }
    Ok(())
}

pub(super) fn validate_backup_tree_blobs(
    repo: &Path,
    expected_tree: &str,
    expected: &BTreeSet<Vec<u8>>,
) -> Result<(), String> {
    let output = run_git(
        repo,
        &[
            "ls-tree",
            "-r",
            "-z",
            "--full-tree",
            expected_tree,
            "--",
            BACKUP_SNAPSHOT_PATHS[0],
            BACKUP_SNAPSHOT_PATHS[1],
        ],
        Duration::from_secs(10),
    )?;
    if !output.status.success() {
        return Err(format!(
            "读取最终备份 Git tree 失败: {}",
            command_output(&output)
        ));
    }

    let mut actual = BTreeMap::new();
    for entry in output.stdout.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let tab = entry
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| "最终备份 Git tree 条目格式无效".to_string())?;
        let metadata = std::str::from_utf8(&entry[..tab])
            .map_err(|_| "最终备份 Git tree 元数据不是 UTF-8".to_string())?;
        let mut fields = metadata.split_whitespace();
        let mode = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        let oid = fields.next().unwrap_or_default();
        if fields.next().is_some()
            || kind != "blob"
            || !matches!(mode, "100644" | "100755")
            || oid.is_empty()
        {
            return Err(format!(
                "最终备份 Git tree 包含不安全条目: {}",
                String::from_utf8_lossy(&entry[tab + 1..])
            ));
        }
        if actual
            .insert(entry[tab + 1..].to_vec(), oid.to_string())
            .is_some()
        {
            return Err("最终备份 Git tree 包含重复路径".to_string());
        }
    }

    if actual.len() != expected.len() {
        return Err("最终 Git tree 与已扫描备份文件数量不一致".to_string());
    }
    for path in expected {
        if !actual.contains_key(path) {
            return Err(format!(
                "最终 Git tree 缺少已扫描文件: {}",
                String::from_utf8_lossy(path)
            ));
        }
    }
    scan_immutable_git_blobs(repo, &actual)
}

#[derive(Debug)]
pub(super) struct ImmutableGitBlob {
    pub(super) oid: String,
    pub(super) size: u64,
    pub(super) sample_path: Vec<u8>,
}

pub(super) fn scan_immutable_git_blobs(
    repo: &Path,
    entries: &BTreeMap<Vec<u8>, String>,
) -> Result<(), String> {
    let mut paths_by_oid = BTreeMap::<String, Vec<&[u8]>>::new();
    for (path, oid) in entries {
        paths_by_oid.entry(oid.clone()).or_default().push(path);
    }
    let object_ids = paths_by_oid.keys().cloned().collect::<Vec<_>>();
    let input = object_ids.join("\n") + "\n";
    let output = run_git_with_input(
        repo,
        &[
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ],
        input.as_bytes(),
        Duration::from_secs(30),
    )?;
    if !output.status.success() {
        return Err(format!(
            "读取最终 Git blob 元数据失败: {}",
            command_output(&output)
        ));
    }
    let lines = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.len() != object_ids.len() {
        return Err("最终 Git blob 元数据数量不一致".to_string());
    }

    let mut blobs = Vec::with_capacity(object_ids.len());
    let mut total_bytes = 0u64;
    let mut size_by_oid = BTreeMap::new();
    for (expected_oid, line) in object_ids.iter().zip(lines) {
        let line =
            std::str::from_utf8(line).map_err(|_| "最终 Git blob 元数据不是 UTF-8".to_string())?;
        let mut fields = line.split_whitespace();
        let oid = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        let size = fields
            .next()
            .unwrap_or_default()
            .parse::<u64>()
            .map_err(|_| "最终 Git blob 大小无效".to_string())?;
        if fields.next().is_some() || oid != expected_oid || kind != "blob" {
            return Err("最终 Git blob 元数据与 tree 不一致".to_string());
        }
        let paths = paths_by_oid
            .get(expected_oid)
            .ok_or_else(|| "最终 Git blob 缺少路径".to_string())?;
        let max_size = paths
            .iter()
            .map(|path| {
                if *path == BACKUP_SNAPSHOT_PATHS[1].as_bytes() {
                    MAX_BACKUP_MANIFEST_BYTES
                } else {
                    MAX_SENSITIVE_SCAN_BYTES
                }
            })
            .min()
            .unwrap_or(MAX_SENSITIVE_SCAN_BYTES);
        if size > max_size {
            return Err(format!(
                "最终 Git tree 文件超过安全扫描上限: {}",
                String::from_utf8_lossy(paths[0])
            ));
        }
        size_by_oid.insert(expected_oid.clone(), size);
        blobs.push(ImmutableGitBlob {
            oid: expected_oid.clone(),
            size,
            sample_path: paths[0].to_vec(),
        });
    }
    for oid in entries.values() {
        total_bytes = total_bytes.saturating_add(*size_by_oid.get(oid).unwrap_or(&u64::MAX));
    }
    if total_bytes > MAX_BACKUP_BYTES + MAX_BACKUP_MANIFEST_BYTES + MAX_SENSITIVE_SCAN_BYTES {
        return Err("最终 Git tree 超过备份安全上限".to_string());
    }

    for chunk in git_blob_chunks(&blobs) {
        scan_immutable_git_blob_chunk(repo, chunk)?;
    }
    Ok(())
}

pub(super) fn git_blob_chunks(blobs: &[ImmutableGitBlob]) -> Vec<&[ImmutableGitBlob]> {
    const MAX_BATCH_BYTES: u64 = 16 * 1024 * 1024;
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < blobs.len() {
        let mut end = start;
        let mut bytes = 0u64;
        while end < blobs.len() && (end == start || bytes + blobs[end].size <= MAX_BATCH_BYTES) {
            bytes = bytes.saturating_add(blobs[end].size);
            end += 1;
        }
        chunks.push(&blobs[start..end]);
        start = end;
    }
    chunks
}

pub(super) fn scan_immutable_git_blob_chunk(
    repo: &Path,
    blobs: &[ImmutableGitBlob],
) -> Result<(), String> {
    let input = blobs
        .iter()
        .map(|blob| blob.oid.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let output = run_git_with_input(
        repo,
        &["cat-file", "--batch"],
        input.as_bytes(),
        Duration::from_secs(120),
    )?;
    if !output.status.success() {
        return Err(format!(
            "读取最终 Git blob 失败: {}",
            command_output(&output)
        ));
    }

    let mut cursor = 0usize;
    for blob in blobs {
        let header_end = output.stdout[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| cursor + offset)
            .ok_or_else(|| "最终 Git blob 响应缺少头部".to_string())?;
        let header = std::str::from_utf8(&output.stdout[cursor..header_end])
            .map_err(|_| "最终 Git blob 头部不是 UTF-8".to_string())?;
        let mut fields = header.split_whitespace();
        let oid = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        let size = fields
            .next()
            .unwrap_or_default()
            .parse::<u64>()
            .map_err(|_| "最终 Git blob 响应大小无效".to_string())?;
        if fields.next().is_some() || oid != blob.oid || kind != "blob" || size != blob.size {
            return Err("最终 Git blob 响应与 tree 不一致".to_string());
        }
        let content_start = header_end + 1;
        let content_end = content_start
            .checked_add(size as usize)
            .ok_or_else(|| "最终 Git blob 大小溢出".to_string())?;
        if output.stdout.get(content_end) != Some(&b'\n') {
            return Err("最终 Git blob 响应不完整".to_string());
        }
        let classification = classify_backup_bytes(&output.stdout[content_start..content_end]);
        match classification {
            SensitiveScan::Safe => {}
            SensitiveScan::Sensitive => {
                return Err(format!(
                    "最终 Git tree 包含疑似敏感内容，已拒绝提交: {}",
                    String::from_utf8_lossy(&blob.sample_path)
                ));
            }
            SensitiveScan::Unscannable(reason) => {
                return Err(format!(
                    "最终 Git tree 包含无法安全扫描的文件（{}），已拒绝提交: {}",
                    unscannable_description(reason),
                    String::from_utf8_lossy(&blob.sample_path)
                ));
            }
        }
        cursor = content_end + 1;
    }
    if cursor != output.stdout.len() {
        return Err("最终 Git blob 响应包含额外数据".to_string());
    }
    Ok(())
}

pub(super) fn prepared_backup_was_committed(
    repo: &Path,
    journal: &BackupSnapshotJournal,
) -> Result<bool, String> {
    let current_head = current_git_head(repo)?;
    if current_head == journal.baseline_head {
        return Ok(false);
    }
    if current_head.is_none() {
        return Err(
            "备份事务期间 Git HEAD 从已有提交变为 unborn，已保留现场并拒绝自动回滚".to_string(),
        );
    }
    let current_head = current_head.expect("已在上方排除 unborn HEAD");
    let expected_commit = journal.expected_commit.as_deref().ok_or_else(|| {
        "备份事务期间 Git HEAD 已变化，但日志缺少精确 commit；已保留现场并拒绝自动回滚".to_string()
    })?;
    if current_head != expected_commit {
        return Err(
            "备份事务期间 Git HEAD 已变化但不是本事务提交；已保留现场并拒绝自动回滚".to_string(),
        );
    }
    validate_recovered_backup_commit(repo, &current_head, journal)?;
    Ok(true)
}

pub(super) fn validate_recovered_backup_commit(
    repo: &Path,
    commit: &str,
    journal: &BackupSnapshotJournal,
) -> Result<(), String> {
    let expected_tree = journal.expected_tree.as_deref().ok_or_else(|| {
        "备份事务期间 Git HEAD 已变化，但日志缺少预期 tree；已拒绝自动回滚".to_string()
    })?;
    if git_tree(repo, commit)? != expected_tree {
        return Err("备份事务提交的 tree 与日志不一致；已保留现场并拒绝自动回滚".to_string());
    }
    let expected_parents = journal.baseline_head.iter().cloned().collect::<Vec<_>>();
    if git_commit_parents(repo, commit)? != expected_parents {
        return Err("备份事务提交的 parent 与基线不一致；已保留现场并拒绝自动回滚".to_string());
    }
    Ok(())
}

pub(super) fn git_commit_parents(repo: &Path, commit: &str) -> Result<Vec<String>, String> {
    let output = run_git(
        repo,
        &["rev-list", "--parents", "--max-count=1", commit],
        Duration::from_secs(10),
    )?;
    if !output.status.success() {
        return Err(format!(
            "读取备份提交 parent 失败: {}",
            command_output(&output)
        ));
    }
    let mut fields = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if fields.first().map(String::as_str) != Some(commit) {
        return Err("读取备份提交 parent 时返回了意外 commit".to_string());
    }
    fields.remove(0);
    Ok(fields)
}
