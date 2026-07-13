use crate::app_core::{assistant_definitions, atomic_write};
use crate::operation_plan::StableHash;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const STATE_FILE_NAME: &str = ".skillmate-state.json";
const STATE_SCHEMA_VERSION: u32 = 2;
const MAX_FINGERPRINT_FILES: usize = 10_000;
const MAX_FINGERPRINT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FINGERPRINT_DEPTH: usize = 32;

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillMateState {
    pub version: u32,
    pub managed_skills: Vec<ManagedSkillState>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct ManagedSkillState {
    pub assistant: String,
    pub path: String,
    pub origin: String,
    pub installed_at: String,
    pub last_seen_hash: String,
}

#[derive(Debug, Clone)]
pub struct ManagedStateCheckpoint {
    state: SkillMateState,
    state_file_existed: bool,
}

impl ManagedStateCheckpoint {
    pub fn capture(root: &Path) -> Result<Self, String> {
        Ok(Self {
            state: read_managed_state(root)?,
            state_file_existed: root.join(STATE_FILE_NAME).exists(),
        })
    }

    #[cfg(test)]
    pub fn new_managed_paths(&self, root: &Path) -> Result<Vec<PathBuf>, String> {
        let previous = self
            .state
            .managed_skills
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<std::collections::HashSet<_>>();
        Ok(read_managed_state(root)?
            .managed_skills
            .into_iter()
            .filter(|entry| !previous.contains(entry.path.as_str()))
            .map(|entry| PathBuf::from(entry.path))
            .collect())
    }

    pub fn restore(&self, root: &Path) -> Result<(), String> {
        if self.state_file_existed {
            write_managed_state(root, &self.state)
        } else {
            let state_path = root.join(STATE_FILE_NAME);
            match fs::remove_file(&state_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!(
                    "无法移除回滚后的受管状态 {}: {}",
                    state_path.to_string_lossy(),
                    error
                )),
            }
        }
    }
}

pub fn read_managed_state(root: &Path) -> Result<SkillMateState, String> {
    let path = root.join(STATE_FILE_NAME);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SkillMateState {
                version: STATE_SCHEMA_VERSION,
                managed_skills: vec![],
            })
        }
        Err(error) => {
            return Err(format!(
                "无法读取受管状态 {}: {}",
                path.to_string_lossy(),
                error
            ))
        }
    };
    let mut state: SkillMateState = serde_json::from_str(&content)
        .map_err(|error| format!("受管状态文件损坏 {}: {}", path.to_string_lossy(), error))?;
    if state.version > STATE_SCHEMA_VERSION {
        return Err(format!(
            "受管状态版本 {} 高于当前支持版本 {}",
            state.version, STATE_SCHEMA_VERSION
        ));
    }
    let normalized = validate_and_normalize_state(root, &mut state)?;
    let migrated = state.version < STATE_SCHEMA_VERSION;
    if migrated {
        migrate_state_fingerprints(&mut state);
        state.version = STATE_SCHEMA_VERSION;
    }
    if normalized || migrated {
        write_managed_state(root, &state)?;
    }
    Ok(state)
}

pub fn mark_managed_skill(
    root: &Path,
    assistant: &str,
    target_path: &Path,
    origin: &str,
) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut state = read_managed_state(root)?;
    state.version = STATE_SCHEMA_VERSION;
    let target = normalize_managed_entry_path(root, target_path)?
        .to_string_lossy()
        .to_string();
    let hash = content_fingerprint(target_path)?;
    let installed_at = chrono::Utc::now().to_rfc3339();
    if let Some(existing) = state
        .managed_skills
        .iter_mut()
        .find(|item| item.path == target)
    {
        existing.assistant = assistant.to_string();
        existing.origin = origin.to_string();
        existing.last_seen_hash = hash;
        if existing.installed_at.is_empty() {
            existing.installed_at = installed_at;
        }
    } else {
        state.managed_skills.push(ManagedSkillState {
            assistant: assistant.to_string(),
            path: target,
            origin: origin.to_string(),
            installed_at,
            last_seen_hash: hash,
        });
    }
    write_managed_state(root, &state)
}

pub fn is_managed_by_state(root: &Path, target_path: &Path) -> Result<bool, String> {
    Ok(managed_state_entry(root, target_path)?.is_some())
}

pub fn managed_state_entry(
    root: &Path,
    target_path: &Path,
) -> Result<Option<ManagedSkillState>, String> {
    let target = normalize_managed_entry_path(root, target_path)?
        .to_string_lossy()
        .to_string();
    Ok(read_managed_state(root)?
        .managed_skills
        .into_iter()
        .find(|item| item.path == target))
}

