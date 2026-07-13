use crate::app_core::generate_id;
use crate::database::parse_legacy_list;
use crate::{lock_app_db, AppState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Scenario {
    pub id: String,
    pub name: String,
    pub description: String,
    pub skill_ids: Vec<String>,
    pub created_at: String,
}

#[tauri::command]
pub fn get_all_tags(state: tauri::State<'_, AppState>) -> Result<Vec<Tag>, String> {
    let db = lock_app_db(&state)?;
    get_all_tags_from_db(&db)
}

pub fn get_all_tags_from_db(db: &Connection) -> Result<Vec<Tag>, String> {
    let mut statement = db
        .prepare("SELECT id, name, color FROM tags")
        .map_err(|error| error.to_string())?;
    let tags = statement
        .query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(tags)
}

#[tauri::command]
pub fn add_tag(
    state: tauri::State<'_, AppState>,
    name: String,
    color: String,
) -> Result<Tag, String> {
    let db = lock_app_db(&state)?;
    let id = generate_id();
    db.execute(
        "INSERT INTO tags (id, name, color) VALUES (?, ?, ?)",
        params![id, name, color],
    )
    .map_err(|error| error.to_string())?;
    Ok(Tag { id, name, color })
}

#[tauri::command]
pub fn update_skill_tags(
    state: tauri::State<'_, AppState>,
    skill_path: String,
    tags: Vec<String>,
) -> Result<String, String> {
    let db = lock_app_db(&state)?;
    let tags_json = serde_json::to_string(&tags).map_err(|error| error.to_string())?;
    db.execute(
        "INSERT INTO skill_tags (skill_path, tags, tags_json) VALUES (?, '', ?)
         ON CONFLICT(skill_path) DO UPDATE SET tags = '', tags_json = excluded.tags_json",
        params![skill_path, tags_json],
    )
    .map_err(|error| error.to_string())?;
    Ok("已更新".to_string())
}

#[tauri::command]
pub fn get_scenarios(state: tauri::State<'_, AppState>) -> Result<Vec<Scenario>, String> {
    let db = lock_app_db(&state)?;
    get_scenarios_from_db(&db)
}

pub fn get_scenarios_from_db(db: &Connection) -> Result<Vec<Scenario>, String> {
    let mut statement = db
        .prepare("SELECT id, name, description, skill_ids_json, COALESCE(skill_ids, ''), created_at FROM scenarios")
        .map_err(|error| error.to_string())?;
    let records = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    records
        .into_iter()
        .map(
            |(id, name, description, skill_ids_json, legacy_skill_ids, created_at)| {
                let mut skill_ids = serde_json::from_str::<Vec<String>>(&skill_ids_json)
                    .map_err(|error| format!("场景 {} 的 skill_ids_json 损坏: {}", id, error))?;
                if skill_ids.is_empty() && !legacy_skill_ids.is_empty() {
                    skill_ids = parse_legacy_list(&legacy_skill_ids);
                }
                Ok(Scenario {
                    id,
                    name,
                    description,
                    skill_ids,
                    created_at,
                })
            },
        )
        .collect()
}

#[tauri::command]
pub fn create_scenario(
    state: tauri::State<'_, AppState>,
    name: String,
    description: String,
    skill_ids: Vec<String>,
) -> Result<Scenario, String> {
    let db = lock_app_db(&state)?;
    let id = generate_id();
    let created_at = chrono::Local::now().format("%Y-%m-%d").to_string();
    let skill_ids_json = serde_json::to_string(&skill_ids).map_err(|error| error.to_string())?;
    db.execute("INSERT INTO scenarios (id, name, description, skill_ids, skill_ids_json, created_at) VALUES (?, ?, ?, '', ?, ?)", params![id, name, description, skill_ids_json, created_at]).map_err(|error| error.to_string())?;
    Ok(Scenario {
        id,
        name,
        description,
        skill_ids,
        created_at,
    })
}

#[tauri::command]
pub fn delete_scenario(
    state: tauri::State<'_, AppState>,
    scenario_id: String,
) -> Result<String, String> {
    let db = lock_app_db(&state)?;
    db.execute("DELETE FROM scenarios WHERE id = ?", params![scenario_id])
        .map_err(|error| error.to_string())?;
    Ok("已删除".to_string())
}
