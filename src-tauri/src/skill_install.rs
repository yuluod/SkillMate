use crate::app_core::{expand_path, generate_id, run_command_with_timeout};
use crate::install_policy::InstallPolicyDecision;
use crate::managed_state::mark_managed_skill;
use crate::operation_plan::{operation_plan_token, StableHash};
pub use crate::skill_install_source::{
    detect_install_source_rules, install_target_name, is_git_install_source,
    parse_git_install_spec, sanitize_git_locator, sanitize_git_remote_url, validate_git_reference,
    validate_git_repo_locator, GitInstallSpec, InstallDetection,
};
use crate::skill_package::{
    detect_skill_package, DetectedSkill, PackageDetection, SkillPackageSource,
};
use crate::skill_structure::{
    analyze_skill_structure, inspect_skill_for_inventory, SkillStructureInfo,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallResult {
    pub success: bool,
    pub message: String,
    pub output: String,
    pub structure_status: String,
    pub structure_features: Vec<String>,
    pub structure_warnings: Vec<String>,
    pub manifest_title: Option<String>,
    pub manifest_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstallPreview {
    pub can_install: bool,
    pub can_apply: bool,
    pub message: String,
    pub target_name: String,
    pub target_path: String,
    pub source_kind: String,
    pub package_detection: PackageDetection,
    pub target_actions: Vec<PreviewAction>,
    pub conflicts: Vec<PreviewConflict>,
    pub structure_status: String,
    pub structure_features: Vec<String>,
    pub structure_warnings: Vec<String>,
    pub manifest_title: Option<String>,
    pub manifest_description: Option<String>,
    pub install_policy: InstallPolicyDecision,
    pub plan_token: String,
    pub source_digest: String,
    pub resolved_ref: String,
}

#[derive(Serialize)]
struct InstallPlanEnvelope<'a> {
    package: &'a str,
    assistant_name: &'a str,
    install_mode: &'a str,
    project_path: &'a str,
    preview: &'a InstallPreview,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct PreviewAction {
    pub action: String,
    pub source: String,
    pub target: String,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct PreviewConflict {
    pub target: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct GitInstallOutcome {
    pub structure: SkillStructureInfo,
    pub installed_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSnapshotProbe {
    pub latest_ref: String,
    pub source_digest: String,
}

#[derive(Debug, Clone)]
pub struct GitSnapshotProbeRequest {
    pub key: String,
    pub origin_locator: String,
    pub resolved_locator: String,
    pub tracking_ref: String,
}

const MAX_INSTALL_FILES: usize = 10_000;
const MAX_INSTALL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_INSTALL_DEPTH: usize = 32;

pub struct PreparedGitInstall {
    spec: GitInstallSpec,
    checkout: TempGitSource,
    package_detection: PackageDetection,
    source_digest: String,
    resolved_ref: String,
}

struct TempGitSource {
    temp_root: PathBuf,
    repo_path: PathBuf,
    source_path: PathBuf,
}

impl TempGitSource {
    fn prepare(spec: &GitInstallSpec) -> Result<Self, String> {
        let temp_root =
            std::env::temp_dir().join(format!("skillmate-git-source-{}", generate_id()));
        let repo_path = temp_root.join(&spec.target_name);
        let mut checkout = Self {
            temp_root,
            repo_path,
            source_path: PathBuf::new(),
        };
        fs::create_dir_all(&checkout.temp_root).map_err(|error| error.to_string())?;
        clone_git_install_spec(spec, &checkout.repo_path)?;
        checkout.source_path =
            resolve_git_source_path(&checkout.repo_path, spec.subdir.as_deref())?;
        Ok(checkout)
    }

    fn source_path(&self) -> &Path {
        &self.source_path
    }

    fn repo_path(&self) -> &Path {
        &self.repo_path
    }
}

impl Drop for TempGitSource {
    fn drop(&mut self) {
        if self.temp_root.exists() {
            let _ = fs::remove_dir_all(&self.temp_root);
        }
    }
}

impl PreparedGitInstall {
    pub fn prepare(package: &str) -> Result<Self, String> {
        Self::from_spec(parse_git_install_spec(package)?)
    }

    fn from_spec(spec: GitInstallSpec) -> Result<Self, String> {
        ensure_git_available()?;
        let checkout = TempGitSource::prepare(&spec)?;
        let package_detection = detect_skill_package(checkout.source_path());
        let source_digest =
            package_content_fingerprint(checkout.source_path(), &package_detection)?;
        let resolved_ref = git_output(checkout.repo_path(), &["rev-parse", "HEAD"])?;
        Ok(Self {
            spec,
            checkout,
            package_detection,
            source_digest,
            resolved_ref,
        })
    }

    pub fn preview(&self, target_root: &Path, fallback_name: &str) -> InstallPreview {
        build_install_preview_with_source(
            self.package_detection.clone(),
            target_root,
            fallback_name,
            "git",
            target_root.join(fallback_name).exists(),
            self.source_digest.clone(),
            self.resolved_ref.clone(),
        )
    }

    pub fn apply(
        self,
        target_root: &Path,
        fallback_name: &str,
        assistant_name: &str,
        expected_resolved_ref: Option<&str>,
        save_origin: impl Fn(&Path, &GitInstallSpec, &GitInstallOutcome) -> Result<(), String>,
    ) -> Result<GitInstallOutcome, String> {
        verify_resolved_ref(expected_resolved_ref, &self.resolved_ref)?;
        verify_source_digest(
            Some(&self.source_digest),
            &package_content_fingerprint(self.checkout.source_path(), &self.package_detection)?,
        )?;
        let preview = self.preview(target_root, fallback_name);
        if !preview.can_apply {
            return Err(preview.message);
        }
        let source = SkillPackageSource::open(self.checkout.source_path())?;
        let mut created_targets = Vec::new();
        let result = (|| {
            let mut first_outcome = None;
            for skill in &self.package_detection.detected_skills {
                let source_skill_path = source.resolve_detected_skill(skill)?;
                let target_path =
                    target_root.join(target_name_for_detected_skill(skill, fallback_name));
                copy_dir_recursive(&source_skill_path, &target_path)?;
                created_targets.push(target_path.clone());
                let structure = analyze_skill_structure(&target_path);
                if structure.structure_status != "complete" {
                    return Err(format!(
                        "复制后的 Skill 结构无效: {}",
                        target_path.to_string_lossy()
                    ));
                }
                let mut skill_spec = spec_for_detected_skill(&self.spec, skill);
                skill_spec.target_name = target_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| fallback_name.to_string());
                let outcome = GitInstallOutcome {
                    structure: structure.clone(),
                    installed_ref: self.resolved_ref.clone(),
                };
                save_origin(&target_path, &skill_spec, &outcome)?;
                mark_managed_skill(
                    target_root,
                    assistant_name,
                    &target_path,
                    &sanitize_git_locator(&skill_spec.original),
                )?;
                if first_outcome.is_none() {
                    first_outcome = Some(outcome);
                }
            }
            verify_source_digest(
                Some(&self.source_digest),
                &package_content_fingerprint(self.checkout.source_path(), &self.package_detection)?,
            )?;
            first_outcome.ok_or_else(|| "未识别到可安装的 Skill".to_string())
        })();
        if result.is_err() {
            for target in created_targets {
                if target.exists() {
                    let _ = remove_existing_path(&target);
                }
            }
        }
        result
    }
}

#[derive(Default)]
struct InstallBudget {
    files: usize,
    bytes: u64,
}

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    let target_existed = path_exists_or_symlink(dst);
    let mut budget = InstallBudget::default();
    let result = copy_dir_recursive_inner(src, dst, 0, &mut budget);
    if result.is_err() && !target_existed && path_exists_or_symlink(dst) {
        let _ = remove_existing_path(dst);
    }
    result
}

fn copy_dir_recursive_inner(
    src: &Path,
    dst: &Path,
    depth: usize,
    budget: &mut InstallBudget,
) -> Result<(), String> {
    if depth > MAX_INSTALL_DEPTH {
        return Err(format!(
            "Skill 目录层级超过 {} 层，已停止复制",
            MAX_INSTALL_DEPTH
        ));
    }
    if !src.is_dir() {
        return Err(format!("本地目录不存在: {}", src.to_string_lossy()));
    }
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if matches!(
            entry.file_name().to_string_lossy().as_ref(),
            ".git" | ".hg" | ".svn"
        ) {
            continue;
        }
        let entry_path = entry.path();
        let target_path = dst.join(entry.file_name());
        let metadata = fs::symlink_metadata(&entry_path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            copy_dir_recursive_inner(&entry_path, &target_path, depth + 1, budget)?;
        } else if metadata.is_file() {
            budget.files += 1;
            budget.bytes = budget.bytes.saturating_add(metadata.len());
            if budget.files > MAX_INSTALL_FILES || budget.bytes > MAX_INSTALL_BYTES {
                return Err(format!(
                    "Skill 超过安装限制（最多 {} 个文件、{} MB）",
                    MAX_INSTALL_FILES,
                    MAX_INSTALL_BYTES / 1024 / 1024
                ));
            }
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(&entry_path, &target_path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn package_content_fingerprint(
    source_root: &Path,
    package: &PackageDetection,
) -> Result<String, String> {
    let mut hash = StableHash::new();
    let mut budget = InstallBudget::default();
    let mut skills = package.detected_skills.iter().collect::<Vec<_>>();
    skills.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    if skills.is_empty() {
        return Ok(format!("sha256:{}", hash.finish()));
    }
    let source = SkillPackageSource::open(source_root)?;
    for skill in skills {
        hash.update(skill.relative_path.as_bytes());
        hash.update(&[0]);
        let skill_path = source.resolve_detected_skill(skill)?;
        fingerprint_installable_tree(&skill_path, &skill_path, 0, &mut budget, &mut hash)?;
    }
    Ok(format!("sha256:{}", hash.finish()))
}

pub fn installable_content_fingerprint(source_path: &Path) -> Result<String, String> {
    let package = detect_skill_package(source_path);
    if package.detected_skills.is_empty() {
        return Err("未识别到可计算指纹的 Skill".to_string());
    }
    package_content_fingerprint(source_path, &package)
}

fn verify_source_digest(expected: Option<&str>, actual: &str) -> Result<(), String> {
    let Some(expected) = expected.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if expected == actual {
        Ok(())
    } else {
        Err("安装来源在预览后发生变化，请重新检查结构".to_string())
    }
}

fn verify_resolved_ref(expected: Option<&str>, actual: &str) -> Result<(), String> {
    let Some(expected) = expected.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if expected == actual {
        Ok(())
    } else {
        Err("Git 来源在预览后发生变化，请重新检查结构".to_string())
    }
}

pub fn seal_install_preview(
    mut preview: InstallPreview,
    package: &str,
    assistant_name: &str,
    install_mode: &str,
    project_path: Option<&str>,
) -> InstallPreview {
    preview.plan_token.clear();
    let token = operation_plan_token(
        "install",
        &InstallPlanEnvelope {
            package: package.trim(),
            assistant_name: assistant_name.trim(),
            install_mode: install_mode.trim(),
            project_path: project_path.unwrap_or_default().trim(),
            preview: &preview,
        },
    );
    match token {
        Ok(token) => preview.plan_token = token,
        Err(error) => {
            preview.can_install = false;
            preview.can_apply = false;
            preview.message = format!("无法生成安装计划: {}", error);
            preview
                .structure_warnings
                .push("plan_token_failed".to_string());
        }
    }
    preview
}

fn fingerprint_installable_tree(
    path: &Path,
    root: &Path,
    depth: usize,
    budget: &mut InstallBudget,
    hash: &mut StableHash,
) -> Result<(), String> {
    if depth > MAX_INSTALL_DEPTH {
        return Err(format!(
            "Skill 目录层级超过 {} 层，无法生成操作计划",
            MAX_INSTALL_DEPTH
        ));
    }
    let mut entries = fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        if matches!(name.to_string_lossy().as_ref(), ".git" | ".hg" | ".svn") {
            continue;
        }
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let relative = entry_path.strip_prefix(root).unwrap_or(&entry_path);
        hash.update(relative.to_string_lossy().as_bytes());
        hash.update(&[0]);
        if metadata.is_dir() {
            hash.update(b"directory");
            fingerprint_installable_tree(&entry_path, root, depth + 1, budget, hash)?;
        } else if metadata.is_file() {
            budget.files += 1;
            budget.bytes = budget.bytes.saturating_add(metadata.len());
            if budget.files > MAX_INSTALL_FILES || budget.bytes > MAX_INSTALL_BYTES {
                return Err(format!(
                    "Skill 超过安装限制（最多 {} 个文件、{} MB）",
                    MAX_INSTALL_FILES,
                    MAX_INSTALL_BYTES / 1024 / 1024
                ));
            }
            hash.update(b"file");
            let mut file = fs::File::open(&entry_path).map_err(|error| error.to_string())?;
            let mut buffer = [0u8; 8192];
            loop {
                let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
        }
    }
    Ok(())
}

pub fn remove_existing_path(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).map_err(|e| e.to_string())
    } else if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(|e| e.to_string())
    } else {
        fs::remove_file(path).map_err(|e| e.to_string())
    }
}

pub fn preview_install_source(package: &str, source: &str, target_root: &Path) -> InstallPreview {
    let skill_name = match install_target_name(package, source) {
        Ok(name) => name,
        Err(err) => {
            return install_preview(InstallPreviewDraft {
                can_install: false,
                can_apply: false,
                message: err,
                target_name: String::new(),
                target_path: target_root.to_string_lossy().to_string(),
                source_kind: source.to_string(),
                package_detection: PackageDetection {
                    package_kind: "unknown".to_string(),
                    detected_skills: vec![],
                    warnings: vec!["unrecognized_input".to_string()],
                    needs_model: true,
                },
                target_actions: vec![],
                conflicts: vec![],
                structure: None,
                source_digest: String::new(),
                resolved_ref: String::new(),
            })
        }
    };
    let target_path = target_root.join(&skill_name);
    let target_exists = target_path.exists();
    let source_kind = if is_git_install_source(source) {
        "git"
    } else {
        source
    };

    let preview_result = match source {
        source_kind if is_git_install_source(source_kind) => PreparedGitInstall::prepare(package)
            .map(|prepared| prepared.preview(target_root, &skill_name)),
        "local" => {
            let source_path = expand_path(package.trim());
            let package_detection = detect_skill_package(&source_path);
            package_content_fingerprint(&source_path, &package_detection).map(|source_digest| {
                build_install_preview_with_source(
                    package_detection,
                    target_root,
                    &skill_name,
                    source_kind,
                    target_exists,
                    source_digest,
                    String::new(),
                )
            })
        }
        _ => Err("当前版本仅支持 Git 仓库和本地目录安装".to_string()),
    };

    match preview_result {
        Ok(preview) => preview,
        Err(err) => install_preview(InstallPreviewDraft {
            can_install: false,
            can_apply: false,
            message: err,
            target_name: skill_name,
            target_path: target_path.to_string_lossy().to_string(),
            source_kind: source_kind.to_string(),
            package_detection: PackageDetection {
                package_kind: "unknown".to_string(),
                detected_skills: vec![],
                warnings: vec!["structure_preview_failed".to_string()],
                needs_model: true,
            },
            target_actions: vec![],
            conflicts: vec![PreviewConflict {
                target: target_path.to_string_lossy().to_string(),
                reason: "structure_preview_failed".to_string(),
            }],
            structure: Some(SkillStructureInfo {
                structure_status: "nonstandard".to_string(),
                structure_features: vec![],
                structure_warnings: vec!["structure_preview_failed".to_string()],
                manifest_title: None,
                manifest_description: None,
            }),
            source_digest: String::new(),
            resolved_ref: String::new(),
        }),
    }
}

pub fn install_local_package_at_digest(
    source_path: &Path,
    target_root: &Path,
    fallback_name: &str,
    assistant_name: &str,
    expected_source_digest: Option<&str>,
) -> Result<SkillStructureInfo, String> {
    let package = detect_skill_package(source_path);
    let source_digest = package_content_fingerprint(source_path, &package)?;
    verify_source_digest(expected_source_digest, &source_digest)?;
    let source = SkillPackageSource::open(source_path)?;
    let preview =
        build_install_preview(package.clone(), target_root, fallback_name, "local", false);
    if !preview.can_apply {
        return Err(preview.message);
    }
    let mut created_targets = Vec::new();
    let result = (|| {
        let mut first_structure = None;
        for skill in &package.detected_skills {
            let source_skill_path = source.resolve_detected_skill(skill)?;
            let logical_source_path = source_for_detected_skill(source_path, skill);
            let target_path =
                target_root.join(target_name_for_detected_skill(skill, fallback_name));
            copy_dir_recursive(&source_skill_path, &target_path)?;
            created_targets.push(target_path.clone());
            mark_managed_skill(
                target_root,
                assistant_name,
                &target_path,
                &format!("local:{}", logical_source_path.to_string_lossy()),
            )?;
            let structure = analyze_skill_structure(&target_path);
            if structure.structure_status != "complete" {
                return Err(format!(
                    "复制后的 Skill 结构无效: {}",
                    target_path.to_string_lossy()
                ));
            }
            if first_structure.is_none() {
                first_structure = Some(structure);
            }
        }
        let structure = first_structure.ok_or_else(|| "未识别到可安装的 Skill".to_string())?;
        let current_digest = package_content_fingerprint(source_path, &package)?;
        verify_source_digest(expected_source_digest, &current_digest)?;
        Ok(structure)
    })();
    if result.is_err() {
        for target in created_targets {
            if target.exists() {
                let _ = remove_existing_path(&target);
            }
        }
    }
    result
}

pub fn preview_local_symlink_install(
    source_path: &Path,
    target_root: &Path,
    fallback_name: &str,
) -> InstallPreview {
    let package = detect_skill_package(source_path);
    match package_content_fingerprint(source_path, &package) {
        Ok(source_digest) => build_symlink_install_preview_with_source(
            package,
            source_path,
            target_root,
            fallback_name,
            source_digest,
        ),
        Err(error) => install_preview(InstallPreviewDraft {
            can_install: false,
            can_apply: false,
            message: error,
            target_name: fallback_name.to_string(),
            target_path: target_root
                .join(fallback_name)
                .to_string_lossy()
                .to_string(),
            source_kind: "local_symlink".to_string(),
            package_detection: package,
            target_actions: vec![],
            conflicts: vec![],
            structure: None,
            source_digest: String::new(),
            resolved_ref: String::new(),
        }),
    }
}

#[cfg(test)]
pub fn install_local_symlink_package(
    source_path: &Path,
    target_root: &Path,
    fallback_name: &str,
    assistant_name: &str,
) -> Result<SkillStructureInfo, String> {
    install_local_symlink_package_at_digest(
        source_path,
        target_root,
        fallback_name,
        assistant_name,
        None,
    )
}

pub fn install_local_symlink_package_at_digest(
    source_path: &Path,
    target_root: &Path,
    fallback_name: &str,
    assistant_name: &str,
    expected_source_digest: Option<&str>,
) -> Result<SkillStructureInfo, String> {
    let package = detect_skill_package(source_path);
    let source_digest = package_content_fingerprint(source_path, &package)?;
    verify_source_digest(expected_source_digest, &source_digest)?;
    let source = SkillPackageSource::open(source_path)?;
    let preview =
        build_symlink_install_preview(package.clone(), source_path, target_root, fallback_name);
    if !preview.can_apply {
        return Err(preview.message);
    }
    fs::create_dir_all(target_root).map_err(|e| e.to_string())?;
    let mut created_targets = Vec::new();
    let result = (|| {
        let mut first_structure = None;
        for skill in &package.detected_skills {
            let source_skill_path = source.resolve_detected_skill(skill)?;
            let logical_source_path = source_for_detected_skill(source_path, skill);
            let target_path =
                target_root.join(target_name_for_detected_skill(skill, fallback_name));
            create_dir_symlink(&source_skill_path, &target_path)?;
            created_targets.push(target_path.clone());
            mark_managed_skill(
                target_root,
                assistant_name,
                &target_path,
                &format!("symlink:{}", logical_source_path.to_string_lossy()),
            )?;
            let structure = analyze_skill_structure(&source_skill_path);
            if first_structure.is_none() {
                first_structure = Some(structure);
            }
        }
        let structure =
            first_structure.ok_or_else(|| "未识别到可软连接安装的 Skill".to_string())?;
        let current_digest = package_content_fingerprint(source_path, &package)?;
        verify_source_digest(expected_source_digest, &current_digest)?;
        Ok(structure)
    })();
    if result.is_err() {
        for target in created_targets {
            if target.exists() {
                let _ = remove_existing_path(&target);
            }
        }
    }
    result
}

#[cfg(test)]
pub fn install_git_package(
    spec: GitInstallSpec,
    target_root: &Path,
    fallback_name: &str,
    assistant_name: &str,
    save_origin: impl Fn(&Path, &GitInstallSpec, &GitInstallOutcome) -> Result<(), String>,
) -> Result<GitInstallOutcome, String> {
    install_git_package_at_ref(
        spec,
        target_root,
        fallback_name,
        assistant_name,
        None,
        save_origin,
    )
}

pub fn install_git_package_at_ref(
    spec: GitInstallSpec,
    target_root: &Path,
    fallback_name: &str,
    assistant_name: &str,
    expected_resolved_ref: Option<&str>,
    save_origin: impl Fn(&Path, &GitInstallSpec, &GitInstallOutcome) -> Result<(), String>,
) -> Result<GitInstallOutcome, String> {
    PreparedGitInstall::from_spec(spec)?.apply(
        target_root,
        fallback_name,
        assistant_name,
        expected_resolved_ref,
        save_origin,
    )
}

#[cfg(unix)]
fn create_dir_symlink(source: &Path, target: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err(format!(
            "软连接来源目录不存在: {}",
            source.to_string_lossy()
        ));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::os::unix::fs::symlink(source, target).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn create_dir_symlink(source: &Path, target: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err(format!(
            "软连接来源目录不存在: {}",
            source.to_string_lossy()
        ));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::os::windows::fs::symlink_dir(source, target).map_err(|e| e.to_string())
}

pub fn sync_git_snapshot_skill_checked(
    origin_locator: &str,
    resolved_locator: &str,
    tracking_ref: &str,
    target_path: &Path,
    validate: impl Fn(&SkillStructureInfo) -> Result<(), String>,
) -> Result<GitInstallOutcome, String> {
    let mut spec = parse_git_install_spec(origin_locator).or_else(|_| {
        let mut spec = parse_git_install_spec(resolved_locator)?;
        if spec.reference.is_none() && !tracking_ref.trim().is_empty() {
            spec.reference = Some(tracking_ref.trim().to_string());
        }
        Ok::<GitInstallSpec, String>(spec)
    })?;
    if spec.reference.is_none() && !tracking_ref.trim().is_empty() {
        spec.reference = Some(tracking_ref.trim().to_string());
    }
    ensure_git_available()?;
    with_temp_git_source(&spec, |source_path, repo_path| {
        if target_path.exists() || fs::symlink_metadata(target_path).is_ok() {
            return Err("更新目标尚未由受管事务暂存，已拒绝覆盖".to_string());
        }
        copy_dir_recursive(source_path, target_path)?;
        let structure = inspect_skill_for_inventory(target_path).structure;
        if structure.structure_status != "complete" {
            return Err("上游版本不再符合 Agent Skills 规范，已拒绝更新".to_string());
        }
        validate(&structure)?;
        let installed_ref = git_output(repo_path, &["rev-parse", "HEAD"]).unwrap_or_default();
        Ok(GitInstallOutcome {
            structure,
            installed_ref,
        })
    })
}

pub fn probe_git_snapshot(
    origin_locator: &str,
    resolved_locator: &str,
    tracking_ref: &str,
) -> Result<GitSnapshotProbe, String> {
    let spec = git_snapshot_spec(origin_locator, resolved_locator, tracking_ref)?;
    ensure_git_available()?;
    with_temp_git_source(&spec, |source_path, repo_path| {
        Ok(GitSnapshotProbe {
            latest_ref: git_output(repo_path, &["rev-parse", "HEAD"])?,
            source_digest: installable_content_fingerprint(source_path)?,
        })
    })
}

pub fn probe_git_snapshots(
    requests: &[GitSnapshotProbeRequest],
) -> Vec<(String, Result<GitSnapshotProbe, String>)> {
    let mut results = Vec::new();
    let mut groups = BTreeMap::<(String, String), Vec<(String, GitInstallSpec)>>::new();
    for request in requests {
        match git_snapshot_spec(
            &request.origin_locator,
            &request.resolved_locator,
            &request.tracking_ref,
        ) {
            Ok(spec) => {
                let group_key = (
                    spec.repo_url.clone(),
                    spec.reference.clone().unwrap_or_default(),
                );
                groups
                    .entry(group_key)
                    .or_default()
                    .push((request.key.clone(), spec));
            }
            Err(error) => results.push((request.key.clone(), Err(error))),
        }
    }

    for (_, group) in groups {
        let mut clone_spec = group[0].1.clone();
        clone_spec.subdir = None;
        let group_result = with_temp_git_source(&clone_spec, |_, repo_path| {
            let latest_ref = git_output(repo_path, &["rev-parse", "HEAD"])?;
            let probes = group
                .iter()
                .map(|(key, spec)| {
                    let source_path =
                        match resolve_git_source_path(repo_path, spec.subdir.as_deref()) {
                            Ok(path) => path,
                            Err(error) => return (key.clone(), Err(error)),
                        };
                    (
                        key.clone(),
                        installable_content_fingerprint(&source_path).map(|source_digest| {
                            GitSnapshotProbe {
                                latest_ref: latest_ref.clone(),
                                source_digest,
                            }
                        }),
                    )
                })
                .collect::<Vec<_>>();
            Ok(probes)
        });
        match group_result {
            Ok(group_results) => results.extend(group_results),
            Err(error) => {
                results.extend(group.into_iter().map(|(key, _)| (key, Err(error.clone()))));
            }
        }
    }
    results
}

pub fn has_git_snapshot_spec(origin_locator: &str, resolved_locator: &str) -> bool {
    git_snapshot_spec(origin_locator, resolved_locator, "").is_ok()
}

fn git_snapshot_spec(
    origin_locator: &str,
    resolved_locator: &str,
    tracking_ref: &str,
) -> Result<GitInstallSpec, String> {
    let mut spec = parse_git_install_spec(origin_locator).or_else(|_| {
        let mut spec = parse_git_install_spec(resolved_locator)?;
        if spec.reference.is_none() && !tracking_ref.trim().is_empty() {
            spec.reference = Some(tracking_ref.trim().to_string());
        }
        Ok::<GitInstallSpec, String>(spec)
    })?;
    if spec.reference.is_none() && !tracking_ref.trim().is_empty() {
        spec.reference = Some(tracking_ref.trim().to_string());
    }
    Ok(spec)
}

struct InstallPreviewDraft {
    can_install: bool,
    can_apply: bool,
    message: String,
    target_name: String,
    target_path: String,
    source_kind: String,
    package_detection: PackageDetection,
    target_actions: Vec<PreviewAction>,
    conflicts: Vec<PreviewConflict>,
    structure: Option<SkillStructureInfo>,
    source_digest: String,
    resolved_ref: String,
}

fn install_preview(draft: InstallPreviewDraft) -> InstallPreview {
    let structure = draft.structure.unwrap_or_default();
    InstallPreview {
        can_install: draft.can_install,
        can_apply: draft.can_apply,
        message: draft.message,
        target_name: draft.target_name,
        target_path: draft.target_path,
        source_kind: draft.source_kind,
        package_detection: draft.package_detection,
        target_actions: draft.target_actions,
        conflicts: draft.conflicts,
        structure_status: structure.structure_status,
        structure_features: structure.structure_features,
        structure_warnings: structure.structure_warnings,
        manifest_title: structure.manifest_title,
        manifest_description: structure.manifest_description,
        install_policy: InstallPolicyDecision::default(),
        plan_token: String::new(),
        source_digest: draft.source_digest,
        resolved_ref: draft.resolved_ref,
    }
}

fn build_install_preview(
    package_detection: PackageDetection,
    target_root: &Path,
    fallback_name: &str,
    source_kind: &str,
    legacy_target_exists: bool,
) -> InstallPreview {
    build_install_preview_with_source(
        package_detection,
        target_root,
        fallback_name,
        source_kind,
        legacy_target_exists,
        String::new(),
        String::new(),
    )
}

fn build_install_preview_with_source(
    mut package_detection: PackageDetection,
    target_root: &Path,
    fallback_name: &str,
    source_kind: &str,
    legacy_target_exists: bool,
    source_digest: String,
    resolved_ref: String,
) -> InstallPreview {
    let mut actions = Vec::new();
    let mut conflicts = Vec::new();
    let mut planned_targets = HashSet::new();
    if package_detection.detected_skills.is_empty() {
        conflicts.push(PreviewConflict {
            target: target_root.to_string_lossy().to_string(),
            reason: "unrecognized_input".to_string(),
        });
    }
    for skill in &package_detection.detected_skills {
        let target = target_root.join(target_name_for_detected_skill(skill, fallback_name));
        let target_string = target.to_string_lossy().to_string();
        if !planned_targets.insert(target.clone()) {
            conflicts.push(PreviewConflict {
                target: target_string.clone(),
                reason: "duplicate_target".to_string(),
            });
            actions.push(PreviewAction {
                action: "skip".to_string(),
                source: skill.relative_path.clone(),
                target: target_string,
                reason: "安装计划包含重复目标目录".to_string(),
            });
        } else if skill.structure_status != "complete" {
            conflicts.push(PreviewConflict {
                target: target_string.clone(),
                reason: "invalid_skill_structure".to_string(),
            });
            actions.push(PreviewAction {
                action: "skip".to_string(),
                source: skill.relative_path.clone(),
                target: target_string,
                reason: "Skill 不符合 Agent Skills 规范".to_string(),
            });
        } else if target.exists() {
            conflicts.push(PreviewConflict {
                target: target_string.clone(),
                reason: "target_exists".to_string(),
            });
            actions.push(PreviewAction {
                action: "skip".to_string(),
                source: skill.relative_path.clone(),
                target: target_string,
                reason: "目标目录已存在".to_string(),
            });
        } else {
            actions.push(PreviewAction {
                action: "copy".to_string(),
                source: skill.relative_path.clone(),
                target: target_string,
                reason: "安装 Skill 目录".to_string(),
            });
        }
    }
    if legacy_target_exists && conflicts.is_empty() {
        conflicts.push(PreviewConflict {
            target: target_root
                .join(fallback_name)
                .to_string_lossy()
                .to_string(),
            reason: "target_exists".to_string(),
        });
    }
    let can_apply = conflicts.is_empty() && !actions.is_empty();
    if !can_apply
        && !package_detection
            .warnings
            .iter()
            .any(|w| w == "target_exists")
    {
        package_detection.warnings.push(if actions.is_empty() {
            "unrecognized_input".to_string()
        } else {
            "target_exists".to_string()
        });
    }
    let primary = package_detection
        .detected_skills
        .first()
        .map(skill_structure_from_detection)
        .unwrap_or_else(|| SkillStructureInfo {
            structure_status: "nonstandard".to_string(),
            structure_features: vec![],
            structure_warnings: package_detection.warnings.clone(),
            manifest_title: None,
            manifest_description: None,
        });
    let target_name = package_detection
        .detected_skills
        .first()
        .map(|skill| target_name_for_detected_skill(skill, fallback_name))
        .unwrap_or_else(|| fallback_name.to_string());
    let target_path = target_root.join(&target_name).to_string_lossy().to_string();
    install_preview(InstallPreviewDraft {
        can_install: can_apply,
        can_apply,
        message: if can_apply {
            format!("将安装 {} 个 Skill", actions.len())
        } else if conflicts.is_empty() {
            "未识别到可安装的 Skill".to_string()
        } else {
            format!("发现 {} 个安装冲突", conflicts.len())
        },
        target_name,
        target_path,
        source_kind: source_kind.to_string(),
        package_detection,
        target_actions: actions,
        conflicts,
        structure: Some(primary),
        source_digest,
        resolved_ref,
    })
}

fn build_symlink_install_preview(
    package_detection: PackageDetection,
    source_root: &Path,
    target_root: &Path,
    fallback_name: &str,
) -> InstallPreview {
    build_symlink_install_preview_with_source(
        package_detection,
        source_root,
        target_root,
        fallback_name,
        String::new(),
    )
}

fn build_symlink_install_preview_with_source(
    mut package_detection: PackageDetection,
    source_root: &Path,
    target_root: &Path,
    fallback_name: &str,
    source_digest: String,
) -> InstallPreview {
    let mut actions = Vec::new();
    let mut conflicts = Vec::new();
    let mut planned_targets = HashSet::new();
    let source_boundary = SkillPackageSource::open(source_root);
    let mut unsafe_source = false;
    if package_detection.detected_skills.is_empty() {
        conflicts.push(PreviewConflict {
            target: target_root.to_string_lossy().to_string(),
            reason: "unrecognized_input".to_string(),
        });
    }
    for skill in &package_detection.detected_skills {
        let resolved_source = match &source_boundary {
            Ok(source) => source.resolve_detected_skill(skill),
            Err(error) => Err(error.clone()),
        };
        let source = match resolved_source {
            Ok(source) => source,
            Err(error) => {
                unsafe_source = true;
                let source = source_for_detected_skill(source_root, skill);
                let source_string = source.to_string_lossy().to_string();
                let target = target_root.join(target_name_for_detected_skill(skill, fallback_name));
                let target_string = target.to_string_lossy().to_string();
                conflicts.push(PreviewConflict {
                    target: source_string.clone(),
                    reason: "unsafe_paths".to_string(),
                });
                actions.push(PreviewAction {
                    action: "skip".to_string(),
                    source: source_string,
                    target: target_string,
                    reason: error,
                });
                continue;
            }
        };
        let target = target_root.join(target_name_for_detected_skill(skill, fallback_name));
        let source_string = source.to_string_lossy().to_string();
        let target_string = target.to_string_lossy().to_string();
        if !planned_targets.insert(target.clone()) {
            conflicts.push(PreviewConflict {
                target: target_string.clone(),
                reason: "duplicate_target".to_string(),
            });
            actions.push(PreviewAction {
                action: "skip".to_string(),
                source: source_string,
                target: target_string,
                reason: "软连接计划包含重复目标目录".to_string(),
            });
        } else if skill.structure_status != "complete" {
            conflicts.push(PreviewConflict {
                target: target_string.clone(),
                reason: "invalid_skill_structure".to_string(),
            });
            actions.push(PreviewAction {
                action: "skip".to_string(),
                source: source_string,
                target: target_string,
                reason: "Skill 不符合 Agent Skills 规范".to_string(),
            });
        } else if !source.is_dir() {
            conflicts.push(PreviewConflict {
                target: source_string.clone(),
                reason: "path_missing".to_string(),
            });
            actions.push(PreviewAction {
                action: "skip".to_string(),
                source: source_string,
                target: target_string,
                reason: "软连接来源目录不存在".to_string(),
            });
        } else if path_exists_or_symlink(&target) {
            conflicts.push(PreviewConflict {
                target: target_string.clone(),
                reason: "target_exists".to_string(),
            });
            actions.push(PreviewAction {
                action: "skip".to_string(),
                source: source_string,
                target: target_string,
                reason: "目标目录已存在".to_string(),
            });
        } else {
            actions.push(PreviewAction {
                action: "symlink".to_string(),
                source: source_string,
                target: target_string,
                reason: "创建项目级软连接".to_string(),
            });
        }
    }
    if unsafe_source
        && !package_detection
            .warnings
            .iter()
            .any(|warning| warning == "unsafe_paths")
    {
        package_detection.warnings.push("unsafe_paths".to_string());
    }
    let can_apply = conflicts.is_empty() && !actions.is_empty();
    if !can_apply
        && !package_detection
            .warnings
            .iter()
            .any(|w| w == "target_exists")
    {
        package_detection.warnings.push(if actions.is_empty() {
            "unrecognized_input".to_string()
        } else {
            "target_exists".to_string()
        });
    }
    let primary = package_detection
        .detected_skills
        .first()
        .map(skill_structure_from_detection)
        .unwrap_or_else(|| SkillStructureInfo {
            structure_status: "nonstandard".to_string(),
            structure_features: vec![],
            structure_warnings: package_detection.warnings.clone(),
            manifest_title: None,
            manifest_description: None,
        });
    let target_name = package_detection
        .detected_skills
        .first()
        .map(|skill| target_name_for_detected_skill(skill, fallback_name))
        .unwrap_or_else(|| fallback_name.to_string());
    let target_path = target_root.join(&target_name).to_string_lossy().to_string();
    install_preview(InstallPreviewDraft {
        can_install: can_apply,
        can_apply,
        message: if can_apply {
            format!("将软连接安装 {} 个 Skill", actions.len())
        } else if conflicts.is_empty() {
            "未识别到可软连接安装的 Skill".to_string()
        } else {
            format!("发现 {} 个软连接安装冲突", conflicts.len())
        },
        target_name,
        target_path,
        source_kind: "local_symlink".to_string(),
        package_detection,
        target_actions: actions,
        conflicts,
        structure: Some(primary),
        source_digest,
        resolved_ref: String::new(),
    })
}

fn skill_structure_from_detection(skill: &DetectedSkill) -> SkillStructureInfo {
    SkillStructureInfo {
        structure_status: skill.structure_status.clone(),
        structure_features: skill.features.clone(),
        structure_warnings: skill.warnings.clone(),
        manifest_title: skill.title.clone(),
        manifest_description: skill.description.clone(),
    }
}

fn path_exists_or_symlink(path: &Path) -> bool {
    path.exists() || fs::symlink_metadata(path).is_ok()
}

fn target_name_for_detected_skill(skill: &DetectedSkill, fallback_name: &str) -> String {
    if skill.relative_path == "." {
        return skill
            .title
            .as_deref()
            .filter(|name| !name.is_empty() && *name != "." && *name != "..")
            .filter(|name| !name.contains(['/', '\\']))
            .unwrap_or(fallback_name)
            .to_string();
    }
    Path::new(&skill.relative_path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| fallback_name.to_string())
}

fn source_for_detected_skill(source_root: &Path, skill: &DetectedSkill) -> std::path::PathBuf {
    if skill.relative_path == "." {
        source_root.to_path_buf()
    } else {
        source_root.join(&skill.relative_path)
    }
}

fn spec_for_detected_skill(spec: &GitInstallSpec, skill: &DetectedSkill) -> GitInstallSpec {
    let mut next = spec.clone();
    if skill.relative_path != "." {
        let combined = match spec.subdir.as_deref() {
            Some(base) if !base.trim().is_empty() => format!("{}/{}", base, skill.relative_path),
            _ => skill.relative_path.clone(),
        };
        next.subdir = Some(combined.clone());
        let reference = spec.reference.clone().unwrap_or_else(|| "HEAD".to_string());
        next.original = format!("{}#{}:{}", spec.repo_url, reference, combined);
    }
    next
}

fn ensure_git_available() -> Result<(), String> {
    let check = if cfg!(target_os = "windows") {
        Command::new("where").arg("git").output()
    } else {
        Command::new("which").arg("git").output()
    };
    if check.map(|o| o.status.success()).unwrap_or(false) {
        Ok(())
    } else {
        Err("未安装: git".to_string())
    }
}

fn clone_git_install_spec(spec: &GitInstallSpec, clone_path: &Path) -> Result<(), String> {
    validate_git_repo_locator(&spec.repo_url)?;
    if let Some(reference) = spec.reference.as_deref() {
        validate_git_reference(reference)?;
    }
    let clone_target = clone_path.to_string_lossy().to_string();
    if let Some(reference) = spec.reference.as_deref() {
        let shallow = run_git(
            &[
                "clone",
                "--depth",
                "1",
                "--branch",
                reference,
                "--single-branch",
                "--no-recurse-submodules",
                "--",
                spec.repo_url.as_str(),
                clone_target.as_str(),
            ],
            None,
            90,
        )?;
        if shallow.status.success() {
            return Ok(());
        }
        if clone_path.exists() {
            remove_existing_path(clone_path)?;
        }
        fetch_git_reference_shallow(spec, clone_path, reference).map_err(|fetch_error| {
            format!(
                "无法浅克隆 Git 引用：{}；{}",
                format_git_error(&shallow),
                fetch_error
            )
        })
    } else {
        let out = run_git(
            &[
                "clone",
                "--depth",
                "1",
                "--no-recurse-submodules",
                "--",
                spec.repo_url.as_str(),
                clone_target.as_str(),
            ],
            None,
            90,
        )?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format_git_error(&out))
        }
    }
}

fn fetch_git_reference_shallow(
    spec: &GitInstallSpec,
    clone_path: &Path,
    reference: &str,
) -> Result<(), String> {
    let clone_target = clone_path.to_string_lossy().to_string();
    let init = run_git(&["init", "--quiet", "--", clone_target.as_str()], None, 10)?;
    if !init.status.success() {
        return Err(format_git_error(&init));
    }
    let fetch = run_git(
        &[
            "fetch",
            "--depth",
            "1",
            "--no-tags",
            "--no-recurse-submodules",
            "--",
            spec.repo_url.as_str(),
            reference,
        ],
        Some(clone_path),
        90,
    )?;
    if !fetch.status.success() {
        return Err(format_git_error(&fetch));
    }
    let checkout = run_git(
        &["checkout", "--detach", "FETCH_HEAD"],
        Some(clone_path),
        30,
    )?;
    if checkout.status.success() {
        Ok(())
    } else {
        Err(format_git_error(&checkout))
    }
}

fn resolve_git_source_path(repo_path: &Path, subdir: Option<&str>) -> Result<PathBuf, String> {
    let repo_metadata = fs::symlink_metadata(repo_path).map_err(|error| {
        format!(
            "无法检查 Git checkout 目录 {}: {error}",
            repo_path.to_string_lossy()
        )
    })?;
    if repo_metadata.file_type().is_symlink() || !repo_metadata.is_dir() {
        return Err("Git checkout 根目录不是安全的目录".to_string());
    }
    let canonical_repo = repo_path.canonicalize().map_err(|error| {
        format!(
            "无法解析 Git checkout 目录 {}: {error}",
            repo_path.to_string_lossy()
        )
    })?;
    let Some(subdir) = subdir.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(canonical_repo);
    };

    let mut candidate = repo_path.to_path_buf();
    for component in Path::new(subdir).components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(name) => candidate.push(name),
            _ => return Err("Git 子目录不能包含绝对路径或上级目录".to_string()),
        }
        let metadata = fs::symlink_metadata(&candidate)
            .map_err(|_| format!("仓库子目录不存在: {}", subdir))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("Git 子目录不能经过软连接: {}", subdir));
        }
        if !metadata.is_dir() {
            return Err(format!("仓库子目录不存在: {}", subdir));
        }
    }

    let resolved = candidate
        .canonicalize()
        .map_err(|_| format!("仓库子目录不存在: {}", subdir))?;
    if !resolved.starts_with(&canonical_repo) {
        return Err(format!("Git 子目录越过了 checkout 根目录: {}", subdir));
    }
    Ok(resolved)
}