pub fn managed_state_origin(root: &Path, target_path: &Path) -> Result<Option<String>, String> {
    Ok(managed_state_entry(root, target_path)?.map(|entry| entry.origin))
}

pub fn unmark_managed_skill(root: &Path, target_path: &Path) -> Result<(), String> {
    let mut state = read_managed_state(root)?;
    let target = normalize_managed_entry_path(root, target_path)?;
    let target = target.to_string_lossy();
    let original_len = state.managed_skills.len();
    state
        .managed_skills
        .retain(|item| item.path.as_str() != target.as_ref());
    if state.managed_skills.len() == original_len {
        return Ok(());
    }
    write_managed_state(root, &state)
}

pub fn refresh_managed_skill_fingerprint(root: &Path, target_path: &Path) -> Result<bool, String> {
    let mut state = read_managed_state(root)?;
    let target = normalize_managed_entry_path(root, target_path)?;
    let target = target.to_string_lossy();
    let Some(entry) = state
        .managed_skills
        .iter_mut()
        .find(|entry| entry.path.as_str() == target.as_ref())
    else {
        return Ok(false);
    };
    entry.last_seen_hash = content_fingerprint(target_path)?;
    write_managed_state(root, &state)?;
    Ok(true)
}

fn write_managed_state(root: &Path, state: &SkillMateState) -> Result<(), String> {
    let path = root.join(STATE_FILE_NAME);
    let content = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    atomic_write(&path, content.as_bytes())
}

fn validate_and_normalize_state(root: &Path, state: &mut SkillMateState) -> Result<bool, String> {
    let supported_assistants = assistant_definitions()
        .iter()
        .map(|assistant| assistant.name)
        .collect::<std::collections::HashSet<_>>();
    let mut seen = std::collections::HashSet::new();
    let mut changed = false;
    for entry in &mut state.managed_skills {
        if !supported_assistants.contains(entry.assistant.as_str()) {
            return Err(format!("受管状态包含不支持的助手: {}", entry.assistant));
        }
        let normalized = normalize_managed_entry_path(root, Path::new(&entry.path))?;
        let normalized_text = normalized.to_string_lossy().to_string();
        if !seen.insert(normalized_text.clone()) {
            return Err(format!("受管状态包含重复路径: {}", normalized_text));
        }
        if entry.path != normalized_text {
            entry.path = normalized_text;
            changed = true;
        }
    }
    Ok(changed)
}

fn normalize_managed_entry_path(root: &Path, target_path: &Path) -> Result<PathBuf, String> {
    let logical_root = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("无法解析当前目录: {}", error))?
            .join(root)
    };
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("无法解析受管根目录 {}: {}", root.to_string_lossy(), error))?;
    let parent = target_path
        .parent()
        .ok_or_else(|| "受管 Skill 路径缺少父目录".to_string())?;
    let canonical_parent = parent.canonicalize().map_err(|error| {
        format!(
            "无法解析受管 Skill 父目录 {}: {}",
            parent.to_string_lossy(),
            error
        )
    })?;
    if canonical_parent != canonical_root {
        return Err(format!(
            "受管 Skill 必须是根目录的直接子项: {}",
            target_path.to_string_lossy()
        ));
    }
    let name = target_path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "受管 Skill 路径缺少目录名".to_string())?;
    Ok(logical_root.join(name))
}

pub fn content_fingerprint(path: &Path) -> Result<String, String> {
    let mut hash = StableHash::new();
    let mut budget = FingerprintBudget::default();
    collect_fingerprint(path, path, 0, &mut budget, &mut |bytes| hash.update(bytes))?;
    Ok(format!("sha256:{}", hash.finish()))
}

pub fn fingerprint_matches(path: &Path, expected: &str) -> Result<bool, String> {
    if expected.starts_with("sha256:") {
        return Ok(content_fingerprint(path)? == expected);
    }
    if expected.starts_with("fnv1a64:") {
        return Ok(legacy_content_fingerprint(path)? == expected);
    }
    Err("无法识别受管内容指纹格式".to_string())
}

#[derive(Default)]
struct FingerprintBudget {
    files: usize,
    bytes: u64,
}

