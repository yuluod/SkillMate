use crate::app_core::{atomic_write, expand_path};
use crate::library_manifest::count_rows;
use crate::operation_plan::operation_plan_token;
use crate::Scenario;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize)]
pub struct ScenarioManifest {
    pub version: u32,
    pub exported_at: String,
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScenarioManifestPreview {
    pub replace_existing: bool,
    pub scenarios_to_add: usize,
    pub scenarios_to_replace: usize,
    pub existing_scenarios_to_remove: usize,
    pub missing_skill_refs: Vec<String>,
    pub plan_token: String,
}

pub fn build_scenario_manifest(scenarios: Vec<Scenario>) -> ScenarioManifest {
    ScenarioManifest {
        version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        scenarios,
    }
}

pub fn read_scenario_manifest(path: String) -> Result<ScenarioManifest, String> {
    let source_path = expand_path(path.trim());
    if !source_path.exists() {
        return Err("场景 manifest 文件不存在".to_string());
    }
    let content = fs::read_to_string(&source_path).map_err(|e| e.to_string())?;
    let manifest: ScenarioManifest =
        serde_json::from_str(&content).map_err(|e| format!("场景 manifest 格式错误: {}", e))?;
    if manifest.version != 1 {
        return Err(format!("不支持的场景 manifest 版本: {}", manifest.version));
    }
    Ok(manifest)
}

pub fn preview_scenario_manifest(
    db: &Connection,
    manifest: &ScenarioManifest,
    replace_existing: bool,
    known_skill_paths: &[String],
) -> Result<ScenarioManifestPreview, String> {
    let mut scenarios_to_add = 0usize;
    let mut scenarios_to_replace = 0usize;
    for scenario in &manifest.scenarios {
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

    let known_skill_paths: std::collections::HashSet<&str> =
        known_skill_paths.iter().map(|path| path.as_str()).collect();
    let mut missing_skill_refs: Vec<String> = manifest
        .scenarios
        .iter()
        .flat_map(|scenario| scenario.skill_ids.iter())
        .filter(|path| !known_skill_paths.contains(path.as_str()))
        .cloned()
        .collect();
    missing_skill_refs.sort();
    missing_skill_refs.dedup();

    let mut preview = ScenarioManifestPreview {
        replace_existing,
        scenarios_to_add,
        scenarios_to_replace,
        existing_scenarios_to_remove: if replace_existing {
            count_rows(db, "scenarios")?
        } else {
            0
        },
        missing_skill_refs,
        plan_token: String::new(),
    };
    preview.plan_token = operation_plan_token("scenario-import", &(manifest, &preview))?;
    Ok(preview)
}

pub fn merge_scenario_manifest(
    db: &Connection,
    manifest: ScenarioManifest,
    replace_existing: bool,
) -> Result<usize, String> {
    let transaction = db
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    if replace_existing {
        transaction
            .execute("DELETE FROM scenarios", [])
            .map_err(|e| e.to_string())?;
    }
    let mut scenario_count = 0usize;
    for scenario in manifest.scenarios {
        let skill_ids_json =
            serde_json::to_string(&scenario.skill_ids).map_err(|error| error.to_string())?;
        transaction.execute(
            "INSERT OR REPLACE INTO scenarios (id, name, description, skill_ids, skill_ids_json, created_at) VALUES (?, ?, ?, '', ?, ?)",
            params![scenario.id, scenario.name, scenario.description, skill_ids_json, scenario.created_at],
        )
        .map_err(|e| e.to_string())?;
        scenario_count += 1;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(scenario_count)
}

pub fn write_scenario_manifest(
    path: String,
    manifest: &ScenarioManifest,
) -> Result<String, String> {
    let target_path = expand_path(path.trim());
    if target_path.to_string_lossy().trim().is_empty() {
        return Err("导出文件路径不能为空".to_string());
    }
    let payload = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    atomic_write(&target_path, payload.as_bytes())?;
    Ok(format!(
        "已导出场景 manifest 到 {}",
        target_path.to_string_lossy()
    ))
}
