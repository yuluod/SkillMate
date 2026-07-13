use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_ENTRY_DOCUMENT_BYTES: usize = 1024 * 1024;
const MAX_SAFETY_SCAN_FILES: usize = 2_000;
const MAX_SAFETY_SCAN_DEPTH: usize = 16;
const MAX_SAFETY_SCAN_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillEntryKind {
    Standard,
    LegacyFilename,
    ReadmeOnly,
    Missing,
}

pub fn detect_skill_entry(path: &Path) -> SkillEntryKind {
    if exact_child_file(path, "SKILL.md").is_some() {
        SkillEntryKind::Standard
    } else if exact_child_file(path, "skill.md").is_some() {
        SkillEntryKind::LegacyFilename
    } else if exact_child_file(path, "README.md").is_some()
        || exact_child_file(path, "readme.md").is_some()
    {
        SkillEntryKind::ReadmeOnly
    } else {
        SkillEntryKind::Missing
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillStructureInfo {
    pub structure_status: String,
    pub structure_features: Vec<String>,
    pub structure_warnings: Vec<String>,
    pub manifest_title: Option<String>,
    pub manifest_description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillInventoryInspection {
    pub structure: SkillStructureInfo,
    pub preview: String,
    pub version: Option<String>,
    pub content_size: u64,
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
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    tags: Vec<String>,
    compatibility: Option<String>,
    legacy_compatible: Vec<String>,
    license: Option<String>,
    has_metadata: bool,
    metadata_version: Option<String>,
    allowed_tools: Option<String>,
    frontmatter_present: bool,
    warnings: Vec<String>,
}

pub fn analyze_skill_structure(path: &Path) -> SkillStructureInfo {
    analyze_skill_documents(path).0
}

pub fn inspect_skill_for_inventory(path: &Path) -> SkillInventoryInspection {
    let (mut structure, preview, version) = analyze_skill_documents(path);
    let content = inspect_skill_content(path);
    for warning in content.warnings {
        push_unique(&mut structure.structure_warnings, &warning);
    }
    SkillInventoryInspection {
        structure,
        preview,
        version,
        content_size: content.size,
    }
}

fn analyze_skill_documents(path: &Path) -> (SkillStructureInfo, String, Option<String>) {
    let mut features = Vec::new();
    let mut warnings = Vec::new();
    let mut manifest = ParsedSkillManifest::default();

    if !path.exists() {
        return (
            SkillStructureInfo {
                structure_status: "nonstandard".to_string(),
                structure_features: features,
                structure_warnings: vec![
                    "path_missing".to_string(),
                    "missing_entry_document".to_string(),
                ],
                manifest_title: None,
                manifest_description: None,
            },
            String::new(),
            None,
        );
    }

    let skill_doc = read_named_file(path, &["SKILL.md"]);
    let legacy_skill_doc = read_named_file(path, &["skill.md"]);
    let readme_doc = read_named_file(path, &["README.md", "readme.md"]);
    if [&skill_doc, &legacy_skill_doc, &readme_doc]
        .into_iter()
        .flatten()
        .any(|(_, _, truncated)| *truncated)
    {
        push_unique(&mut warnings, "entry_document_truncated");
    }

    if skill_doc.is_some() {
        features.push("skill_md".to_string());
    }
    if legacy_skill_doc.is_some() {
        features.push("legacy_skill_md".to_string());
    }
    if readme_doc.is_some() {
        features.push("readme".to_string());
    }

    let support_dirs = ["references", "scripts", "assets"];
    for dir in support_dirs {
        if path.join(dir).is_dir() {
            features.push(dir.to_string());
        }
    }

    if let Some((_, content, _)) = &skill_doc {
        match parse_skill_frontmatter(content) {
            Ok(parsed) => {
                if parsed.frontmatter_present {
                    features.push("frontmatter".to_string());
                } else {
                    push_unique(&mut warnings, "missing_frontmatter");
                }
                if parsed.name.is_some() {
                    features.push("name".to_string());
                }
                if parsed.description.is_some() {
                    features.push("description".to_string());
                }
                if parsed.compatibility.is_some() {
                    features.push("compatibility".to_string());
                }
                if parsed.license.is_some() {
                    features.push("license".to_string());
                }
                if parsed.has_metadata {
                    features.push("metadata".to_string());
                }
                if parsed.allowed_tools.is_some() {
                    features.push("allowed_tools".to_string());
                }
                if !parsed.tags.is_empty() {
                    features.push("legacy_tags".to_string());
                }
                if !parsed.legacy_compatible.is_empty() {
                    features.push("legacy_compatible".to_string());
                    push_unique(&mut warnings, "legacy_compatible_field");
                }
                warnings.extend(parsed.warnings.iter().cloned());
                manifest = parsed;
            }
            Err(warning) => push_unique(&mut warnings, &warning),
        }

        validate_manifest_fields(path, &manifest, &mut warnings);
        if markdown_body_without_frontmatter(content)
            .lines()
            .all(|line| line.trim().is_empty())
        {
            features.push("metadata_only".to_string());
        }
    } else if legacy_skill_doc.is_some() {
        push_unique(&mut warnings, "legacy_skill_filename");
        push_unique(&mut warnings, "missing_skill_md");
    } else if readme_doc.is_some() {
        push_unique(&mut warnings, "readme_only");
        push_unique(&mut warnings, "missing_skill_md");
    }

    let structure_status =
        if skill_doc.is_some() && !warnings.iter().any(|code| is_spec_error(code)) {
            "complete"
        } else if skill_doc.is_some() || legacy_skill_doc.is_some() || readme_doc.is_some() {
            "partial"
        } else {
            push_unique(&mut warnings, "missing_entry_document");
            "nonstandard"
        }
        .to_string();

    let preview = skill_doc
        .as_ref()
        .or(legacy_skill_doc.as_ref())
        .or(readme_doc.as_ref())
        .map(|(_, content, _)| truncate_text(content, 3000))
        .unwrap_or_default();
    let version = manifest.metadata_version.clone();
    (
        SkillStructureInfo {
            structure_status,
            structure_features: features,
            structure_warnings: warnings,
            manifest_title: manifest.name.or(manifest.title),
            manifest_description: manifest.description,
        },
        preview,
        version,
    )
}

pub fn read_skill_preview(path: &Path) -> String {
    read_named_file(path, &["SKILL.md", "skill.md", "README.md", "readme.md"])
        .map(|(_, content, _)| truncate_text(&content, 3000))
        .unwrap_or_default()
}

#[cfg(test)]
pub fn read_skill_manifest_version(path: &Path) -> Option<String> {
    let (_, content, _) = read_named_file(path, &["SKILL.md"])?;
    parse_skill_frontmatter(&content)
        .ok()
        .and_then(|manifest| manifest.metadata_version)
}

pub fn validate_skill_structure(path: &Path) -> SkillValidationReport {
    let mut structure = analyze_skill_structure(path);
    for warning in analyze_skill_safety(path) {
        push_unique(&mut structure.structure_warnings, &warning);
    }
    let mut checks = Vec::new();

    push_check(
        &mut checks,
        "entry_document",
        if structure.structure_features.iter().any(|f| f == "skill_md") {
            "pass"
        } else if structure
            .structure_features
            .iter()
            .any(|f| f == "legacy_skill_md" || f == "readme")
        {
            "warning"
        } else {
            "fail"
        },
        if structure.structure_features.iter().any(|f| f == "skill_md") {
            "已识别标准入口文档"
        } else if structure
            .structure_features
            .iter()
            .any(|f| f == "legacy_skill_md")
        {
            "入口文件必须命名为 SKILL.md"
        } else if structure.structure_features.iter().any(|f| f == "readme") {
            "README 不是 Agent Skill 标准入口"
        } else {
            "缺少可识别入口文档"
        },
    );
    push_check(
        &mut checks,
        "frontmatter",
        if has_warning(&structure, "frontmatter_invalid")
            || has_warning(&structure, "missing_frontmatter")
        {
            "fail"
        } else if structure
            .structure_features
            .iter()
            .any(|f| f == "frontmatter")
        {
            "pass"
        } else {
            "warning"
        },
        if has_warning(&structure, "frontmatter_invalid") {
            "YAML frontmatter 解析失败"
        } else if has_warning(&structure, "missing_frontmatter") {
            "SKILL.md 缺少 YAML frontmatter"
        } else if structure
            .structure_features
            .iter()
            .any(|f| f == "frontmatter")
        {
            "已解析轻量 frontmatter"
        } else {
            "未识别 YAML frontmatter"
        },
    );
    push_check(
        &mut checks,
        "name",
        if structure.structure_features.iter().any(|f| f == "name")
            && !["invalid_name", "name_directory_mismatch", "missing_name"]
                .iter()
                .any(|code| has_warning(&structure, code))
        {
            "pass"
        } else {
            "fail"
        },
        if has_warning(&structure, "name_directory_mismatch") {
            "name 必须与 Skill 目录名一致"
        } else if has_warning(&structure, "invalid_name") {
            "name 只能包含小写字母、数字和单个连字符"
        } else if structure.structure_features.iter().any(|f| f == "name") {
            "name 符合 Agent Skills 规范"
        } else {
            "frontmatter 缺少必填 name"
        },
    );
    push_check(
        &mut checks,
        "description",
        if structure.manifest_description.is_some()
            && !["missing_description", "description_too_long"]
                .iter()
                .any(|code| has_warning(&structure, code))
        {
            "pass"
        } else {
            "fail"
        },
        if has_warning(&structure, "description_too_long") {
            "description 不能超过 1024 个字符"
        } else if structure.manifest_description.is_some() {
            "description 符合 Agent Skills 规范"
        } else {
            "frontmatter 缺少必填 description"
        },
    );
    push_check(
        &mut checks,
        "resources",
        "pass",
        if ["references", "scripts", "assets"]
            .iter()
            .any(|feature| structure.structure_features.iter().any(|f| f == feature))
        {
            "已识别可选资源目录"
        } else {
            "未提供可选资源目录，不影响规范状态"
        },
    );
    let compatibility_invalid = has_warning(&structure, "invalid_compatibility")
        || has_warning(&structure, "compatibility_too_long");
    push_check(
        &mut checks,
        "compatibility",
        if compatibility_invalid {
            "fail"
        } else {
            "pass"
        },
        if compatibility_invalid {
            "compatibility 必须是 1-500 个字符的字符串"
        } else if structure
            .structure_features
            .iter()
            .any(|f| f == "compatibility")
        {
            "已声明可选 compatibility 元数据"
        } else {
            "未声明可选 compatibility 元数据"
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

    for (code, message) in [
        (
            "contains_scripts",
            "包含 scripts 或可执行脚本，安装后可能被 Agent 运行",
        ),
        (
            "declares_dependencies",
            "包含依赖清单，运行前应核对第三方依赖",
        ),
        ("contains_symlinks", "包含软连接；复制安装时软连接会被跳过"),
        ("contains_hidden_files", "包含隐藏文件，请确认其用途"),
        ("references_network", "内容引用网络访问或下载命令"),
        ("references_environment", "内容引用环境变量或凭据"),
        (
            "safety_scan_incomplete",
            "安全扫描达到文件数或目录深度上限，结果可能不完整",
        ),
    ] {
        if structure.structure_warnings.iter().any(|item| item == code) {
            push_check(&mut checks, code, "warning", message);
        }
    }

    SkillValidationReport {
        structure_status: structure.structure_status,
        checks,
        warnings,
    }
}

pub fn analyze_skill_safety(path: &Path) -> Vec<String> {
    inspect_skill_content(path).warnings
}

#[derive(Default)]
struct SkillContentInspection {
    size: u64,
    warnings: Vec<String>,
}

fn inspect_skill_content(path: &Path) -> SkillContentInspection {
    let mut report = SafetyScan::default();
    scan_safety(path, &mut report, 0);
    let mut warnings = Vec::new();
    if path.join("scripts").is_dir() || report.executable_files {
        warnings.push("contains_scripts".to_string());
    }
    if report.dependency_manifests {
        warnings.push("declares_dependencies".to_string());
    }
    if report.symlinks {
        warnings.push("contains_symlinks".to_string());
    }
    if report.hidden_files {
        warnings.push("contains_hidden_files".to_string());
    }
    if report.network_references {
        warnings.push("references_network".to_string());
    }
    if report.environment_references {
        warnings.push("references_environment".to_string());
    }
    if report.incomplete {
        warnings.push("safety_scan_incomplete".to_string());
    }
    SkillContentInspection {
        size: report.total_size,
        warnings,
    }
}

#[derive(Default)]
struct SafetyScan {
    executable_files: bool,
    dependency_manifests: bool,
    symlinks: bool,
    hidden_files: bool,
    network_references: bool,
    environment_references: bool,
    visited_files: usize,
    scanned_bytes: u64,
    total_size: u64,
    incomplete: bool,
}

fn scan_safety(path: &Path, report: &mut SafetyScan, depth: usize) {
    if depth > MAX_SAFETY_SCAN_DEPTH || report.visited_files >= MAX_SAFETY_SCAN_FILES {
        report.incomplete = true;
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        report.incomplete = true;
        return;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" || name == STATE_FILE_NAME_FOR_SCAN {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&entry_path) else {
            continue;
        };
        if name.starts_with('.') {
            report.hidden_files = true;
        }
        if metadata.file_type().is_symlink() {
            report.symlinks = true;
            continue;
        }
        if metadata.is_dir() {
            scan_safety(&entry_path, report, depth + 1);
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        if report.visited_files >= MAX_SAFETY_SCAN_FILES {
            report.incomplete = true;
            break;
        }
        report.visited_files += 1;
        report.total_size = report.total_size.saturating_add(metadata.len());
        if is_executable_file(&entry_path, &metadata) {
            report.executable_files = true;
        }
        if is_dependency_manifest(&name) {
            report.dependency_manifests = true;
        }
        if metadata.len() <= 512 * 1024 && is_text_candidate(&entry_path) {
            if !reserve_safety_scan_bytes(report, metadata.len()) {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&entry_path) {
                let lowercase = content.to_ascii_lowercase();
                if ["https://", "http://", "curl ", "wget ", "invoke-webrequest"]
                    .iter()
                    .any(|needle| lowercase.contains(needle))
                {
                    report.network_references = true;
                }
                if ["process.env", "os.environ", "std::env", "${", "$env:"]
                    .iter()
                    .any(|needle| lowercase.contains(needle))
                {
                    report.environment_references = true;
                }
            }
        }
    }
}

fn reserve_safety_scan_bytes(report: &mut SafetyScan, bytes: u64) -> bool {
    if report.scanned_bytes.saturating_add(bytes) > MAX_SAFETY_SCAN_BYTES {
        report.incomplete = true;
        false
    } else {
        report.scanned_bytes = report.scanned_bytes.saturating_add(bytes);
        true
    }
}

const STATE_FILE_NAME_FOR_SCAN: &str = ".skillmate-state.json";

fn is_dependency_manifest(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "pyproject.toml"
            | "requirements.txt"
            | "uv.lock"
            | "cargo.toml"
            | "cargo.lock"
            | "go.mod"
            | "go.sum"
    )
}

fn is_text_candidate(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "md" | "txt"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "js"
            | "mjs"
            | "ts"
            | "tsx"
            | "py"
            | "sh"
            | "bash"
            | "zsh"
            | "ps1"
            | "bat"
            | "cmd"
            | "rs"
    )
}

#[cfg(unix)]
fn is_executable_file(_path: &Path, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(windows)]
fn is_executable_file(path: &Path, _metadata: &fs::Metadata) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "exe" | "cmd" | "bat" | "ps1"
    )
}

fn push_check(checks: &mut Vec<SkillValidationCheck>, code: &str, status: &str, message: &str) {
    checks.push(SkillValidationCheck {
        code: code.to_string(),
        status: status.to_string(),
        message: message.to_string(),
    });
}

fn read_named_file(path: &Path, names: &[&str]) -> Option<(String, String, bool)> {
    for name in names {
        let Some(file_path) = exact_child_file(path, name) else {
            continue;
        };
        if let Ok((content, truncated)) = read_limited_text(&file_path, MAX_ENTRY_DOCUMENT_BYTES) {
            return Some(((*name).to_string(), content, truncated));
        }
    }
    None
}

fn read_limited_text(path: &Path, max_bytes: usize) -> Result<(String, bool), String> {
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    file.take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let truncated = bytes.len() > max_bytes;
    if truncated {
        bytes.truncate(max_bytes);
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

fn exact_child_file(path: &Path, name: &str) -> Option<PathBuf> {
    fs::read_dir(path)
        .ok()?
        .flatten()
        .find(|entry| {
            entry.file_name() == OsStr::new(name)
                && entry
                    .file_type()
                    .map(|file_type| file_type.is_file())
                    .unwrap_or(false)
        })
        .map(|entry| entry.path())
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
        return true;
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

fn parse_skill_frontmatter(content: &str) -> Result<ParsedSkillManifest, String> {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Ok(ParsedSkillManifest::default());
    }

    let mut yaml = Vec::new();
    for raw_line in lines {
        if raw_line.trim() == "---" {
            return parse_skill_manifest_yaml(&yaml.join("\n"));
        }
        yaml.push(raw_line);
    }
    Err("frontmatter_invalid".to_string())
}

fn parse_skill_manifest_yaml(yaml: &str) -> Result<ParsedSkillManifest, String> {
    let value: Value = serde_yaml::from_str(yaml).map_err(|_| "frontmatter_invalid".to_string())?;
    let mapping = match value {
        Value::Mapping(mapping) => mapping,
        Value::Null => Mapping::new(),
        _ => return Err("frontmatter_invalid".to_string()),
    };
    let mut manifest = ParsedSkillManifest {
        frontmatter_present: true,
        ..Default::default()
    };
    manifest.name = read_string_field(&mapping, "name", "invalid_name", &mut manifest.warnings);
    manifest.title = read_string_field(&mapping, "title", "invalid_title", &mut manifest.warnings);
    manifest.description = read_string_field(
        &mapping,
        "description",
        "invalid_description",
        &mut manifest.warnings,
    );
    manifest.compatibility = read_string_field(
        &mapping,
        "compatibility",
        "invalid_compatibility",
        &mut manifest.warnings,
    );
    manifest.license = read_string_field(
        &mapping,
        "license",
        "invalid_license",
        &mut manifest.warnings,
    );
    manifest.allowed_tools = read_string_field(
        &mapping,
        "allowed-tools",
        "invalid_allowed_tools",
        &mut manifest.warnings,
    );
    manifest.tags = read_legacy_list_field(&mapping, "tags");
    manifest.legacy_compatible = read_legacy_list_field(&mapping, "compatible");
    if manifest.legacy_compatible.is_empty() {
        manifest.legacy_compatible = read_legacy_list_field(&mapping, "compatible_with");
    }
    if let Some(metadata) = mapping.get(Value::String("metadata".to_string())) {
        manifest.has_metadata = true;
        if !is_string_mapping(metadata) {
            push_unique(&mut manifest.warnings, "invalid_metadata");
        } else if let Value::Mapping(metadata) = metadata {
            manifest.metadata_version = metadata
                .get(Value::String("version".to_string()))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }
    Ok(manifest)
}

fn read_string_field(
    mapping: &Mapping,
    key: &str,
    invalid_code: &str,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let value = mapping.get(Value::String(key.to_string()))?;
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => {
            push_unique(warnings, invalid_code);
            None
        }
    }
}

fn read_legacy_list_field(mapping: &Mapping, key: &str) -> Vec<String> {
    let Some(value) = mapping.get(Value::String(key.to_string())) else {
        return Vec::new();
    };
    match value {
        Value::String(value) => value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
        Value::Sequence(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn is_string_mapping(value: &Value) -> bool {
    let Value::Mapping(mapping) = value else {
        return false;
    };
    mapping
        .iter()
        .all(|(key, value)| key.as_str().is_some() && value.as_str().is_some())
}

fn validate_manifest_fields(
    path: &Path,
    manifest: &ParsedSkillManifest,
    warnings: &mut Vec<String>,
) {
    if !manifest.frontmatter_present {
        return;
    }
    match manifest.name.as_deref() {
        Some(name) => {
            if !is_valid_skill_name(name) {
                push_unique(warnings, "invalid_name");
            } else if path.file_name().and_then(|value| value.to_str()) != Some(name) {
                push_unique(warnings, "name_directory_mismatch");
            }
        }
        None => push_unique(warnings, "missing_name"),
    }
    match manifest.description.as_deref() {
        Some(description) if description.chars().count() > 1024 => {
            push_unique(warnings, "description_too_long")
        }
        Some(_) => {}
        None => push_unique(warnings, "missing_description"),
    }
    if let Some(compatibility) = manifest.compatibility.as_deref() {
        let length = compatibility.chars().count();
        if length == 0 || length > 500 {
            push_unique(warnings, "compatibility_too_long");
        }
    }
}

fn is_valid_skill_name(name: &str) -> bool {
    let length = name.len();
    (1..=64).contains(&length)
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == b'-')
}

fn is_spec_error(code: &str) -> bool {
    matches!(
        code,
        "frontmatter_invalid"
            | "missing_frontmatter"
            | "missing_name"
            | "invalid_name"
            | "name_directory_mismatch"
            | "missing_description"
            | "invalid_description"
            | "description_too_long"
            | "invalid_compatibility"
            | "compatibility_too_long"
            | "invalid_license"
            | "invalid_metadata"
            | "invalid_allowed_tools"
    )
}

fn has_warning(structure: &SkillStructureInfo, code: &str) -> bool {
    structure.structure_warnings.iter().any(|item| item == code)
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|item| item == value) {
        values.push(value.to_string());
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

    fn is_windows_symlink_permission_error(error: &std::io::Error) -> bool {
        cfg!(windows)
            && (error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314))
    }

    #[cfg(unix)]
    fn create_file_symlink(source: &Path, target: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(source, target)
    }

    #[cfg(windows)]
    fn create_file_symlink(source: &Path, target: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(source, target)
    }

    fn create_file_symlink_or_skip(source: &Path, target: &Path) -> bool {
        match create_file_symlink(source, target) {
            Ok(()) => true,
            Err(error) if is_windows_symlink_permission_error(&error) => {
                eprintln!("跳过 symlink 单测：当前 Windows 环境不允许创建符号链接：{error}");
                false
            }
            Err(error) => panic!("创建测试符号链接失败: {error}"),
        }
    }

    #[test]
    fn truncate_text_keeps_utf8_boundary() {
        assert_eq!(truncate_text("技能说明", 5), "技...\n(截断)");
        assert_eq!(truncate_text("abc", 5), "abc");
    }

    #[test]
    fn symlinked_skill_entry_is_not_a_standard_document() {
        let root = test_dir("symlink-entry");
        let skill = root.join("writer");
        let outside = root.join("outside.md");
        write_text(&outside, "---\nname: writer\ndescription: 写作\n---\n");
        std::fs::create_dir_all(&skill).unwrap();
        if !create_file_symlink_or_skip(&outside, &skill.join("SKILL.md")) {
            let _ = std::fs::remove_dir_all(root);
            return;
        }

        assert_ne!(detect_skill_entry(&skill), SkillEntryKind::Standard);
        assert_ne!(analyze_skill_structure(&skill).structure_status, "complete");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unsafe_path_scan_fails_closed_after_depth_limit() {
        let root = test_dir("unsafe-depth");
        std::fs::create_dir_all(&root).unwrap();
        let canonical = root.canonicalize().unwrap();

        assert!(path_has_unsafe_symlink(&root, &canonical, 7));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn marks_standard_skill_as_complete() {
        let root = test_dir("complete");
        let base = root.join("writing-skill");
        write_text(
            &base.join("SKILL.md"),
            "---\nname: writing-skill\ndescription: 帮助整理文稿\ncompatibility: Designed for Codex\nlicense: AGPL-3.0\nmetadata:\n  author: skillmate\nallowed-tools: Read Write\n---\n\n# 写作 Skill\n\n处理文稿。",
        );

        let structure = analyze_skill_structure(&base);

        assert_eq!(structure.structure_status, "complete");
        assert!(structure
            .structure_features
            .contains(&"skill_md".to_string()));
        assert!(structure
            .structure_features
            .contains(&"frontmatter".to_string()));
        assert_eq!(structure.manifest_title, Some("writing-skill".to_string()));
        assert_eq!(
            structure.manifest_description,
            Some("帮助整理文稿".to_string())
        );

        assert!(structure
            .structure_features
            .contains(&"compatibility".to_string()));
        assert!(structure
            .structure_features
            .contains(&"metadata".to_string()));

        std::fs::remove_dir_all(root).ok();
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
        let root = test_dir("dirs");
        let base = root.join("resource-skill");
        write_text(
            &base.join("SKILL.md"),
            "---\nname: resource-skill\ndescription: 读取可选资源目录\n---\n\n# Skill\n\n说明内容。",
        );
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
        assert!(structure.structure_warnings.is_empty());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn invalid_frontmatter_warns_without_blocking_scan() {
        let base = test_dir("frontmatter");
        write_text(
            &base.join("SKILL.md"),
            "---\nname: Broken\ninvalid line\n---\n\n# Skill\n\n说明内容。",
        );

        let structure = analyze_skill_structure(&base);

        assert_eq!(structure.structure_status, "partial");
        assert!(structure
            .structure_warnings
            .contains(&"frontmatter_invalid".to_string()));
        assert_eq!(structure.manifest_title, None);

        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn validation_report_explains_structure_checks() {
        let root = test_dir("validation");
        let base = root.join("writer");
        write_text(
            &base.join("SKILL.md"),
            "---\nname: writer\ndescription: 帮助整理文稿\ncompatibility: Designed for Codex\n---\n\n说明内容。",
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

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn requires_exact_skill_filename_and_required_frontmatter() {
        let root = test_dir("strict");
        let legacy = root.join("legacy-skill");
        write_text(
            &legacy.join("skill.md"),
            "---\nname: legacy-skill\ndescription: 旧入口\n---\n",
        );
        let missing_description = root.join("missing-description");
        write_text(
            &missing_description.join("SKILL.md"),
            "---\nname: missing-description\n---\n",
        );

        let legacy_structure = analyze_skill_structure(&legacy);
        let missing_structure = analyze_skill_structure(&missing_description);

        assert_eq!(legacy_structure.structure_status, "partial");
        assert!(legacy_structure
            .structure_warnings
            .contains(&"legacy_skill_filename".to_string()));
        assert_eq!(missing_structure.structure_status, "partial");
        assert!(missing_structure
            .structure_warnings
            .contains(&"missing_description".to_string()));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn optional_resource_directories_do_not_affect_validity() {
        let root = test_dir("optional-resources");
        let base = root.join("minimal-skill");
        write_text(
            &base.join("SKILL.md"),
            "---\nname: minimal-skill\ndescription: 最小合法 Skill\n---\n",
        );

        let structure = analyze_skill_structure(&base);

        assert_eq!(structure.structure_status, "complete");
        assert!(!structure
            .structure_warnings
            .contains(&"missing_support_dirs".to_string()));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn parses_multiline_yaml_description() {
        let root = test_dir("yaml");
        let base = root.join("yaml-skill");
        write_text(
            &base.join("SKILL.md"),
            "---\nname: yaml-skill\ndescription: >\n  处理带有多行描述的\n  标准 Skill\n---\n",
        );

        let structure = analyze_skill_structure(&base);

        assert_eq!(structure.structure_status, "complete");
        assert_eq!(
            structure.manifest_description,
            Some("处理带有多行描述的 标准 Skill\n".trim().to_string())
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reads_version_from_manifest_metadata() {
        let root = test_dir("manifest-version");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nname: manifest-version\ndescription: 测试版本\nmetadata:\n  version: 1.2.3\n---\n",
        )
        .unwrap();

        assert_eq!(read_skill_manifest_version(&root).as_deref(), Some("1.2.3"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn validation_report_fails_unsafe_symlink() {
        let base = test_dir("unsafe");
        let outside = test_dir("outside").join("secret.txt");
        write_text(&base.join("SKILL.md"), "# Skill\n\n说明内容。");
        write_text(&outside, "secret");

        if !create_file_symlink_or_skip(&outside, &base.join("secret-link")) {
            std::fs::remove_dir_all(base).ok();
            if let Some(parent) = outside.parent() {
                std::fs::remove_dir_all(parent).ok();
            }
            return;
        }

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

    #[test]
    fn safety_scan_reports_scripts_dependencies_network_and_environment() {
        let root = test_dir("safety");
        let base = root.join("network-skill");
        write_text(
            &base.join("SKILL.md"),
            "---\nname: network-skill\ndescription: 网络测试\n---\n",
        );
        write_text(
            &base.join("scripts/run.sh"),
            "curl https://example.com -H \"Authorization: $TOKEN\"",
        );
        write_text(&base.join("package.json"), "{\"dependencies\":{}}");
        write_text(&base.join(".config"), "hidden");

        let warnings = analyze_skill_safety(&base);
        let report = validate_skill_structure(&base);

        assert!(warnings.contains(&"contains_scripts".to_string()));
        assert!(warnings.contains(&"declares_dependencies".to_string()));
        assert!(warnings.contains(&"contains_hidden_files".to_string()));
        assert!(warnings.contains(&"references_network".to_string()));
        assert!(report
            .checks
            .iter()
            .any(|check| check.code == "contains_scripts" && check.status == "warning"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn entry_document_read_is_bounded_and_reported() {
        let base = test_dir("large-entry-document");
        fs::create_dir_all(&base).unwrap();
        fs::write(
            base.join("SKILL.md"),
            vec![b'a'; MAX_ENTRY_DOCUMENT_BYTES + 64],
        )
        .unwrap();

        let (_, content, truncated) = read_named_file(&base, &["SKILL.md"]).unwrap();
        let structure = analyze_skill_structure(&base);

        assert!(truncated);
        assert_eq!(content.len(), MAX_ENTRY_DOCUMENT_BYTES);
        assert!(structure
            .structure_warnings
            .contains(&"entry_document_truncated".to_string()));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn safety_scan_reports_incomplete_depth_limited_result() {
        let base = test_dir("safety-depth-limit");
        let mut nested = base.clone();
        for index in 0..=MAX_SAFETY_SCAN_DEPTH + 1 {
            nested = nested.join(format!("level-{}", index));
        }
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("script.sh"), "curl https://example.com").unwrap();

        let warnings = analyze_skill_safety(&base);

        assert!(warnings.contains(&"safety_scan_incomplete".to_string()));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn safety_scan_byte_budget_fails_closed() {
        let mut report = SafetyScan {
            scanned_bytes: MAX_SAFETY_SCAN_BYTES - 1,
            ..SafetyScan::default()
        };

        assert!(!reserve_safety_scan_bytes(&mut report, 2));
        assert!(report.incomplete);
        assert_eq!(report.scanned_bytes, MAX_SAFETY_SCAN_BYTES - 1);
    }
}
