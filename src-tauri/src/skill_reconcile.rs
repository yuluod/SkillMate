use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ReconcileTransaction {
    created_targets: Vec<PathBuf>,
    moved_targets: Vec<(PathBuf, PathBuf)>,
}

impl ReconcileTransaction {
    pub fn prepare(removals: &[PathBuf], install_targets: &[PathBuf]) -> Result<Self, String> {
        let created_targets = install_targets
            .iter()
            .filter(|path| !path_exists(path))
            .cloned()
            .collect::<Vec<_>>();
        let mut transaction = Self {
            created_targets,
            moved_targets: Vec::new(),
        };

        for (index, target) in removals.iter().enumerate() {
            if !path_exists(target) {
                continue;
            }
            let backup = backup_path(target, index);
            if path_exists(&backup) {
                let rollback_error = transaction.rollback().err();
                return Err(prepare_error(
                    format!("回滚暂存路径已存在: {}", backup.to_string_lossy()),
                    rollback_error,
                ));
            }
            if let Err(error) = fs::rename(target, &backup) {
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
            transaction.moved_targets.push((target.clone(), backup));
        }
        Ok(transaction)
    }

    pub fn commit(mut self) -> Result<(), String> {
        let mut failures = Vec::new();
        for (_, backup) in self.moved_targets.drain(..) {
            if let Err(error) = remove_path(&backup) {
                failures.push(format!("{}: {}", backup.to_string_lossy(), error));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(format!("清理回滚暂存失败: {}", failures.join("；")))
        }
    }

    pub fn rollback(&mut self) -> Result<(), String> {
        let mut failures = Vec::new();
        for target in self.created_targets.iter().rev() {
            if path_exists(target) {
                if let Err(error) = remove_path(target) {
                    failures.push(format!("移除 {} 失败: {}", target.to_string_lossy(), error));
                }
            }
        }
        for (target, backup) in self.moved_targets.iter().rev() {
            if path_exists(target) {
                if let Err(error) = remove_path(target) {
                    failures.push(format!("移除 {} 失败: {}", target.to_string_lossy(), error));
                    continue;
                }
            }
            if path_exists(backup) {
                if let Err(error) = fs::rename(backup, target) {
                    failures.push(format!("恢复 {} 失败: {}", target.to_string_lossy(), error));
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("；"))
        }
    }
}

fn prepare_error(error: String, rollback_error: Option<String>) -> String {
    match rollback_error {
        Some(rollback_error) => format!("{}；回滚不完整: {}", error, rollback_error),
        None => error,
    }
}

fn backup_path(target: &Path, index: usize) -> PathBuf {
    let name = target
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "skill".into());
    target.with_file_name(format!(
        ".{}.skillmate-reconcile-{}-{}",
        name,
        std::process::id(),
        index
    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "skillmate-reconcile-{}-{}-{}",
            name,
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    #[test]
    fn rollback_restores_removed_targets_and_deletes_new_targets() {
        let root = test_root("rollback");
        let removed = root.join("removed");
        let created = root.join("created");
        fs::create_dir_all(&removed).unwrap();
        fs::write(removed.join("SKILL.md"), "old").unwrap();

        let mut transaction = ReconcileTransaction::prepare(
            std::slice::from_ref(&removed),
            std::slice::from_ref(&created),
        )
        .unwrap();
        fs::create_dir_all(&created).unwrap();
        fs::write(created.join("SKILL.md"), "new").unwrap();
        transaction.rollback().unwrap();

        assert_eq!(fs::read_to_string(removed.join("SKILL.md")).unwrap(), "old");
        assert!(!created.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn commit_removes_staged_targets() {
        let root = test_root("commit");
        let removed = root.join("removed");
        fs::create_dir_all(&removed).unwrap();

        let transaction =
            ReconcileTransaction::prepare(std::slice::from_ref(&removed), &[]).unwrap();
        transaction.commit().unwrap();

        assert!(!removed.exists());
        assert!(fs::read_dir(&root).unwrap().next().is_none());
        let _ = fs::remove_dir_all(root);
    }
}