fn with_temp_git_source<T>(
    spec: &GitInstallSpec,
    action: impl FnOnce(&Path, &Path) -> Result<T, String>,
) -> Result<T, String> {
    let checkout = TempGitSource::prepare(spec)?;
    action(checkout.source_path(), checkout.repo_path())
}

fn run_git(args: &[&str], current_dir: Option<&Path>, timeout_secs: u64) -> Result<Output, String> {
    let mut safe_args = vec![
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "protocol.file.allow=user",
        "-c",
        "submodule.recurse=false",
    ];
    safe_args.extend_from_slice(args);
    run_command_with_timeout(
        "git",
        &safe_args,
        current_dir,
        Duration::from_secs(timeout_secs),
        &[
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GCM_INTERACTIVE", "Never"),
            ("GIT_LFS_SKIP_SMUDGE", "1"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
        ],
    )
}

fn git_output(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = run_git(args, Some(repo), 10)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format_git_error(&out))
    }
}

fn format_git_error(out: &Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if stderr.is_empty() {
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    } else {
        stderr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("skillmate-install-test-{}-{}", name, generate_id()))
    }

    fn complete_detected_skill(relative_path: &str) -> DetectedSkill {
        DetectedSkill {
            relative_path: relative_path.to_string(),
            structure_status: "complete".to_string(),
            title: Some("writer".to_string()),
            description: Some("写作".to_string()),
            features: vec!["skill_md".to_string()],
            warnings: vec![],
        }
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

    #[cfg(unix)]
    fn create_dir_symlink_for_test(source: &Path, target: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(source, target)
    }

    #[cfg(windows)]
    fn create_dir_symlink_for_test(source: &Path, target: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(source, target)
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

    fn dir_symlink_supported_or_skip(source: &Path, target: &Path) -> bool {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        match create_dir_symlink_for_test(source, target) {
            Ok(()) => {
                remove_test_dir_symlink(target);
                true
            }
            Err(error) if is_windows_symlink_permission_error(&error) => {
                eprintln!("跳过 symlink 单测：当前 Windows 环境不允许创建目录符号链接：{error}");
                false
            }
            Err(error) => panic!("创建测试目录符号链接失败: {error}"),
        }
    }

    #[cfg(unix)]
    fn remove_test_dir_symlink(path: &Path) {
        let _ = fs::remove_file(path);
    }

    #[cfg(windows)]
    fn remove_test_dir_symlink(path: &Path) {
        let _ = fs::remove_dir(path);
    }

    #[test]
    fn parse_git_install_spec_accepts_plain_repo() {
        let spec = parse_git_install_spec("https://github.com/example/cool-skill.git").unwrap();

        assert_eq!(spec.repo_url, "https://github.com/example/cool-skill.git");
        assert_eq!(spec.reference, None);
        assert_eq!(spec.subdir, None);
        assert_eq!(spec.target_name, "cool-skill");
        assert!(has_git_snapshot_spec(&spec.original, &spec.repo_url));
    }

    #[test]
    fn parse_git_install_spec_accepts_ref_and_subdir() {
        let spec = parse_git_install_spec("git@github.com:example/skills.git#v1.2.0:skills/writer")
            .unwrap();

        assert_eq!(spec.repo_url, "git@github.com:example/skills.git");
        assert_eq!(spec.reference, Some("v1.2.0".to_string()));
        assert_eq!(spec.subdir, Some("skills/writer".to_string()));
        assert_eq!(spec.target_name, "writer");
    }

    #[test]
    fn parse_git_install_spec_accepts_github_tree_url() {
        let spec =
            parse_git_install_spec("https://github.com/example/skills/tree/main/skills/writer")
                .unwrap();

        assert_eq!(spec.repo_url, "https://github.com/example/skills.git");
        assert_eq!(spec.reference, Some("main".to_string()));
        assert_eq!(spec.subdir, Some("skills/writer".to_string()));
        assert_eq!(spec.target_name, "writer");
    }

    #[test]
    fn parse_git_install_spec_rejects_ambiguous_github_tree_branch() {
        let err = parse_git_install_spec(
            "https://github.com/example/skills/tree/feature/foo/skills/writer",
        )
        .unwrap_err();

        assert_eq!(
            err,
            "GitHub tree URL 的分支名可能包含 /，请改用 repo#ref:path 格式"
        );
    }

    #[test]
    fn parse_git_install_spec_accepts_github_shorthand() {
        let spec = parse_git_install_spec("example/skills#main:skills/writer").unwrap();

        assert_eq!(spec.repo_url, "https://github.com/example/skills.git");
        assert_eq!(spec.reference, Some("main".to_string()));
        assert_eq!(spec.subdir, Some("skills/writer".to_string()));
        assert_eq!(spec.target_name, "writer");
    }

    #[test]
    fn parse_git_install_spec_rejects_parent_escape_subdir() {
        let err = parse_git_install_spec("https://github.com/example/skills.git#main:../writer")
            .unwrap_err();

        assert_eq!(err, "Git 子目录不能包含绝对路径或上级目录");
    }

    #[test]
    fn detect_install_source_identifies_git_subdir() {
        let detection = detect_install_source_rules("example/skills#main:skills/writer");

        assert_eq!(detection.detector, "rules");
        assert_eq!(detection.source_kind, "git_subdir");
        assert_eq!(detection.normalized_source, "git");
        assert_eq!(
            detection.repo_url,
            Some("https://github.com/example/skills.git".into())
        );
        assert_eq!(detection.reference, Some("main".into()));
        assert_eq!(detection.subdir, Some("skills/writer".into()));
        assert_eq!(detection.target_name, Some("writer".into()));
        assert_eq!(detection.confidence, "high");
        assert!(!detection.needs_model);
    }

    #[test]
    fn detect_install_source_marks_archive_as_unsupported() {
        let detection =
            detect_install_source_rules("https://github.com/example/skills/archive/main.zip");

        assert_eq!(detection.source_kind, "archive");
        assert_eq!(detection.normalized_source, "");
        assert_eq!(detection.confidence, "medium");
        assert_eq!(detection.warnings, vec!["archive_unsupported".to_string()]);
        assert!(!detection.needs_model);
    }

    #[test]
    fn detect_install_source_defers_unknown_input_to_model() {
        let detection = detect_install_source_rules("帮我安装这个 README 里的 writer skill");

        assert_eq!(detection.source_kind, "unknown");
        assert_eq!(detection.confidence, "low");
        assert_eq!(detection.warnings, vec!["unrecognized_input".to_string()]);
        assert!(detection.needs_model);
    }

    #[test]
    fn copy_dir_recursive_skips_symlinks() {
        let root = test_dir("skip-symlink");
        let src = root.join("src");
        let dst = root.join("dst");
        let outside = root.join("outside.txt");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "skill").unwrap();
        fs::write(&outside, "outside").unwrap();

        if !create_file_symlink_or_skip(&outside, &src.join("outside-link")) {
            let _ = fs::remove_dir_all(root);
            return;
        }

        let detection = detect_skill_package(&src);
        assert!(detection.detected_skills[0]
            .warnings
            .contains(&"unsafe_paths".to_string()));

        copy_dir_recursive(&src, &dst).unwrap();

        assert_eq!(fs::read_to_string(dst.join("SKILL.md")).unwrap(), "skill");
        assert!(!dst.join("outside-link").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_preview_rejects_symlinked_skill_root_outside_source() {
        let root = test_dir("outside-skill-link");
        let source_root = root.join("trusted/bundle");
        let outside_skill = root.join("untrusted/writer");
        let target_root = root.join("installed");
        fs::create_dir_all(&source_root).unwrap();
        fs::create_dir_all(&outside_skill).unwrap();
        fs::write(
            outside_skill.join("SKILL.md"),
            "---\nname: writer\ndescription: 外部 Skill\n---\n",
        )
        .unwrap();
        if !dir_symlink_supported_or_skip(&outside_skill, &source_root.join("__probe")) {
            let _ = fs::remove_dir_all(root);
            return;
        }
        create_dir_symlink_for_test(&outside_skill, &source_root.join("writer")).unwrap();

        let preview = preview_install_source(
            source_root.to_string_lossy().as_ref(),
            "local",
            &target_root,
        );

        assert!(!preview.can_apply);
        assert!(preview.package_detection.detected_skills.is_empty());
        assert!(preview
            .package_detection
            .warnings
            .contains(&"unsafe_paths".to_string()));
        let install =
            install_local_package_at_digest(&source_root, &target_root, "writer", "Codex", None);
        assert!(install.is_err());
        assert!(!target_root.join("writer").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_preview_rejects_symlinked_bundle_root_outside_source() {
        let root = test_dir("outside-bundle-link");
        let outside_bundle = root.join("untrusted/bundle");
        let outside_skill = outside_bundle.join("writer");
        let source_root = root.join("trusted/bundle");
        let target_root = root.join("installed");
        fs::create_dir_all(source_root.parent().unwrap()).unwrap();
        fs::create_dir_all(&outside_skill).unwrap();
        fs::write(
            outside_skill.join("SKILL.md"),
            "---\nname: writer\ndescription: 外部 Skill\n---\n",
        )
        .unwrap();
        if !dir_symlink_supported_or_skip(&outside_bundle, &source_root) {
            let _ = fs::remove_dir_all(root);
            return;
        }
        create_dir_symlink_for_test(&outside_bundle, &source_root).unwrap();

        let preview = preview_install_source(
            source_root.to_string_lossy().as_ref(),
            "local",
            &target_root,
        );

        assert!(!preview.can_apply);
        assert!(preview.package_detection.detected_skills.is_empty());
        assert!(preview
            .package_detection
            .warnings
            .contains(&"unsafe_paths".to_string()));
        let install =
            install_local_package_at_digest(&source_root, &target_root, "writer", "Codex", None);
        assert!(install.is_err());
        assert!(!target_root.join("writer").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_preview_rejects_skill_beneath_symlinked_parent_outside_source() {
        let root = test_dir("outside-parent-link");
        let source_root = root.join("trusted/bundle");
        let outside_category = root.join("untrusted/writing");
        let outside_skill = outside_category.join("writer");
        let target_root = root.join("installed");
        fs::create_dir_all(source_root.join("skills")).unwrap();
        fs::create_dir_all(&outside_skill).unwrap();
        fs::write(
            outside_skill.join("SKILL.md"),
            "---\nname: writer\ndescription: 外部 Skill\n---\n",
        )
        .unwrap();
        if !dir_symlink_supported_or_skip(&outside_category, &source_root.join("skills/__probe")) {
            let _ = fs::remove_dir_all(root);
            return;
        }
        create_dir_symlink_for_test(&outside_category, &source_root.join("skills/writing"))
            .unwrap();

        let preview = preview_install_source(
            source_root.to_string_lossy().as_ref(),
            "local",
            &target_root,
        );

        assert!(!preview.can_apply);
        assert!(preview.package_detection.detected_skills.is_empty());
        assert!(preview
            .package_detection
            .warnings
            .contains(&"unsafe_paths".to_string()));
        assert!(!target_root.join("writer").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn install_preview_rejects_duplicate_target_names() {
        let target = test_dir("duplicate-target");
        let preview = build_install_preview(
            PackageDetection {
                package_kind: "multi_skill".to_string(),
                detected_skills: vec![
                    complete_detected_skill("first/writer"),
                    complete_detected_skill("second/writer"),
                ],
                warnings: vec![],
                needs_model: false,
            },
            &target,
            "fallback",
            "local",
            false,
        );

        assert!(!preview.can_apply);
        assert!(preview
            .conflicts
            .iter()
            .any(|conflict| conflict.reason == "duplicate_target"));
        let _ = fs::remove_dir_all(target);
    }

    #[cfg(unix)]
    #[test]
    fn failed_recursive_copy_removes_new_partial_target() {
        use std::os::unix::fs::PermissionsExt;

        let root = test_dir("partial-copy");
        let source = root.join("source");
        let target = root.join("target");
        let locked = source.join("locked");
        fs::create_dir_all(&locked).unwrap();
        fs::write(source.join("SKILL.md"), "skill").unwrap();
        fs::write(locked.join("secret"), "secret").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        let result = copy_dir_recursive(&source, &target);

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert!(!target.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_symlink_install_creates_project_link() {
        let root = test_dir("project-symlink");
        let source = root.join("writer");
        let target_root = root.join("project/.codex/skills");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: writer\ndescription: 写作 Skill\n---\n\n# Writer\n\n说明",
        )
        .unwrap();
        if !dir_symlink_supported_or_skip(&source, &target_root.join("__probe")) {
            let _ = fs::remove_dir_all(root);
            return;
        }

        let preview = preview_local_symlink_install(&source, &target_root, "writer");
        assert!(preview.can_apply);
        assert_eq!(preview.target_actions[0].action, "symlink");

        let structure =
            install_local_symlink_package(&source, &target_root, "writer", "Codex").unwrap();
        let target = target_root.join("writer");

        assert_eq!(structure.structure_status, "complete");
        assert!(fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "---\nname: writer\ndescription: 写作 Skill\n---\n\n# Writer\n\n说明"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepared_git_install_reuses_checkout_and_cleans_it() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let root = test_dir("prepared-git-reuse");
        let repo = root.join("writer");
        let target_root = root.join("installed");
        fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let output = run_git(args, Some(&repo), 10).unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "SkillMate Test"]);
        git(&["config", "user.email", "skillmate-test@example.com"]);
        fs::write(
            repo.join("SKILL.md"),
            "---\nname: writer\ndescription: 准备后复用 checkout\n---\n",
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "initial"]);

        let prepared = PreparedGitInstall::prepare(repo.to_string_lossy().as_ref()).unwrap();
        let checkout_root = prepared.checkout.temp_root.clone();
        let preview = prepared.preview(&target_root, "writer");
        assert!(preview.can_apply, "{preview:?}");
        fs::remove_dir_all(&repo).unwrap();

        let outcome = prepared
            .apply(
                &target_root,
                "writer",
                "Codex",
                Some(&preview.resolved_ref),
                |_, _, _| Ok(()),
            )
            .unwrap();

        assert_eq!(outcome.installed_ref, preview.resolved_ref);
        assert!(target_root.join("writer/SKILL.md").exists());
        assert!(!checkout_root.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepared_git_install_rejects_symlinked_subdir_component() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let root = test_dir("git-subdir-symlink");
        let repo = root.join("source");
        let outside_skill = root.join("outside/writer");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&outside_skill).unwrap();
        fs::write(
            outside_skill.join("SKILL.md"),
            "---\nname: writer\ndescription: checkout 外部 Skill\n---\n",
        )
        .unwrap();
        if !dir_symlink_supported_or_skip(&root.join("outside"), &repo.join("__probe")) {
            let _ = fs::remove_dir_all(root);
            return;
        }
        create_dir_symlink_for_test(&root.join("outside"), &repo.join("link")).unwrap();
        let git = |args: &[&str]| {
            let output = run_git(args, Some(&repo), 10).unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "SkillMate Test"]);
        git(&["config", "user.email", "skillmate-test@example.com"]);
        git(&["add", "link"]);
        git(&["commit", "-m", "symlink"]);
        let package = format!("{}#main:link/writer", repo.to_string_lossy());

        let error = match PreparedGitInstall::prepare(&package) {
            Ok(_) => panic!("不应接受经过软连接的 Git 子目录"),
            Err(error) => error,
        };

        assert!(
            error.contains("软连接") || error.contains("仓库子目录不存在"),
            "{error}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn root_git_snapshot_can_update_without_embedded_git_directory() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let root = test_dir("root-git-snapshot");
        let repo = root.join("writer");
        let target_root = root.join("installed");
        fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let output = run_git(args, Some(&repo), 10).unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "SkillMate Test"]);
        git(&["config", "user.email", "skillmate-test@example.com"]);
        fs::write(
            repo.join("SKILL.md"),
            "---\nname: writer\ndescription: 初始版本\n---\n",
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "initial"]);

        let spec = parse_git_install_spec(&repo.to_string_lossy()).unwrap();
        install_git_package(spec.clone(), &target_root, "writer", "Codex", |_, _, _| {
            Ok(())
        })
        .unwrap();
        let installed = target_root.join("writer");
        assert!(!installed.join(".git").exists());

        fs::write(
            repo.join("SKILL.md"),
            "---\nname: writer\ndescription: 更新版本\n---\n",
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "update"]);
        let previous = root.join("previous-writer");
        fs::rename(&installed, &previous).unwrap();
        sync_git_snapshot_skill_checked(&spec.original, &spec.repo_url, "main", &installed, |_| {
            Ok(())
        })
        .unwrap();
        fs::remove_dir_all(&previous).unwrap();

        assert!(fs::read_to_string(installed.join("SKILL.md"))
            .unwrap()
            .contains("更新版本"));
        assert!(!installed.join(".git").exists());

        fs::write(
            repo.join("SKILL.md"),
            "---\nname: writer\ndescription: 应被策略阻止\n---\n",
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "blocked"]);
        fs::rename(&installed, &previous).unwrap();
        let error = sync_git_snapshot_skill_checked(
            &spec.original,
            &spec.repo_url,
            "main",
            &installed,
            |_| Err("安装策略阻止更新".to_string()),
        )
        .unwrap_err();
        fs::remove_dir_all(&installed).unwrap();
        fs::rename(&previous, &installed).unwrap();

        assert_eq!(error, "安装策略阻止更新");
        assert!(fs::read_to_string(installed.join("SKILL.md"))
            .unwrap()
            .contains("更新版本"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn commit_reference_uses_shallow_fetch_instead_of_full_clone() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let root = test_dir("commit-shallow-fetch");
        let repo = root.join("source");
        let clone = root.join("clone");
        fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let output = run_git(args, Some(&repo), 10).unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "SkillMate Test"]);
        git(&["config", "user.email", "skillmate-test@example.com"]);
        fs::write(repo.join("SKILL.md"), "first").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "first"]);
        let first_ref = git_output(&repo, &["rev-parse", "HEAD"]).unwrap();
        for content in ["second", "third"] {
            fs::write(repo.join("SKILL.md"), content).unwrap();
            git(&["add", "."]);
            git(&["commit", "-m", content]);
        }
        let spec = GitInstallSpec {
            original: repo.to_string_lossy().to_string(),
            repo_url: repo.to_string_lossy().to_string(),
            reference: Some(first_ref.clone()),
            subdir: None,
            target_name: "writer".to_string(),
        };

        clone_git_install_spec(&spec, &clone).unwrap();

        assert_eq!(
            git_output(&clone, &["rev-parse", "HEAD"]).unwrap(),
            first_ref
        );
        assert_eq!(
            git_output(&clone, &["rev-list", "--count", "HEAD"]).unwrap(),
            "1"
        );
        assert!(clone.join(".git/shallow").exists());
        let _ = fs::remove_dir_all(root);
    }
}
