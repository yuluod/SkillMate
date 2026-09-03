use crate::app_core::{atomic_write, expand_path, managed_skill_roots};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySettings {
    pub path: String,
    pub configurable: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LibraryConfig {
    #[serde(default)]
    library_dir: String,
}

fn data_directory() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("skillmate")
}

fn config_path() -> PathBuf {
    data_directory().join("config.json")
}

fn default_library_root() -> PathBuf {
    data_directory().join("skills")
}

pub fn configured_library_root() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("SKILLMATE_LIBRARY_DIR") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err("SKILLMATE_LIBRARY_DIR 必须是绝对路径".to_string());
        }
        ensure_library_root_is_isolated(&path, &managed_skill_roots())?;
        return Ok(path);
    }
    let path = config_path();
    if !path.exists() {
        return Ok(default_library_root());
    }
    let bytes = fs::read(&path).map_err(|error| format!("无法读取统一库配置: {error}"))?;
    let config: LibraryConfig =
        serde_json::from_slice(&bytes).map_err(|error| format!("统一库配置格式无效: {error}"))?;
    let root = if config.library_dir.trim().is_empty() {
        default_library_root()
    } else {
        let root = PathBuf::from(config.library_dir);
        if root.is_absolute() {
            root
        } else {
            return Err("统一库配置必须使用绝对路径".to_string());
        }
    };
    ensure_library_root_is_isolated(&root, &managed_skill_roots())?;
    Ok(root)
}

pub fn get_library_settings() -> Result<LibrarySettings, String> {
    Ok(LibrarySettings {
        path: configured_library_root()?.to_string_lossy().to_string(),
        configurable: std::env::var_os("SKILLMATE_LIBRARY_DIR").is_none(),
    })
}

pub fn set_library_root(db: &Connection, value: &str) -> Result<LibrarySettings, String> {
    if std::env::var_os("SKILLMATE_LIBRARY_DIR").is_some() {
        return Err("统一库位置由 SKILLMATE_LIBRARY_DIR 环境变量控制".to_string());
    }
    let target = expand_path(value.trim());
    if !target.is_absolute() {
        return Err("统一库位置必须是绝对路径".to_string());
    }
    ensure_library_root_is_isolated(&target, &managed_skill_roots())?;
    let current = configured_library_root()?;
    if same_path(&current, &target) {
        return get_library_settings();
    }
    ensure_library_can_move(db, &current)?;
    if target.exists() {
        let mut entries =
            fs::read_dir(&target).map_err(|error| format!("无法读取新的统一库目录: {error}"))?;
        if entries.next().is_some() {
            return Err("新的统一库目录必须为空".to_string());
        }
    } else {
        fs::create_dir_all(&target).map_err(|error| format!("无法创建新的统一库目录: {error}"))?;
    }
    let config = LibraryConfig {
        library_dir: target.to_string_lossy().to_string(),
    };
    let bytes = serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?;
    atomic_write(&config_path(), &bytes)?;
    get_library_settings()
}

fn ensure_library_can_move(db: &Connection, current: &Path) -> Result<(), String> {
    let managed_count: i64 = db
        .query_row("SELECT COUNT(*) FROM library_skills", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if managed_count > 0 {
        return Err(
            "统一库中已有 Skill。为避免现有启用链接失效，请先移除或迁移这些 Skill 后再更换目录"
                .to_string(),
        );
    }
    if current.exists()
        && fs::read_dir(current)
            .map_err(|error| format!("无法读取当前统一库目录: {error}"))?
            .next()
            .is_some()
    {
        return Err("当前统一库目录不为空，请先移除或迁移其中的 Skill".to_string());
    }
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.canonicalize().unwrap_or_else(|_| left.to_path_buf())
        == right.canonicalize().unwrap_or_else(|_| right.to_path_buf())
}

fn ensure_library_root_is_isolated(
    target: &Path,
    discovery_roots: &[PathBuf],
) -> Result<(), String> {
    let target_canonical = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
    let overlaps = discovery_roots.iter().any(|root| {
        target.starts_with(root)
            || target_canonical.starts_with(root.canonicalize().unwrap_or_else(|_| root.clone()))
    });
    if overlaps {
        Err("统一库不能位于 Agent 的 Skill 发现目录内".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_library_path_is_rejected() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE library_skills (id TEXT)")
            .unwrap();
        assert!(set_library_root(&db, "relative/path").is_err());
    }

    #[test]
    fn managed_library_cannot_change_root() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE library_skills (id TEXT)")
            .unwrap();
        db.execute("INSERT INTO library_skills VALUES ('skill-1')", [])
            .unwrap();
        let error = ensure_library_can_move(&db, Path::new("/missing-library")).unwrap_err();

        assert!(error.contains("统一库中已有 Skill"));
    }

    #[test]
    fn non_empty_library_directory_cannot_move() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE library_skills (id TEXT)")
            .unwrap();
        let root = std::env::temp_dir().join(format!(
            "skillmate-library-config-{}",
            crate::app_core::generate_id()
        ));
        fs::create_dir_all(root.join("writer")).unwrap();

        let error = ensure_library_can_move(&db, &root).unwrap_err();

        assert!(error.contains("当前统一库目录不为空"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn library_root_cannot_live_inside_an_agent_discovery_root() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-library-boundary-{}",
            crate::app_core::generate_id()
        ));
        let discovery_root = root.join(".agents/skills");
        let target = discovery_root.join("library");

        let error = ensure_library_root_is_isolated(&target, &[discovery_root]).unwrap_err();

        assert!(error.contains("发现目录"));
    }
}
