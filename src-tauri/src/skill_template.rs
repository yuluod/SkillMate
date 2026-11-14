use rusqlite::Connection;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::managed_installation::record_managed_path;
use crate::skill_library::{library_root, register_library_skill};
use crate::skill_structure::{is_valid_skill_name, validate_skill_structure};

pub const MAX_DESCRIPTION_CHARS: usize = 1024;

#[derive(Debug, Serialize)]
pub struct SkillDraftValidation {
    pub ok: bool,
    pub errors: Vec<String>,
}

/// 创建前的纯校验：不落盘。name 规则与 `validate_skill_structure` 的
/// `invalid_name` 检查同源，description 规则与 `description_too_long` 同源；
/// 另检查统一库内的同名目录与同名登记，避免产生半成品目录。
pub fn validate_skill_draft(
    db: &Connection,
    name: &str,
    description: &str,
) -> SkillDraftValidation {
    let errors = draft_errors(db, name, description);
    SkillDraftValidation {
        ok: errors.is_empty(),
        errors,
    }
}

/// 从静态模板直接在统一库创建新 Skill：写入合法 SKILL.md → 结构校验 →
/// 受管登记（sidecar + managed_roots + library_skills）。失败时清理半成品目录。
pub fn create_library_skill(
    db: &Connection,
    name: &str,
    description: &str,
    locale: &str,
) -> Result<PathBuf, String> {
    let errors = draft_errors(db, name, description);
    if !errors.is_empty() {
        return Err(errors.join("；"));
    }
    let name = name.trim();
    let root = library_root()?;
    let target = root.join(name);

    let outcome = (|| -> Result<(), String> {
        write_template_skill(&target, name, description.trim(), locale)?;
        let report = validate_skill_structure(&target);
        if report.structure_status != "complete" {
            let messages = report
                .checks
                .iter()
                .map(|check| check.message.clone())
                .collect::<Vec<_>>()
                .join("；");
            return Err(format!("生成的模板未通过结构校验: {messages}"));
        }
        crate::managed_state::mark_managed_skill(&root, "SkillMate", &target, "template")?;
        record_managed_path(db, &root, &target, "library", None)?;
        register_library_skill(db, &target, "模板创建", "template", None)?;
        Ok(())
    })();
    match outcome {
        Ok(()) => Ok(target),
        Err(error) => {
            cleanup_created_skill(&target);
            Err(error)
        }
    }
}

fn draft_errors(db: &Connection, name: &str, description: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        errors.push("名称不能为空".to_string());
    } else if !is_valid_skill_name(trimmed_name) {
        errors.push(
            "名称只能使用小写字母、数字和连字符（1–64 字符），不能以连字符开头或结尾，且不能包含连续连字符"
                .to_string(),
        );
    }
    let trimmed_description = description.trim();
    if trimmed_description.is_empty() {
        errors.push("描述不能为空".to_string());
    } else if trimmed_description.chars().count() > MAX_DESCRIPTION_CHARS {
        errors.push(format!("描述不能超过 {MAX_DESCRIPTION_CHARS} 字符"));
    }
    if !errors.is_empty() {
        return errors;
    }
    match library_root() {
        Ok(root) => {
            let target = root.join(trimmed_name);
            if target.exists() {
                errors.push(format!("统一库中已存在同名目录: {}", target.display()));
            }
            let registered = db
                .query_row(
                    "SELECT 1 FROM library_skills WHERE name = ?",
                    [trimmed_name],
                    |row| row.get::<_, i64>(0),
                )
                .is_ok();
            if registered {
                errors.push(format!("已存在同名 Skill 登记: {trimmed_name}"));
            }
        }
        Err(error) => errors.push(error),
    }
    errors
}

/// 双引号 YAML 标量：换行折叠为空格，转义反斜杠与引号，
/// 保证任意用户输入都不会破坏 frontmatter 结构。
fn yaml_quote(value: &str) -> String {
    let collapsed = value
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let escaped = collapsed.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn write_template_skill(
    target: &Path,
    name: &str,
    description: &str,
    locale: &str,
) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|error| format!("无法创建 Skill 目录: {error}"))?;
    let body = template_body(name, description, locale);
    fs::write(target.join("SKILL.md"), body).map_err(|error| format!("无法写入 SKILL.md: {error}"))
}

