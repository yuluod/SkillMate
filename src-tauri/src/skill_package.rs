use crate::skill_structure::{
    analyze_skill_safety, analyze_skill_structure, detect_skill_entry, SkillEntryKind,
    SkillStructureInfo,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_DISCOVERY_DEPTH: usize = 4;
const MAX_DISCOVERY_DIRECTORIES: usize = 4_096;

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

#[derive(Debug, Clone)]
pub struct SkillPackageSource {
    source_root: PathBuf,
    canonical_root: PathBuf,
}

impl SkillPackageSource {
    pub fn open(source_root: &Path) -> Result<Self, String> {
        let metadata = fs::symlink_metadata(source_root).map_err(|error| {
            format!(
                "无法检查 Skill 来源目录 {}: {error}",
                source_root.to_string_lossy()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Skill 来源目录不能是软连接: {}",
                source_root.to_string_lossy()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!(
                "Skill 来源不是目录: {}",
                source_root.to_string_lossy()
            ));
        }
        let canonical_root = source_root.canonicalize().map_err(|error| {
            format!(
                "无法解析 Skill 来源目录 {}: {error}",
                source_root.to_string_lossy()
            )
        })?;
        Ok(Self {
            source_root: source_root.to_path_buf(),
            canonical_root,
        })
    }

    pub fn resolve_detected_skill(&self, skill: &DetectedSkill) -> Result<PathBuf, String> {
        self.resolve_relative(Path::new(&skill.relative_path))
    }

    fn resolve_relative(&self, relative: &Path) -> Result<PathBuf, String> {
        let candidate = if relative == Path::new(".") {
            self.source_root.clone()
        } else {
            if relative.as_os_str().is_empty()
                || relative.is_absolute()
                || relative
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(format!(
                    "Skill 候选路径不是安全的相对路径: {}",
                    relative.to_string_lossy()
                ));
            }
            self.canonical_root.join(relative)
        };
        self.resolve_directory(&candidate)
    }

    fn resolve_directory(&self, candidate: &Path) -> Result<PathBuf, String> {
        let metadata = fs::symlink_metadata(candidate).map_err(|error| {
            format!(
                "无法检查 Skill 候选目录 {}: {error}",
                candidate.to_string_lossy()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Skill 候选目录不能是软连接: {}",
                candidate.to_string_lossy()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!(
                "Skill 候选路径不是目录: {}",
                candidate.to_string_lossy()
            ));
        }
        let resolved = candidate.canonicalize().map_err(|error| {
            format!(
                "无法解析 Skill 候选目录 {}: {error}",
                candidate.to_string_lossy()
            )
        })?;
        if !resolved.starts_with(&self.canonical_root) {
            return Err(format!(
                "Skill 候选目录越过了来源根目录: {}",
                candidate.to_string_lossy()
            ));
        }
        Ok(resolved)
    }

    fn relative_path(&self, candidate: &Path) -> Result<String, String> {
        candidate
            .strip_prefix(&self.canonical_root)
            .map_err(|_| {
                format!(
                    "Skill 候选目录越过了来源根目录: {}",
                    candidate.to_string_lossy()
                )
            })
            .map(|relative| {
                if relative.as_os_str().is_empty() {
                    ".".to_string()
                } else {
                    normalized_relative_path(relative)
                }
            })
    }
}

#[derive(Default)]
struct CollectedSkills {
    skills: Vec<DetectedSkill>,
    unsafe_paths: bool,
    scan_limited: bool,
}

pub fn detect_skill_package(path: &Path) -> PackageDetection {
    if fs::symlink_metadata(path).is_err() {
        return PackageDetection {
            package_kind: "unknown".to_string(),
            detected_skills: vec![],
            warnings: vec!["path_missing".to_string()],
            needs_model: true,
        };
    }

    let source = match SkillPackageSource::open(path) {
        Ok(source) => source,
        Err(_) => {
            return PackageDetection {
                package_kind: "unknown".to_string(),
                detected_skills: vec![],
                warnings: vec!["unsafe_paths".to_string()],
                needs_model: false,
            }
        }
    };

    let scan_root = &source.canonical_root;
    let has_bundle_signal = has_assistant_bundle_signal(scan_root);
    let root_structure = analyze_skill_structure(scan_root);
    if has_standard_entry_document(scan_root) {
        let Ok(resolved) = source.resolve_relative(Path::new(".")) else {
            return PackageDetection {
                package_kind: "single_skill".to_string(),
                detected_skills: vec![],
                warnings: with_unsafe_path_warning(package_warnings(has_bundle_signal, false)),
                needs_model: false,
            };
        };
        return PackageDetection {
            package_kind: "single_skill".to_string(),
            detected_skills: vec![detected_skill(".".to_string(), &resolved, root_structure)],
            warnings: package_warnings(has_bundle_signal, false),
            needs_model: false,
        };
    }

    let mut collected = collect_child_skills(&source, false);
    if collected.skills.is_empty() {
        let legacy = collect_child_skills(&source, true);
        collected.unsafe_paths |= legacy.unsafe_paths;
        collected.skills = legacy.skills;
    }
    let mut skills = collected.skills;
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
            warnings: package_detection_warnings(
                has_bundle_signal,
                false,
                collected.unsafe_paths,
                collected.scan_limited,
            ),
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
            warnings: package_detection_warnings(
                has_bundle_signal,
                false,
                collected.unsafe_paths,
                collected.scan_limited,
            ),
            needs_model: false,
        };
    }

    let mut warnings = package_detection_warnings(
        has_bundle_signal,
        true,
        collected.unsafe_paths,
        collected.scan_limited,
    );
    if root_structure.structure_status == "partial" {
        let Ok(resolved) = source.resolve_relative(Path::new(".")) else {
            return PackageDetection {
                package_kind: "single_skill".to_string(),
                detected_skills: vec![],
                warnings: with_unsafe_path_warning(warnings),
                needs_model: false,
            };
        };
        return PackageDetection {
            package_kind: "single_skill".to_string(),
            detected_skills: vec![detected_skill(".".to_string(), &resolved, root_structure)],
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

fn collect_child_skills(source: &SkillPackageSource, include_legacy: bool) -> CollectedSkills {
    if !include_legacy {
        return collect_standard_skills(source);
    }

    let mut skills = Vec::new();
    let mut unsafe_paths = false;
    let root = &source.canonical_root;
    let mut candidates = safe_immediate_dirs(source, root, &mut unsafe_paths);
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
            let Ok(path) = source.resolve_directory(&path) else {
                unsafe_paths = true;
                continue;
            };
            let direct_candidates = safe_immediate_dirs(source, &path, &mut unsafe_paths);
            for candidate in &direct_candidates {
                if !(has_standard_entry_document(candidate)
                    || include_legacy && has_legacy_entry_document(candidate))
                {
                    candidates.extend(safe_immediate_dirs(source, candidate, &mut unsafe_paths));
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
            match source.relative_path(&candidate) {
                Ok(relative_path) => {
                    skills.push(detected_skill(relative_path, &candidate, structure));
                }
                Err(_) => unsafe_paths = true,
            }
        }
    }
    CollectedSkills {
        skills,
        unsafe_paths,
        scan_limited: false,
    }
}

fn collect_standard_skills(source: &SkillPackageSource) -> CollectedSkills {
    let mut skills = Vec::new();
    let mut unsafe_paths = false;
    let mut scan_limited = false;
    let mut scanned_directories = 0usize;
    let mut queue = VecDeque::new();
    for candidate in safe_immediate_dirs(source, &source.canonical_root, &mut unsafe_paths) {
        queue.push_back((candidate, 1usize));
    }

    while let Some((candidate, depth)) = queue.pop_front() {
        scanned_directories += 1;
        if scanned_directories > MAX_DISCOVERY_DIRECTORIES {
            scan_limited = true;
            break;
        }
        if has_standard_entry_document(&candidate) {
            let structure = analyze_skill_structure(&candidate);
            match source.relative_path(&candidate) {
                Ok(relative_path) => {
                    skills.push(detected_skill(relative_path, &candidate, structure));
                }
                Err(_) => unsafe_paths = true,
            }
            continue;
        }
        if should_skip_discovery_children(&candidate) {
            continue;
        }
        if depth >= MAX_DISCOVERY_DEPTH {
            if !safe_immediate_dirs(source, &candidate, &mut unsafe_paths).is_empty() {
                scan_limited = true;
            }
            continue;
        }
        for child in safe_immediate_dirs(source, &candidate, &mut unsafe_paths) {
            queue.push_back((child, depth + 1));
        }
    }

    skills.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    skills.dedup_by(|left, right| left.relative_path == right.relative_path);
    CollectedSkills {
        skills,
        unsafe_paths,
        scan_limited,
    }
}

fn should_skip_discovery_children(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".git" | ".hg" | ".svn" | "node_modules" | "target" | "dist" | "build"
            )
        })
}