fn collect_fingerprint<F>(
    path: &Path,
    root: &Path,
    depth: usize,
    budget: &mut FingerprintBudget,
    update: &mut F,
) -> Result<(), String>
where
    F: FnMut(&[u8]),
{
    if depth > MAX_FINGERPRINT_DEPTH {
        return Err(format!(
            "Skill 目录层级超过 {} 层，无法计算内容指纹",
            MAX_FINGERPRINT_DEPTH
        ));
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    let relative = path.strip_prefix(root).unwrap_or(path);
    update(relative.to_string_lossy().as_bytes());
    if metadata.file_type().is_symlink() {
        update(b"symlink");
        let target = fs::read_link(path).map_err(|error| error.to_string())?;
        update(target.to_string_lossy().as_bytes());
        return Ok(());
    }
    if metadata.is_file() {
        budget.files += 1;
        budget.bytes = budget.bytes.saturating_add(metadata.len());
        if budget.files > MAX_FINGERPRINT_FILES || budget.bytes > MAX_FINGERPRINT_BYTES {
            return Err(format!(
                "Skill 超过内容指纹限制（最多 {} 个文件、{} MB）",
                MAX_FINGERPRINT_FILES,
                MAX_FINGERPRINT_BYTES / 1024 / 1024
            ));
        }
        update(b"file");
        let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
        let mut buffer = [0u8; 8192];
        loop {
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            update(&buffer[..count]);
        }
        return Ok(());
    }
    if metadata.is_dir() {
        update(b"directory");
        let mut entries = fs::read_dir(path)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            collect_fingerprint(&entry.path(), root, depth + 1, budget, update)?;
        }
    }
    Ok(())
}

fn legacy_content_fingerprint(path: &Path) -> Result<String, String> {
    let mut hash = 0xcbf29ce484222325u64;
    let mut budget = FingerprintBudget::default();
    collect_fingerprint(path, path, 0, &mut budget, &mut |bytes| {
        update_legacy_hash(&mut hash, bytes)
    })?;
    Ok(format!("fnv1a64:{:016x}", hash))
}