fn template_body(name: &str, description: &str, locale: &str) -> String {
    let (intro, dirs) = if locale == "en" {
        (
            "Describe what this skill does and when the agent should use it.",
            "<!--\nOptional directory conventions (create them by hand when needed;\nSkillMate only requires SKILL.md):\n- references/: supplementary documents\n- scripts/: executable scripts\n- assets/: static resources\n-->",
        )
    } else {
        (
            "在这里描述这个 Skill 的用途，以及 Agent 应该在什么场景下使用它。",
            "<!--\n可选目录约定（需要时手动创建即可；SkillMate 只要求 SKILL.md）：\n- references/：补充说明文档\n- scripts/：可执行脚本\n- assets/：静态资源\n-->",
        )
    };
    format!(
        "---\nname: {name}\ndescription: {}\n---\n\n# {name}\n\n{intro}\n\n{dirs}\n",
        yaml_quote(description)
    )
}

fn cleanup_created_skill(target: &Path) {
    // 半成品清理是尽力而为：目录移除失败时保留现场，由用户手工处理，
    // 不掩盖原始错误。
    let _ = fs::remove_dir_all(target);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn draft_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        // 登记链路（record_managed_path / register_library_skill）依赖的最小 schema。
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS library_skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                library_path TEXT NOT NULL UNIQUE,
                source TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                resolved_ref TEXT,
                content_hash TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS managed_roots (
                root_path TEXT PRIMARY KEY,
                scope TEXT NOT NULL DEFAULT 'global',
                project_path TEXT,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS managed_installations (
                skill_path TEXT PRIMARY KEY,
                assistant TEXT NOT NULL,
                source TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                target_name TEXT NOT NULL,
                scope TEXT NOT NULL DEFAULT 'global',
                install_mode TEXT NOT NULL DEFAULT 'copy',
                project_path TEXT,
                tracking_ref TEXT,
                subdir TEXT,
                resolved_ref TEXT,
                content_hash TEXT,
                installed_at TEXT NOT NULL DEFAULT ''
            );",
        )
        .unwrap();
        db
    }

    #[test]
    fn draft_validation_mirrors_structure_rules() {
        let db = draft_db();
        let root =
            std::env::temp_dir().join(format!("skillmate-template-root-{}", std::process::id()));
        let _guard = crate::skill_library::use_test_library_root(root.clone());

        assert!(validate_skill_draft(&db, "", "desc")
            .errors
            .iter()
            .any(|e| e.contains("名称")));
        assert!(validate_skill_draft(&db, "Bad_Name", "desc")
            .errors
            .iter()
            .any(|e| e.contains("名称")));
        assert!(validate_skill_draft(&db, "-bad", "desc")
            .errors
            .iter()
            .any(|e| e.contains("名称")));
        assert!(validate_skill_draft(&db, "ok-name", "  ")
            .errors
            .iter()
            .any(|e| e.contains("描述")));
        assert!(
            validate_skill_draft(&db, "ok-name", &"长".repeat(MAX_DESCRIPTION_CHARS + 1))
                .errors
                .iter()
                .any(|e| e.contains("描述"))
        );

        std::fs::create_dir_all(root.join("taken")).unwrap();
        let taken = validate_skill_draft(&db, "taken", "desc");
        assert!(taken.errors.iter().any(|e| e.contains("同名目录")));
        assert!(!taken.ok);

        let free = validate_skill_draft(&db, "free-name", "desc");
        assert!(free.ok, "unexpected errors: {:?}", free.errors);
    }

    #[test]
    fn created_template_passes_structure_validation_and_registers() {
        let db = draft_db();
        let root =
            std::env::temp_dir().join(format!("skillmate-template-create-{}", std::process::id()));
        let _guard = crate::skill_library::use_test_library_root(root.clone());

        let path =
            create_library_skill(&db, "my-template", "用于写作的模板 Skill", "zh-CN").unwrap();
        assert!(path.join("SKILL.md").exists());
        assert_eq!(path.file_name().unwrap().to_string_lossy(), "my-template");

        let report = validate_skill_structure(&path);
        assert_eq!(
            report.structure_status, "complete",
            "checks: {:?}",
            report.checks
        );

        let registered: String = db
            .query_row(
                "SELECT library_path FROM library_skills WHERE name = 'my-template'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(registered, path.to_string_lossy());

        let content = fs::read_to_string(path.join("SKILL.md")).unwrap();
        assert!(content.contains("name: my-template"));
        assert!(content.contains(&yaml_quote("用于写作的模板 Skill")));

        // 重名创建必须失败且不产生第二个目录
        let again = create_library_skill(&db, "my-template", "另一个", "zh-CN");
        assert!(again.is_err());
    }

    #[test]
    fn yaml_quote_escapes_quotes_and_backslashes() {
        assert_eq!(
            yaml_quote("say \"hi\" \\ done"),
            "\"say \\\"hi\\\" \\\\ done\""
        );
        assert_eq!(yaml_quote("line1\nline2"), "\"line1 line2\"");
    }
}
