use crate::app_core::expand_path;
use crate::{AIAssistant, Scenario, Tag};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryExport {
    pub version: u32,
    pub exported_at: String,
    pub tags: Vec<Tag>,
    pub scenarios: Vec<Scenario>,
    pub skills: Vec<LibrarySkillRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibrarySkillRecord {
    pub name: String,
    pub path: String,
    pub assistant: String,
    pub source_type: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportPreview {
    pub replace_existing: bool,
    pub tags_to_add: usize,
    pub tags_to_replace: usize,
    pub scenarios_to_add: usize,
    pub scenarios_to_replace: usize,
    pub skill_tag_writes: usize,
    pub existing_tags_to_remove: usize,
    pub existing_scenarios_to_remove: usize,
    pub existing_skill_tag_mappings_to_remove: usize,
}

pub fn build_library_export(
    tags: Vec<Tag>,
    scenarios: Vec<Scenario>,
    assistants: Vec<AIAssistant>,
) -> LibraryExport {
    let mut skills = Vec::new();
    for assistant in assistants {
        for skill in assistant.skills {
            skills.push(LibrarySkillRecord {
                name: skill.inventory.name,
                path: skill.inventory.path,
                assistant: assistant.name.clone(),
                source_type: skill.inventory.source_type,
                tags: skill.inventory.tags,
            });
        }
    }

    LibraryExport {
        version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        tags,
        scenarios,
        skills,
    }
}

pub fn merge_imported_library(
    db: &Connection,
    export: LibraryExport,
    replace_existing: bool,
) -> Result<(usize, usize), String> {
    let mut tag_count = 0usize;
    let mut scenario_count = 0usize;

    if replace_existing {
        db.execute("DELETE FROM skill_tags", [])
            .map_err(|e| e.to_string())?;
        db.execute("DELETE FROM scenarios", [])
            .map_err(|e| e.to_string())?;
        db.execute("DELETE FROM tags", [])
            .map_err(|e| e.to_string())?;
    }

    for tag in export.tags {
        db.execute(
            "INSERT OR REPLACE INTO tags (id, name, color) VALUES (?, ?, ?)",
            params![tag.id, tag.name, tag.color],
        )
        .map_err(|e| e.to_string())?;
        tag_count += 1;
    }

    for scenario in export.scenarios {
        let skill_ids = scenario.skill_ids.join(",");
        db.execute(
            "INSERT OR REPLACE INTO scenarios (id, name, description, skill_ids, created_at) VALUES (?, ?, ?, ?, ?)",
            params![scenario.id, scenario.name, scenario.description, skill_ids, scenario.created_at],
        )
        .map_err(|e| e.to_string())?;
        scenario_count += 1;
    }

    for skill in export.skills {
        let tags = skill.tags.join(",");
        if !tags.is_empty() {
            db.execute(
                "INSERT OR REPLACE INTO skill_tags (skill_path, tags) VALUES (?, ?)",
                params![skill.path, tags],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    Ok((tag_count, scenario_count))
}

pub fn preview_imported_library(
    db: &Connection,
    export: &LibraryExport,
    replace_existing: bool,
) -> Result<ImportPreview, String> {
    let mut tags_to_add = 0usize;
    let mut tags_to_replace = 0usize;
    let mut scenarios_to_add = 0usize;
    let mut scenarios_to_replace = 0usize;

    for tag in &export.tags {
        let exists = db
            .query_row(
                "SELECT 1 FROM tags WHERE id = ? LIMIT 1",
                params![tag.id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .is_some();
        if exists {
            tags_to_replace += 1;
        } else {
            tags_to_add += 1;
        }
    }

    for scenario in &export.scenarios {
        let exists = db
            .query_row(
                "SELECT 1 FROM scenarios WHERE id = ? LIMIT 1",
                params![scenario.id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .is_some();
        if exists {
            scenarios_to_replace += 1;
        } else {
            scenarios_to_add += 1;
        }
    }

    Ok(ImportPreview {
        replace_existing,
        tags_to_add,
        tags_to_replace,
        scenarios_to_add,
        scenarios_to_replace,
        skill_tag_writes: export
            .skills
            .iter()
            .filter(|skill| !skill.tags.is_empty())
            .count(),
        existing_tags_to_remove: if replace_existing {
            count_rows(db, "tags")?
        } else {
            0
        },
        existing_scenarios_to_remove: if replace_existing {
            count_rows(db, "scenarios")?
        } else {
            0
        },
        existing_skill_tag_mappings_to_remove: if replace_existing {
            count_rows(db, "skill_tags")?
        } else {
            0
        },
    })
}

pub fn read_library_export(path: String) -> Result<LibraryExport, String> {
    let source_path = expand_path(path.trim());
    if !source_path.exists() {
        return Err("导入文件不存在".to_string());
    }
    let content = fs::read_to_string(&source_path).map_err(|e| e.to_string())?;
    let export: LibraryExport =
        serde_json::from_str(&content).map_err(|e| format!("导入文件格式错误: {}", e))?;
    if export.version != 1 {
        return Err(format!("不支持的导入版本: {}", export.version));
    }
    Ok(export)
}

pub fn count_rows(db: &Connection, table: &str) -> Result<usize, String> {
    db.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| {
        row.get(0)
    })
    .map_err(|e| e.to_string())
}

pub fn write_library_export(path: String, export: &LibraryExport) -> Result<String, String> {
    let target_path = expand_path(path.trim());
    if target_path.to_string_lossy().trim().is_empty() {
        return Err("导出文件路径不能为空".to_string());
    }
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let payload = serde_json::to_string_pretty(export).map_err(|e| e.to_string())?;
    fs::write(&target_path, payload).map_err(|e| e.to_string())?;
    Ok(format!("已导出到 {}", target_path.to_string_lossy()))
}
