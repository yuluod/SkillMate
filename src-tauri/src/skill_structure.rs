use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillStructureInfo {
    pub structure_status: String,
    pub structure_features: Vec<String>,
    pub structure_warnings: Vec<String>,
    pub manifest_title: Option<String>,
    pub manifest_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SkillValidationCheck {
    pub code: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SkillValidationReport {
    pub structure_status: String,
    pub checks: Vec<SkillValidationCheck>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ParsedSkillManifest {
    title: Option<String>,
    description: Option<String>,
    tags: Vec<String>,
    compatible: Vec<String>,
}

pub fn analyze_skill_structure(path: &Path) -> SkillStructureInfo {
    let mut features = Vec::new();
    let mut warnings = Vec::new();
    let mut manifest = ParsedSkillManifest::default();

    if !path.exists() {
        return SkillStructureInfo {
            structure_status: "nonstandard".to_string(),
            structure_features: features,
            structure_warnings: vec![
                "path_missing".to_string(),
                "missing_entry_document".to_string(),
            ],
            manifest_title: None,
            manifest_description: None,
        };
    }

    let skill_doc = read_named_file(path, &["SKILL.md", "skill.md"]);
    let readme_doc = read_named_file(path, &["README.md", "readme.md"]);

    if skill_doc.is_some() {
        features.push("skill_md".to_string());
    }
    if readme_doc.is_some() {
        features.push("readme".to_string());
    }

    let support_dirs = ["references", "scripts", "assets"];
    let mut support_count = 0usize;
    for dir in support_dirs {
        if path.join(dir).is_dir() {
            features.push(dir.to_string());
            support_count += 1;
        }
    }
    if support_count == 0 {
        warnings.push("missing_support_dirs".to_string());
    }

    let mut has_standard_content = false;
    if let Some((_, content)) = &skill_doc {
        match parse_skill_frontmatter(content) {
            Ok(parsed) => {
                if parsed.title.is_some()
                    || parsed.description.is_some()
                    || !parsed.tags.is_empty()
                    || !parsed.compatible.is_empty()
                {
                    features.push("frontmatter".to_string());
                }
                if !parsed.compatible.is_empty() {
                    features.push("compatible".to_string());
                }
                manifest = parsed;
            }
            Err(warning) => warnings.push(warning),
        }

        let body = markdown_body_without_frontmatter(content);
        has_standard_content =
            body.lines().any(|line| !line.trim().is_empty()) || manifest.description.is_some();
        if !has_standard_content {
            warnings.push("empty_skill_md".to_string());
        }
    }

    let structure_status = if skill_doc.is_some() && has_standard_content {
        "complete"
    } else if skill_doc.is_some() || readme_doc.is_some() {
        if skill_doc.is_none() {
            warnings.push("missing_skill_md".to_string());
        }
        "partial"
    } else {
        warnings.push("missing_entry_document".to_string());
        "nonstandard"
    }
    .to_string();

    SkillStructureInfo {
        structure_status,
        structure_features: features,
        structure_warnings: warnings,
        manifest_title: manifest.title,
        manifest_description: manifest.description,
    }
}

pub fn read_skill_preview(path: &Path) -> String {
    read_named_file(path, &["SKILL.md", "skill.md", "README.md", "readme.md"])
        .map(|(_, content)| truncate_text(&content, 3000))
        .unwrap_or_default()
}

pub fn validate_skill_structure(path: &Path) -> SkillValidationReport {
    let structure = analyze_skill_structure(path);
    let mut checks = Vec::new();

    push_check(
        &mut checks,
        "entry_document",
        if structure.structure_features.iter().any(|f| f == "skill_md") {
            "pass"
        } else if structure.structure_features.iter().any(|f| f == "readme") {
            "warning"
        } else {
            "fail"
        },
        if structure.structure_features.iter().any(|f| f == "skill_md") {
            "已识别标准入口文档"
        } else if structure.structure_features.iter().any(|f| f == "readme") {
            "仅识别到 README 入口"
        } else {
            "缺少可识别入口文档"
        },
    );
    push_check(
        &mut checks,
        "frontmatter",
        if structure
            .structure_warnings
            .iter()
            .any(|w| w == "frontmatter_invalid")
        {
            "warning"
        } else if structure
            .structure_features
            .iter()
            .any(|f| f == "frontmatter")
        {
            "pass"
        } else {
            "warning"
        },
        if structure
            .structure_warnings
            .iter()
            .any(|w| w == "frontmatter_invalid")
        {
            "frontmatter 解析失败，已继续扫描"
        } else if structure
            .structure_features
            .iter()
            .any(|f| f == "frontmatter")
        {
            "已解析轻量 frontmatter"
        } else {
            "未声明 frontmatter 元数据"
        },
    );
    push_check(
        &mut checks,
        "description",
        if structure.manifest_description.is_some()
            || structure.structure_status == "complete"
            || structure.structure_features.iter().any(|f| f == "readme")
        {
            "pass"
        } else {
            "warning"
        },
        if structure.manifest_description.is_some() {
            "已识别说明描述"
        } else if structure.structure_status == "complete"
            || structure.structure_features.iter().any(|f| f == "readme")
        {
            "入口文档包含说明内容"
        } else {
            "缺少说明内容"
        },
    );
    push_check(
        &mut checks,
        "resources",
        if ["references", "scripts", "assets"]
            .iter()
            .any(|feature| structure.structure_features.iter().any(|f| f == feature))
        {
            "pass"
        } else {
            "warning"
        },
        if ["references", "scripts", "assets"]
            .iter()
            .any(|feature| structure.structure_features.iter().any(|f| f == feature))
        {
            "已识别推荐资源目录"
        } else {
            "未识别 references/scripts/assets 资源目录"
        },
    );
    push_check(
        &mut checks,
        "compatibility",
        if structure
            .structure_features
            .iter()
            .any(|f| f == "compatible")
        {
            "pass"
        } else {
            "warning"
        },
        if structure
            .structure_features
            .iter()
            .any(|f| f == "compatible")
        {
            "已声明兼容对象"
        } else {
            "未声明 compatible 元数据"
        },
    );
    let has_unsafe = has_unsafe_paths(path);
    push_check(
        &mut checks,
        "unsafe_paths",
        if has_unsafe { "fail" } else { "pass" },
        if has_unsafe {
            "发现可能越界或异常的路径"
        } else {
            "未发现异常路径"
        },
    );

    let mut warnings = structure.structure_warnings.clone();
    if has_unsafe {
        warnings.push("unsafe_paths".to_string());
    }

    SkillValidationReport {
        structure_status: structure.structure_status,
        checks,
        warnings,
    }
}

fn push_check(checks: &mut Vec<SkillValidationCheck>, code: &str, status: &str, message: &str) {
    checks.push(SkillValidationCheck {
        code: code.to_string(),
        status: status.to_string(),
        message: message.to_string(),
    });
}

fn read_named_file(path: &Path, names: &[&str]) -> Option<(String, String)> {
    for name in names {
        let file_path = path.join(name);
        if file_path.is_file() {
            if let Ok(content) = fs::read_to_string(&file_path) {
                return Some(((*name).to_string(), content));
            }
        }
    }
    None
}

fn has_unsafe_paths(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(root) = path.canonicalize() else {
        return true;
    };
    path_has_unsafe_symlink(path, &root, 0)
}

fn path_has_unsafe_symlink(path: &Path, root: &Path, depth: usize) -> bool {
    if depth > 6 {
        return false;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&entry_path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            match entry_path.canonicalize() {
                Ok(target) if target.starts_with(root) => {}
                _ => return true,
            }
        } else if metadata.is_dir() && path_has_unsafe_symlink(&entry_path, root, depth + 1) {
            return true;
        }
    }
    false
}

fn clean_frontmatter_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn parse_frontmatter_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(trimmed);
    inner
        .split(',')
        .map(clean_frontmatter_value)
        .filter(|value| !value.is_empty())
        .collect()
}

fn parse_skill_frontmatter(content: &str) -> Result<ParsedSkillManifest, String> {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Ok(ParsedSkillManifest::default());
    }

    let mut manifest = ParsedSkillManifest::default();
    let mut closed = false;
    for raw_line in lines {
        let line = raw_line.trim();
        if line == "---" {
            closed = true;
            break;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err("frontmatter_invalid".to_string());
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "title" | "name" => {
                if manifest.title.is_none() {
                    let parsed = clean_frontmatter_value(value);
                    if !parsed.is_empty() {
                        manifest.title = Some(parsed);
                    }
                }
            }
            "description" => {
                let parsed = clean_frontmatter_value(value);
                if !parsed.is_empty() {
                    manifest.description = Some(parsed);
                }
            }
            "tags" => {
                manifest.tags = parse_frontmatter_list(value);
            }
            "compatible" | "compatible_with" => {
                manifest.compatible = parse_frontmatter_list(value);
            }
            _ => {}
        }
    }

    if closed {
        Ok(manifest)
    } else {
        Err("frontmatter_invalid".to_string())
    }
}

