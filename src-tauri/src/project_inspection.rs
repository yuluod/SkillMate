use crate::app_core::assistant_definitions;
use crate::skill_inventory::{build_skill, collect_skill_entries, ManagedSkill};
use crate::skill_origin::OriginInferenceCache;
use crate::{Skill, SkillScanDiagnostic};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSkillEntry {
    #[serde(flatten)]
    pub skill: Skill,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectAssistantInspection {
    pub name: String,
    pub icon: String,
    pub project_root: String,
    pub project_count: usize,
    pub global_count: usize,
    pub shadowed_count: usize,
    pub skills: Vec<ProjectSkillEntry>,
    pub diagnostics: Vec<SkillScanDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInspection {
    pub project_path: String,
    pub assistants: Vec<ProjectAssistantInspection>,
}

pub fn inspect_project_skills(
    db: &Connection,
    project_path: &Path,
) -> Result<ProjectInspection, String> {
    let project = project_path
        .canonicalize()
        .map_err(|error| format!("项目路径不存在或无法访问: {error}"))?;
    if !project.is_dir() {
        return Err("项目路径不是目录".to_string());
    }

    let mut assistants = Vec::new();
    let mut origin_cache = OriginInferenceCache::default();
    for assistant in assistant_definitions() {
        let project_root = assistant
            .project_install_root(&project)
            .ok_or_else(|| format!("{} 不支持项目级 Skills", assistant.name))?;
        let mut diagnostics = Vec::new();
        let mut seen_paths = HashSet::new();
        let mut project_entries = Vec::new();
        collect_skill_entries(
            &project_root,
            assistant.recursive_discovery_depth(),
            &mut project_entries,
            &mut seen_paths,
            &mut diagnostics,
        );

        let mut global_entries = Vec::new();
        for root in assistant.global_discovery_roots() {
            collect_skill_entries(
                &root,
                assistant.recursive_discovery_depth(),
                &mut global_entries,
                &mut seen_paths,
                &mut diagnostics,
            );
        }

        let mut identities = HashSet::new();
        let mut skills = Vec::new();
        let mut shadowed_count = 0;
        append_effective_skills(
            db,
            &mut origin_cache,
            project_entries,
            "project",
            &mut identities,
            &mut skills,
            &mut shadowed_count,
        );
        let project_count = skills.len();
        append_effective_skills(
            db,
            &mut origin_cache,
            global_entries,
            "global",
            &mut identities,
            &mut skills,
            &mut shadowed_count,
        );
        let global_count = skills.len().saturating_sub(project_count);
        skills.sort_by(|left, right| {
            scope_order(&left.scope)
                .cmp(&scope_order(&right.scope))
                .then_with(|| left.skill.inventory.name.cmp(&right.skill.inventory.name))
        });
        assistants.push(ProjectAssistantInspection {
            name: assistant.name.to_string(),
            icon: assistant.icon.to_string(),
            project_root: project_root.to_string_lossy().to_string(),
            project_count,
            global_count,
            shadowed_count,
            skills,
            diagnostics,
        });
    }

    Ok(ProjectInspection {
        project_path: project.to_string_lossy().to_string(),
        assistants,
    })
}

fn append_effective_skills(
    db: &Connection,
    origin_cache: &mut OriginInferenceCache,
    paths: Vec<PathBuf>,
    scope: &str,
    identities: &mut HashSet<String>,
    output: &mut Vec<ProjectSkillEntry>,
    shadowed_count: &mut usize,
) {
    for path in paths {
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        let skill = build_skill(db, &ManagedSkill { path, name }, origin_cache);
        let identity = skill
            .structure
            .manifest_title
            .as_deref()
            .unwrap_or(&skill.inventory.name)
            .trim()
            .to_lowercase();
        if !identities.insert(identity) {
            *shadowed_count += 1;
            continue;
        }
        output.push(ProjectSkillEntry {
            skill,
            scope: scope.to_string(),
        });
    }
}

fn scope_order(scope: &str) -> u8 {
    if scope == "project" {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_library::use_test_library_root;
    use rusqlite::Connection;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "skillmate-project-inspection-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_skill(path: &Path, name: &str) {
        fs::create_dir_all(path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test\n---\nbody"),
        )
        .unwrap();
    }

    #[test]
    fn project_skill_wins_over_same_named_global_skill() {
        let root = temp_dir("precedence");
        let project = root.join("project");
        let library = root.join("library");
        fs::create_dir_all(&project).unwrap();
        let _guard = use_test_library_root(library);
        write_skill(&project.join(".agents/skills/project-copy"), "same-skill");

        let db = Connection::open_in_memory().unwrap();
        let inspection = inspect_project_skills(&db, &project).unwrap();
        let codex = inspection
            .assistants
            .iter()
            .find(|assistant| assistant.name == "Codex")
            .unwrap();
        assert_eq!(codex.project_count, 1);
        assert_eq!(codex.skills[0].scope, "project");
        let _ = fs::remove_dir_all(root);
    }
}
