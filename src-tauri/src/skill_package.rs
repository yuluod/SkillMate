use crate::skill_structure::{
    analyze_skill_safety, analyze_skill_structure, detect_skill_entry, SkillEntryKind,
    SkillStructureInfo,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct PackageDetection {
    pub package_kind: String,
    pub detected_skills: Vec<DetectedSkill>,
    pub warnings: Vec<String>,
    pub needs_model: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct DetectedSkill {
    pub relative_path: String,
    pub structure_status: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub features: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn detect_skill_package(path: &Path) -> PackageDetection {
    if !path.exists() {
        return PackageDetection {
            package_kind: "unknown".to_string(),
            detected_skills: vec![],
            warnings: vec!["path_missing".to_string()],
            needs_model: true,
        };
    }

    let has_bundle_signal = has_assistant_bundle_signal(path);
    let root_structure = analyze_skill_structure(path);
    if has_standard_entry_document(path) {
        return PackageDetection {
            package_kind: "single_skill".to_string(),
            detected_skills: vec![detected_skill(path, path, root_structure)],
            warnings: package_warnings(has_bundle_signal, false),
            needs_model: false,
        };
    }

    let mut skills = collect_child_skills(path, false);
    if skills.is_empty() {
        skills = collect_child_skills(path, true);
    }
    if skills.len() > 1 {
        skills.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        return PackageDetection {
            package_kind: if has_bundle_signal {
                "assistant_bundle"
            } else {
                "multi_skill"
            }
            .to_string(),
            detected_skills: skills,
            warnings: package_warnings(has_bundle_signal, false),
            needs_model: false,
        };
    }

    if skills.len() == 1 {
        return PackageDetection {
            package_kind: if has_bundle_signal {
                "assistant_bundle"
            } else {
                "single_skill"
            }
            .to_string(),
            detected_skills: skills,
            warnings: package_warnings(has_bundle_signal, false),
            needs_model: false,
        };
    }

    let mut warnings = package_warnings(has_bundle_signal, true);
    if root_structure.structure_status == "partial" {
        return PackageDetection {
            package_kind: "single_skill".to_string(),
            detected_skills: vec![detected_skill(path, path, root_structure)],
            warnings,
            needs_model: false,
        };
    }
    warnings.push("unrecognized_input".to_string());
    PackageDetection {
        package_kind: if has_bundle_signal {
            "assistant_bundle"
        } else {
            "unknown"
        }
        .to_string(),
        detected_skills: vec![],
        warnings,
        needs_model: !has_bundle_signal,
    }
}

fn collect_child_skills(root: &Path, include_legacy: bool) -> Vec<DetectedSkill> {
    let mut skills = Vec::new();
    let mut candidates = immediate_dirs(root);
    for bundle_root in [
        ".codex/skills",
        ".claude/skills",
        ".gemini/skills",
        ".openclaw/skills",
        ".agents/skills",
        "skills",
        "agents",
    ] {
        let path = root.join(bundle_root);
        if path.is_dir() {
            let direct_candidates = immediate_dirs(&path);
            for candidate in &direct_candidates {
                if !(has_standard_entry_document(candidate)
                    || include_legacy && has_legacy_entry_document(candidate))
                {
                    candidates.extend(immediate_dirs(candidate));
                }
            }
            candidates.extend(direct_candidates);
        }
    }
    candidates.sort();
    candidates.dedup();
    for candidate in candidates {
        if has_standard_entry_document(&candidate)
            || (include_legacy && has_legacy_entry_document(&candidate))
        {
            let structure = analyze_skill_structure(&candidate);
            skills.push(detected_skill(root, &candidate, structure));
        }
    }
    skills
}

fn immediate_dirs(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|entry| entry.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

fn detected_skill(root: &Path, path: &Path, mut structure: SkillStructureInfo) -> DetectedSkill {
    for warning in analyze_skill_safety(path) {
        if !structure.structure_warnings.contains(&warning) {
            structure.structure_warnings.push(warning);
        }
    }
    DetectedSkill {
        relative_path: relative_path(root, path),
        structure_status: structure.structure_status,
        title: structure.manifest_title,
        description: structure.manifest_description,
        features: structure.structure_features,
        warnings: structure.structure_warnings,
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(normalized_relative_path)
        .unwrap_or_else(|| ".".to_string())
}

fn normalized_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn has_standard_entry_document(path: &Path) -> bool {
    detect_skill_entry(path) == SkillEntryKind::Standard
}

fn has_legacy_entry_document(path: &Path) -> bool {
    matches!(
        detect_skill_entry(path),
        SkillEntryKind::LegacyFilename | SkillEntryKind::ReadmeOnly
    )
}

fn has_assistant_bundle_signal(path: &Path) -> bool {
    [
        "agents.toml",
        ".claude/agents",
        ".claude/skills",
        ".gemini/skills",
        ".openclaw/skills",
        ".agents/skills",
        ".codex/skills",
        ".codex-plugin/plugin.json",
    ]
    .iter()
    .any(|name| path.join(name).exists())
}

fn package_warnings(has_bundle_signal: bool, no_skills: bool) -> Vec<String> {
    let mut warnings = Vec::new();
    if has_bundle_signal {
        warnings.push("assistant_bundle_detected".to_string());
    }
    if no_skills {
        warnings.push("missing_entry_document".to_string());
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "skillmate-package-test-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn detects_single_root_skill() {
        let temp = test_dir("single-root");
        let root = temp.join("writer");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nname: writer\ndescription: 帮助整理文稿\n---\n说明",
        )
        .unwrap();

        let detection = detect_skill_package(&root);

        assert_eq!(detection.package_kind, "single_skill");
        assert_eq!(detection.detected_skills.len(), 1);
        assert_eq!(detection.detected_skills[0].relative_path, ".");
        assert_eq!(detection.detected_skills[0].structure_status, "complete");
        assert!(!detection.needs_model);
        let _ = fs::remove_dir_all(temp);
    }

    #[test]
    fn detects_multi_skill_children() {
        let root = test_dir("multi");
        fs::create_dir_all(root.join("writer")).unwrap();
        fs::create_dir_all(root.join("reviewer")).unwrap();
        fs::create_dir_all(root.join("legacy")).unwrap();
        fs::write(
            root.join("writer/SKILL.md"),
            "---\nname: writer\ndescription: 写作\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("reviewer/SKILL.md"),
            "---\nname: reviewer\ndescription: 审查\n---\n",
        )
        .unwrap();
        fs::write(root.join("legacy/README.md"), "legacy").unwrap();

        let detection = detect_skill_package(&root);

        assert_eq!(detection.package_kind, "multi_skill");
        assert_eq!(detection.detected_skills.len(), 2);
        assert!(detection
            .detected_skills
            .iter()
            .any(|skill| skill.relative_path == "writer"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detects_assistant_bundle() {
        let root = test_dir("bundle");
        fs::create_dir_all(root.join(".codex/skills/writer")).unwrap();
        fs::write(
            root.join(".codex/skills/writer/SKILL.md"),
            "---\nname: writer\ndescription: 写作\n---\n",
        )
        .unwrap();

        let detection = detect_skill_package(&root);

        assert_eq!(detection.package_kind, "assistant_bundle");
        assert!(detection
            .detected_skills
            .iter()
            .any(|skill| skill.relative_path == ".codex/skills/writer"));
        assert!(detection
            .warnings
            .contains(&"assistant_bundle_detected".to_string()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detects_one_level_skill_categories() {
        let root = test_dir("category-layout");
        let skill = root.join("skills/writing/writer");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: writer\ndescription: 写作\n---\n",
        )
        .unwrap();

        let detection = detect_skill_package(&root);

        assert_eq!(detection.detected_skills.len(), 1);
        assert_eq!(
            detection.detected_skills[0].relative_path,
            "skills/writing/writer"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_package_needs_model() {
        let root = test_dir("unknown");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("notes.txt"), "帮我安装这个 skill").unwrap();

        let detection = detect_skill_package(&root);

        assert_eq!(detection.package_kind, "unknown");
        assert!(detection.needs_model);
        let _ = fs::remove_dir_all(root);
    }
}