fn markdown_body_without_frontmatter(content: &str) -> String {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return content.to_string();
    }

    let mut body = Vec::new();
    let mut after_frontmatter = false;
    for line in lines {
        if after_frontmatter {
            body.push(line);
        } else if line.trim() == "---" {
            after_frontmatter = true;
        }
    }
    if after_frontmatter {
        body.join("\n")
    } else {
        content.to_string()
    }
}

fn truncate_text(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }
    let boundary = if content.is_char_boundary(max_bytes) {
        max_bytes
    } else {
        content
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|index| *index < max_bytes)
            .last()
            .unwrap_or(0)
    };
    format!("{}...\n(截断)", &content[..boundary])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("skillmate-structure-test-{}-{}", name, stamp))
    }

    fn write_text(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn truncate_text_keeps_utf8_boundary() {
        assert_eq!(truncate_text("技能说明", 5), "技...\n(截断)");
        assert_eq!(truncate_text("abc", 5), "abc");
    }

    #[test]
    fn marks_standard_skill_as_complete() {
        let base = test_dir("complete");
        write_text(
            &base.join("SKILL.md"),
            "---\nname: 写作 Skill\ndescription: 帮助整理文稿\ntags: [writing, ai]\ncompatible: [Codex]\n---\n\n# 写作 Skill\n\n处理文稿。",
        );

        let structure = analyze_skill_structure(&base);

        assert_eq!(structure.structure_status, "complete");
        assert!(structure
            .structure_features
            .contains(&"skill_md".to_string()));
        assert!(structure
            .structure_features
            .contains(&"frontmatter".to_string()));
        assert_eq!(structure.manifest_title, Some("写作 Skill".to_string()));
        assert_eq!(
            structure.manifest_description,
            Some("帮助整理文稿".to_string())
        );

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn marks_readme_only_skill_as_partial() {
        let base = test_dir("readme");
        write_text(&base.join("README.md"), "# 旧格式 Skill\n\n说明内容。");

        let structure = analyze_skill_structure(&base);

        assert_eq!(structure.structure_status, "partial");
        assert!(structure.structure_features.contains(&"readme".to_string()));
        assert!(structure
            .structure_warnings
            .contains(&"missing_skill_md".to_string()));
        assert_eq!(read_skill_preview(&base), "# 旧格式 Skill\n\n说明内容。");

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn marks_directory_without_entry_as_nonstandard() {
        let base = test_dir("nonstandard");
        std::fs::create_dir_all(&base).unwrap();

        let structure = analyze_skill_structure(&base);

        assert_eq!(structure.structure_status, "nonstandard");
        assert!(structure
            .structure_warnings
            .contains(&"missing_entry_document".to_string()));

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn detects_support_directories() {
        let base = test_dir("dirs");
        write_text(&base.join("SKILL.md"), "# Skill\n\n说明内容。");
        std::fs::create_dir_all(base.join("references")).unwrap();
        std::fs::create_dir_all(base.join("scripts")).unwrap();
        std::fs::create_dir_all(base.join("assets")).unwrap();

        let structure = analyze_skill_structure(&base);

        assert_eq!(structure.structure_status, "complete");
        assert!(structure
            .structure_features
            .contains(&"references".to_string()));
        assert!(structure
            .structure_features
            .contains(&"scripts".to_string()));
        assert!(structure.structure_features.contains(&"assets".to_string()));
        assert!(!structure
            .structure_warnings
            .contains(&"missing_support_dirs".to_string()));

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn invalid_frontmatter_warns_without_blocking_scan() {
        let base = test_dir("frontmatter");
        write_text(
            &base.join("SKILL.md"),
            "---\nname: Broken\ninvalid line\n---\n\n# Skill\n\n说明内容。",
        );

        let structure = analyze_skill_structure(&base);

        assert_eq!(structure.structure_status, "complete");
        assert!(structure
            .structure_warnings
            .contains(&"frontmatter_invalid".to_string()));
        assert_eq!(structure.manifest_title, None);

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn validation_report_explains_structure_checks() {
        let base = test_dir("validation");
        write_text(
            &base.join("SKILL.md"),
            "---\nname: 写作\ncompatible: [Codex]\n---\n\n说明内容。",
        );

        let report = validate_skill_structure(&base);

        assert_eq!(report.structure_status, "complete");
        assert!(report
            .checks
            .iter()
            .any(|check| check.code == "entry_document" && check.status == "pass"));
        assert!(report
            .checks
            .iter()
            .any(|check| check.code == "compatibility" && check.status == "pass"));

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn validation_report_fails_unsafe_symlink() {
        let base = test_dir("unsafe");
        let outside = test_dir("outside").join("secret.txt");
        write_text(&base.join("SKILL.md"), "# Skill\n\n说明内容。");
        write_text(&outside, "secret");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, base.join("secret-link")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside, base.join("secret-link")).unwrap();

        let report = validate_skill_structure(&base);

        assert!(report
            .checks
            .iter()
            .any(|check| check.code == "unsafe_paths" && check.status == "fail"));
        assert!(report.warnings.contains(&"unsafe_paths".to_string()));

        std::fs::remove_dir_all(base).ok();
        if let Some(parent) = outside.parent() {
            std::fs::remove_dir_all(parent).ok();
        }
    }
}