fn safe_immediate_dirs(
    source: &SkillPackageSource,
    path: &Path,
    unsafe_paths: &mut bool,
) -> Vec<PathBuf> {
    immediate_dirs(path)
        .into_iter()
        .filter_map(|candidate| match source.resolve_directory(&candidate) {
            Ok(resolved) => Some(resolved),
            Err(_) => {
                *unsafe_paths = true;
                None
            }
        })
        .collect()
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

fn detected_skill(
    relative_path: String,
    path: &Path,
    mut structure: SkillStructureInfo,
) -> DetectedSkill {
    for warning in analyze_skill_safety(path) {
        if !structure.structure_warnings.contains(&warning) {
            structure.structure_warnings.push(warning);
        }
    }
    DetectedSkill {
        relative_path,
        structure_status: structure.structure_status,
        title: structure.manifest_title,
        description: structure.manifest_description,
        features: structure.structure_features,
        warnings: structure.structure_warnings,
    }
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

fn package_detection_warnings(
    has_bundle_signal: bool,
    no_skills: bool,
    unsafe_paths: bool,
    scan_limited: bool,
) -> Vec<String> {
    let mut warnings = package_warnings(has_bundle_signal, no_skills);
    if unsafe_paths {
        warnings = with_unsafe_path_warning(warnings);
    }
    if scan_limited {
        warnings.push("scan_limit_reached".to_string());
    }
    warnings
}

fn with_unsafe_path_warning(mut warnings: Vec<String>) -> Vec<String> {
    if !warnings.iter().any(|warning| warning == "unsafe_paths") {
        warnings.push("unsafe_paths".to_string());
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
    fn detects_skills_nested_beneath_plugin_directories() {
        let root = test_dir("plugin-layout");
        let review = root.join("engineering/skills/code-review");
        let contract = root.join("legal/skills/review-contract");
        fs::create_dir_all(&review).unwrap();
        fs::create_dir_all(&contract).unwrap();
        fs::write(
            review.join("SKILL.md"),
            "---\nname: code-review\ndescription: Review code\n---\n",
        )
        .unwrap();
        fs::write(
            contract.join("SKILL.md"),
            "---\nname: review-contract\ndescription: Review contracts\n---\n",
        )
        .unwrap();

        let detection = detect_skill_package(&root);

        assert_eq!(detection.package_kind, "multi_skill");
        assert_eq!(detection.detected_skills.len(), 2);
        assert!(detection
            .detected_skills
            .iter()
            .any(|skill| skill.relative_path == "engineering/skills/code-review"));
        assert!(detection
            .detected_skills
            .iter()
            .any(|skill| skill.relative_path == "legal/skills/review-contract"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_when_skill_discovery_reaches_the_depth_limit() {
        let root = test_dir("depth-limit");
        let skill = root.join("one/two/three/four/five");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: too-deep\ndescription: Too deep\n---\n",
        )
        .unwrap();

        let detection = detect_skill_package(&root);

        assert!(detection.detected_skills.is_empty());
        assert!(detection
            .warnings
            .contains(&"scan_limit_reached".to_string()));
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