fn update_legacy_hash(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn migrate_state_fingerprints(state: &mut SkillMateState) {
    for entry in &mut state.managed_skills {
        if !entry.last_seen_hash.starts_with("fnv1a64:") {
            continue;
        }
        let path = Path::new(&entry.path);
        if !path.exists() && fs::symlink_metadata(path).is_err() {
            continue;
        }
        if legacy_content_fingerprint(path).as_deref() == Ok(entry.last_seen_hash.as_str()) {
            if let Ok(hash) = content_fingerprint(path) {
                entry.last_seen_hash = hash;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "skillmate-state-test-{}-{}",
            name,
            chrono::Utc::now().timestamp_millis()
        ))
    }

    #[test]
    fn writes_and_reads_managed_state() {
        let root = test_dir("write");
        let skill = root.join("writer");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "writer").unwrap();

        mark_managed_skill(&root, "Codex", &skill, "local:/tmp/writer").unwrap();

        let state = read_managed_state(&root).unwrap();
        assert_eq!(state.version, STATE_SCHEMA_VERSION);
        assert_eq!(state.managed_skills.len(), 1);
        assert!(is_managed_by_state(&root, &skill).unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_legacy_fingerprint_without_losing_drift_detection() {
        let root = test_dir("legacy-fingerprint");
        let skill = root.join("writer");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "writer").unwrap();
        let legacy_hash = legacy_content_fingerprint(&skill).unwrap();
        let state = SkillMateState {
            version: 1,
            managed_skills: vec![ManagedSkillState {
                assistant: "Codex".to_string(),
                path: skill.to_string_lossy().to_string(),
                origin: "local:/tmp/writer".to_string(),
                installed_at: "2026-01-01T00:00:00Z".to_string(),
                last_seen_hash: legacy_hash,
            }],
        };
        write_managed_state(&root, &state).unwrap();

        let migrated = read_managed_state(&root).unwrap();

        assert_eq!(migrated.version, STATE_SCHEMA_VERSION);
        assert!(migrated.managed_skills[0]
            .last_seen_hash
            .starts_with("sha256:"));
        assert_eq!(
            migrated.managed_skills[0].last_seen_hash,
            content_fingerprint(&skill).unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_sidecar_paths_outside_managed_root() {
        let base = test_dir("outside-entry");
        let root = base.join("skills");
        let outside = base.join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("SKILL.md"), "outside").unwrap();
        let state = SkillMateState {
            version: STATE_SCHEMA_VERSION,
            managed_skills: vec![ManagedSkillState {
                assistant: "Codex".to_string(),
                path: outside.to_string_lossy().to_string(),
                origin: "local:/tmp/outside".to_string(),
                installed_at: "2026-01-01T00:00:00Z".to_string(),
                last_seen_hash: content_fingerprint(&outside).unwrap(),
            }],
        };
        write_managed_state(&root, &state).unwrap();

        let error = read_managed_state(&root).unwrap_err();

        assert!(error.contains("直接子项"));
        assert!(outside.exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn rejects_duplicate_and_unknown_sidecar_entries() {
        let root = test_dir("duplicate-entry");
        let skill = root.join("writer");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "writer").unwrap();
        let entry = ManagedSkillState {
            assistant: "Codex".to_string(),
            path: skill.to_string_lossy().to_string(),
            origin: "local:/tmp/writer".to_string(),
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            last_seen_hash: content_fingerprint(&skill).unwrap(),
        };
        write_managed_state(
            &root,
            &SkillMateState {
                version: STATE_SCHEMA_VERSION,
                managed_skills: vec![entry.clone(), entry],
            },
        )
        .unwrap();
        assert!(read_managed_state(&root).unwrap_err().contains("重复路径"));

        let unknown = SkillMateState {
            version: STATE_SCHEMA_VERSION,
            managed_skills: vec![ManagedSkillState {
                assistant: "Unknown Agent".to_string(),
                path: skill.to_string_lossy().to_string(),
                origin: "local:/tmp/writer".to_string(),
                installed_at: "2026-01-01T00:00:00Z".to_string(),
                last_seen_hash: content_fingerprint(&skill).unwrap(),
            }],
        };
        write_managed_state(&root, &unknown).unwrap();
        assert!(read_managed_state(&root)
            .unwrap_err()
            .contains("不支持的助手"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn content_fingerprint_is_stable_and_detects_changes() {
        let root = test_dir("fingerprint");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("SKILL.md"), "one").unwrap();

        let first = content_fingerprint(&root).unwrap();
        let second = content_fingerprint(&root).unwrap();
        fs::write(root.join("SKILL.md"), "two").unwrap();
        let changed = content_fingerprint(&root).unwrap();

        assert_eq!(first, second);
        assert_ne!(first, changed);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn corrupted_state_is_reported_instead_of_becoming_empty_state() {
        let root = test_dir("corrupted");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(STATE_FILE_NAME), "not-json").unwrap();

        let error = read_managed_state(&root).unwrap_err();

        assert!(error.contains("受管状态文件损坏"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn checkpoint_reports_new_paths_and_restores_previous_state() {
        let root = test_dir("checkpoint");
        let existing = root.join("existing");
        let added = root.join("added");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("SKILL.md"), "existing").unwrap();
        mark_managed_skill(&root, "Codex", &existing, "local:/tmp/existing").unwrap();
        let checkpoint = ManagedStateCheckpoint::capture(&root).unwrap();

        fs::create_dir_all(&added).unwrap();
        fs::write(added.join("SKILL.md"), "added").unwrap();
        mark_managed_skill(&root, "Codex", &added, "local:/tmp/added").unwrap();

        assert_eq!(checkpoint.new_managed_paths(&root).unwrap(), vec![added]);
        checkpoint.restore(&root).unwrap();
        let restored = read_managed_state(&root).unwrap();
        assert_eq!(restored.managed_skills.len(), 1);
        assert_eq!(restored.managed_skills[0].path, existing.to_string_lossy());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn preserves_logical_root_path_when_root_is_a_symlink() {
        use std::os::unix::fs::symlink;

        let base = test_dir("logical-root");
        let physical_root = base.join("physical-skills");
        let logical_root = base.join("logical-skills");
        let physical_skill = physical_root.join("writer");
        let logical_skill = logical_root.join("writer");
        fs::create_dir_all(&physical_skill).unwrap();
        fs::write(physical_skill.join("SKILL.md"), "writer").unwrap();
        symlink(&physical_root, &logical_root).unwrap();

        mark_managed_skill(&logical_root, "Codex", &logical_skill, "local:/tmp/writer").unwrap();

        let state = read_managed_state(&logical_root).unwrap();
        assert_eq!(
            state.managed_skills[0].path,
            logical_skill.to_string_lossy()
        );
        assert!(is_managed_by_state(&logical_root, &logical_skill).unwrap());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn refreshes_fingerprint_after_managed_update() {
        let root = test_dir("refresh-fingerprint");
        let skill = root.join("writer");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "old").unwrap();
        mark_managed_skill(&root, "Codex", &skill, "example/skills").unwrap();
        let old_hash = read_managed_state(&root).unwrap().managed_skills[0]
            .last_seen_hash
            .clone();
        fs::write(skill.join("SKILL.md"), "new").unwrap();

        assert!(refresh_managed_skill_fingerprint(&root, &skill).unwrap());

        let state = read_managed_state(&root).unwrap();
        assert_ne!(state.managed_skills[0].last_seen_hash, old_hash);
        assert_eq!(
            state.managed_skills[0].last_seen_hash,
            content_fingerprint(&skill).unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }
}
