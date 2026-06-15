use crate::app_core::now_ms;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const STATE_FILE_NAME: &str = ".skillmate-state.json";

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

pub fn read_managed_state(root: &Path) -> SkillMateState {
    let path = root.join(STATE_FILE_NAME);
    let Ok(content) = fs::read_to_string(path) else {
        return SkillMateState {
            version: 1,
            managed_skills: vec![],
        };
    };
    serde_json::from_str(&content).unwrap_or(SkillMateState {
        version: 1,
        managed_skills: vec![],
    })
}

pub fn mark_managed_skill(
    root: &Path,
    assistant: &str,
    target_path: &Path,
    origin: &str,
) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut state = read_managed_state(root);
    state.version = 1;
    let target = target_path.to_string_lossy().to_string();
    let hash = path_fingerprint(target_path);
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

pub fn is_managed_by_state(root: &Path, target_path: &Path) -> bool {
    managed_state_entry(root, target_path).is_some()
}

pub fn managed_state_entry(root: &Path, target_path: &Path) -> Option<ManagedSkillState> {
    let target = target_path.to_string_lossy().to_string();
    read_managed_state(root)
        .managed_skills
        .into_iter()
        .find(|item| item.path == target)
}

pub fn managed_state_origin(root: &Path, target_path: &Path) -> Option<String> {
    managed_state_entry(root, target_path).map(|entry| entry.origin)
}

fn write_managed_state(root: &Path, state: &SkillMateState) -> Result<(), String> {
    let path = root.join(STATE_FILE_NAME);
    let content = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(path, content).map_err(|e| e.to_string())
}

fn path_fingerprint(path: &Path) -> String {
    let mut count = 0u64;
    let mut bytes = 0u64;
    let mut modified = 0u64;
    collect_fingerprint(path, &mut count, &mut bytes, &mut modified, 0);
    format!("v1:{}:{}:{}:{}", now_ms(), count, bytes, modified)
}

fn collect_fingerprint(
    path: &Path,
    count: &mut u64,
    bytes: &mut u64,
    modified: &mut u64,
    depth: usize,
) {
    if depth > 12 {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    *count += 1;
    *bytes += metadata.len();
    if let Ok(time) = metadata.modified() {
        if let Ok(duration) = time.duration_since(std::time::UNIX_EPOCH) {
            *modified = (*modified).max(duration.as_secs());
        }
    }
    if metadata.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                collect_fingerprint(&entry.path(), count, bytes, modified, depth + 1);
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

        let state = read_managed_state(&root);
        assert_eq!(state.version, 1);
        assert_eq!(state.managed_skills.len(), 1);
        assert!(is_managed_by_state(&root, &skill));
        let _ = fs::remove_dir_all(root);
    }
}
