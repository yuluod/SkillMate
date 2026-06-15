use crate::app_core::expand_path;
use crate::managed_state::mark_managed_skill;
pub use crate::skill_install_source::{
    detect_install_source_rules, install_target_name, is_git_install_source,
    parse_git_install_spec, GitInstallSpec, InstallDetection,
};
use crate::skill_package::{detect_skill_package, DetectedSkill, PackageDetection};
use crate::skill_structure::{analyze_skill_structure, SkillStructureInfo};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::thread::sleep;
use std::time::{Duration, Instant};

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

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("本地目录不存在: {}", src.to_string_lossy()));
    }
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let entry_path = entry.path();
        let target_path = dst.join(entry.file_name());
        let metadata = fs::symlink_metadata(&entry_path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            copy_dir_recursive(&entry_path, &target_path)?;
        } else if metadata.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(&entry_path, &target_path).map_err(|e| e.to_string())?;
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
            return install_preview(
                false,
                false,
                err,
                "",
                target_root.to_string_lossy().to_string(),
                source,
                PackageDetection {
                    package_kind: "unknown".to_string(),
                    detected_skills: vec![],
                    warnings: vec!["unrecognized_input".to_string()],
                    needs_model: true,
                },
                vec![],
                vec![],
                None,
            )
        }
    };
    let target_path = target_root.join(&skill_name);
    let target_exists = target_path.exists();
    let source_kind = if is_git_install_source(source) {
        "git"
    } else {
        source
    };

    let package_result = match source {
        source_kind if is_git_install_source(source_kind) => {
            parse_git_install_spec(package).and_then(|spec| analyze_git_source_package(&spec))
        }
        "local" => Ok(detect_skill_package(&expand_path(package.trim()))),
        _ => Err("当前版本仅支持 Git 仓库和本地目录安装".to_string()),
    };

    match package_result {
        Ok(package_detection) => build_install_preview(
            package_detection,
            target_root,
            &skill_name,
            source_kind,
            target_exists,
        ),
        Err(err) => install_preview(
            false,
            false,
            err,
            skill_name,
            target_path.to_string_lossy().to_string(),
            source_kind,
            PackageDetection {
                package_kind: "unknown".to_string(),
                detected_skills: vec![],
                warnings: vec!["structure_preview_failed".to_string()],
                needs_model: true,
            },
            vec![],
            vec![PreviewConflict {
                target: target_path.to_string_lossy().to_string(),
                reason: "structure_preview_failed".to_string(),
            }],
            Some(SkillStructureInfo {
                structure_status: "nonstandard".to_string(),
                structure_features: vec![],
                structure_warnings: vec!["structure_preview_failed".to_string()],
                manifest_title: None,
                manifest_description: None,
            }),
        ),
    }
}

pub fn install_local_package(
    source_path: &Path,
    target_root: &Path,
    fallback_name: &str,
    assistant_name: &str,
) -> Result<SkillStructureInfo, String> {
    let package = detect_skill_package(source_path);
    let preview =
        build_install_preview(package.clone(), target_root, fallback_name, "local", false);
    if !preview.can_apply {
        return Err(preview.message);
    }
    let mut created_targets = Vec::new();
    let result = (|| {
        let mut first_structure = None;
        for skill in package.detected_skills {
            let source_skill_path = source_for_detected_skill(source_path, &skill);
            let target_path =
                target_root.join(target_name_for_detected_skill(&skill, fallback_name));
            copy_dir_recursive(&source_skill_path, &target_path)?;
            created_targets.push(target_path.clone());
            mark_managed_skill(
                target_root,
                assistant_name,
                &target_path,
                &format!("local:{}", source_skill_path.to_string_lossy()),
            )?;
            let structure = analyze_skill_structure(&target_path);
            if first_structure.is_none() {
                first_structure = Some(structure);
            }
        }
        first_structure.ok_or_else(|| "未识别到可安装的 Skill".to_string())
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
    build_symlink_install_preview(package, source_path, target_root, fallback_name)
}

pub fn install_local_symlink_package(
    source_path: &Path,
    target_root: &Path,
    fallback_name: &str,
    assistant_name: &str,
) -> Result<SkillStructureInfo, String> {
    let package = detect_skill_package(source_path);
    let preview =
        build_symlink_install_preview(package.clone(), source_path, target_root, fallback_name);
    if !preview.can_apply {
        return Err(preview.message);
    }
    fs::create_dir_all(target_root).map_err(|e| e.to_string())?;
    let mut created_targets = Vec::new();
    let result = (|| {
        let mut first_structure = None;
        for skill in package.detected_skills {
            let source_skill_path = source_for_detected_skill(source_path, &skill);
            let target_path =
                target_root.join(target_name_for_detected_skill(&skill, fallback_name));
            create_dir_symlink(&source_skill_path, &target_path)?;
            created_targets.push(target_path.clone());
            mark_managed_skill(
                target_root,
                assistant_name,
                &target_path,
                &format!("symlink:{}", source_skill_path.to_string_lossy()),
            )?;
            let structure = analyze_skill_structure(&source_skill_path);
            if first_structure.is_none() {
                first_structure = Some(structure);
            }
        }
        first_structure.ok_or_else(|| "未识别到可软连接安装的 Skill".to_string())
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

pub fn install_git_package(
    spec: GitInstallSpec,
    target_root: &Path,
    fallback_name: &str,
    assistant_name: &str,
    save_origin: impl Fn(&Path, &GitInstallSpec, &GitInstallOutcome) -> Result<(), String>,
) -> Result<GitInstallOutcome, String> {
    ensure_git_available()?;
    with_temp_git_source(&spec, |source_path, repo_path| {
        let package = detect_skill_package(source_path);
        let preview =
            build_install_preview(package.clone(), target_root, fallback_name, "git", false);
        if !preview.can_apply {
            return Err(preview.message);
        }
        let installed_ref = git_output(repo_path, &["rev-parse", "HEAD"]).unwrap_or_default();
        let mut created_targets = Vec::new();
        let result = (|| {
            let mut first_outcome = None;
            for skill in package.detected_skills {
                let source_skill_path = source_for_detected_skill(source_path, &skill);
                let target_path =
                    target_root.join(target_name_for_detected_skill(&skill, fallback_name));
                copy_dir_recursive(&source_skill_path, &target_path)?;
                created_targets.push(target_path.clone());
                let structure = analyze_skill_structure(&target_path);
                let mut skill_spec = spec_for_detected_skill(&spec, &skill);
                skill_spec.target_name = target_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| fallback_name.to_string());
                let outcome = GitInstallOutcome {
                    structure: structure.clone(),
                    installed_ref: installed_ref.clone(),
                };
                save_origin(&target_path, &skill_spec, &outcome)?;
                mark_managed_skill(
                    target_root,
                    assistant_name,
                    &target_path,
                    &skill_spec.original,
                )?;
                if first_outcome.is_none() {
                    first_outcome = Some(outcome);
                }
            }
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
    })
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

pub fn sync_git_subdir_skill(
    origin_locator: &str,
    resolved_locator: &str,
    tracking_ref: &str,
    target_path: &Path,
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
    if spec.subdir.is_none() {
        return Err("未记录 Git 子目录来源，无法执行子目录更新".to_string());
    }

    ensure_git_available()?;
    with_temp_git_source(&spec, |source_path, repo_path| {
        let parent = target_path
            .parent()
            .ok_or_else(|| "目标路径缺少父目录".to_string())?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let staging = parent.join(format!(".skillmate-sync-{}", now_ms()));
        let copy_result = (|| {
            copy_dir_recursive(source_path, &staging)?;
            let structure = analyze_skill_structure(&staging);
            replace_path_with_staging(&staging, target_path)?;
            let installed_ref = git_output(repo_path, &["rev-parse", "HEAD"]).unwrap_or_default();
            Ok(GitInstallOutcome {
                structure,
                installed_ref,
            })
        })();
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        copy_result
    })
}

fn replace_path_with_staging(staging: &Path, target_path: &Path) -> Result<(), String> {
    let backup = target_path.with_file_name(format!(
        ".{}.skillmate-backup-{}",
        target_path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| "skill".into()),
        now_ms()
    ));
    replace_path_with_staging_at(
        staging,
        target_path,
        &backup,
        |from, to| fs::rename(from, to).map_err(|e| e.to_string()),
        |path| remove_existing_path(path),
    )
}

fn replace_path_with_staging_at(
    staging: &Path,
    target_path: &Path,
    backup: &Path,
    mut rename_path: impl FnMut(&Path, &Path) -> Result<(), String>,
    mut remove_path: impl FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    if !staging.exists() {
        return Err("临时更新目录不存在".to_string());
    }
    let had_target = target_path.exists();

    if had_target {
        if backup.exists() {
            remove_path(backup)?;
        }
        rename_path(target_path, backup)?;
    }

    match rename_path(staging, target_path) {
        Ok(_) => {
            if backup.exists() {
                let _ = remove_path(backup);
            }
            Ok(())
        }
        Err(err) => {
            if had_target && backup.exists() && !target_path.exists() {
                let _ = rename_path(backup, target_path);
            }
            Err(err)
        }
    }
}

pub fn probe_git_subdir_latest_ref(
    origin_locator: &str,
    resolved_locator: &str,
    tracking_ref: &str,
) -> Result<String, String> {
    let spec = git_subdir_spec(origin_locator, resolved_locator, tracking_ref)?;
    ensure_git_available()?;
    with_temp_git_source(&spec, |_, repo_path| {
        git_output(repo_path, &["rev-parse", "HEAD"])
    })
}

pub fn has_git_subdir_spec(origin_locator: &str, resolved_locator: &str) -> bool {
    git_subdir_spec(origin_locator, resolved_locator, "").is_ok()
}

fn git_subdir_spec(
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
    if spec.subdir.is_some() {
        Ok(spec)
    } else {
        Err("未记录 Git 子目录来源".to_string())
    }
}

fn install_preview(
    can_install: bool,
    can_apply: bool,
    message: impl Into<String>,
    target_name: impl Into<String>,
    target_path: impl Into<String>,
    source_kind: impl Into<String>,
    package_detection: PackageDetection,
    target_actions: Vec<PreviewAction>,
    conflicts: Vec<PreviewConflict>,
    structure: Option<SkillStructureInfo>,
) -> InstallPreview {
    let structure = structure.unwrap_or_default();
    InstallPreview {
        can_install,
        can_apply,
        message: message.into(),
        target_name: target_name.into(),
        target_path: target_path.into(),
        source_kind: source_kind.into(),
        package_detection,
        target_actions,
        conflicts,
        structure_status: structure.structure_status,
        structure_features: structure.structure_features,
        structure_warnings: structure.structure_warnings,
        manifest_title: structure.manifest_title,
        manifest_description: structure.manifest_description,
    }
}

fn analyze_git_source_package(spec: &GitInstallSpec) -> Result<PackageDetection, String> {
    ensure_git_available()?;
    with_temp_git_source(spec, |source_path, _| Ok(detect_skill_package(source_path)))
}

fn build_install_preview(
    mut package_detection: PackageDetection,
    target_root: &Path,
    fallback_name: &str,
    source_kind: &str,
    legacy_target_exists: bool,
) -> InstallPreview {
    let mut actions = Vec::new();
    let mut conflicts = Vec::new();
    if package_detection.detected_skills.is_empty() {
        conflicts.push(PreviewConflict {
            target: target_root.to_string_lossy().to_string(),
            reason: "unrecognized_input".to_string(),
        });
    }
    for skill in &package_detection.detected_skills {
        let target = target_root.join(target_name_for_detected_skill(skill, fallback_name));
        let target_string = target.to_string_lossy().to_string();
        if target.exists() {
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
    install_preview(
        can_apply,
        can_apply,
        if can_apply {
            format!("将安装 {} 个 Skill", actions.len())
        } else if conflicts.is_empty() {
            "未识别到可安装的 Skill".to_string()
        } else {
            format!("发现 {} 个安装冲突", conflicts.len())
        },
        target_name,
        target_path,
        source_kind,
        package_detection,
        actions,
        conflicts,
        Some(primary),
    )
}

fn build_symlink_install_preview(
    mut package_detection: PackageDetection,
    source_root: &Path,
    target_root: &Path,
    fallback_name: &str,
) -> InstallPreview {
    let mut actions = Vec::new();
    let mut conflicts = Vec::new();
    if package_detection.detected_skills.is_empty() {
        conflicts.push(PreviewConflict {
            target: target_root.to_string_lossy().to_string(),
            reason: "unrecognized_input".to_string(),
        });
    }
    for skill in &package_detection.detected_skills {
        let source = source_for_detected_skill(source_root, skill);
        let target = target_root.join(target_name_for_detected_skill(skill, fallback_name));
        let source_string = source.to_string_lossy().to_string();
        let target_string = target.to_string_lossy().to_string();
        if !source.is_dir() {
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
    install_preview(
        can_apply,
        can_apply,
        if can_apply {
            format!("将软连接安装 {} 个 Skill", actions.len())
        } else if conflicts.is_empty() {
            "未识别到可软连接安装的 Skill".to_string()
        } else {
            format!("发现 {} 个软连接安装冲突", conflicts.len())
        },
        target_name,
        target_path,
        "local_symlink",
        package_detection,
        actions,
        conflicts,
        Some(primary),
    )
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
        return fallback_name.to_string();
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

        let full = run_git(
            &["clone", spec.repo_url.as_str(), clone_target.as_str()],
            None,
            120,
        )?;
        if !full.status.success() {
            return Err(format_git_error(&full));
        }
        let checkout = run_git(&["checkout", reference], Some(clone_path), 30)?;
        if checkout.status.success() {
            Ok(())
        } else {
            Err(format_git_error(&checkout))
        }
    } else {
        let out = run_git(
            &[
                "clone",
                "--depth",
                "1",
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

fn with_temp_git_source<T>(
    spec: &GitInstallSpec,
    action: impl FnOnce(&Path, &Path) -> Result<T, String>,
) -> Result<T, String> {
    let temp_root = std::env::temp_dir().join(format!("skillmate-git-source-{}", now_ms()));
    fs::create_dir_all(&temp_root).map_err(|e| e.to_string())?;
    let clone_path = temp_root.join("repo");
    let result = (|| {
        clone_git_install_spec(spec, &clone_path)?;
        let source_path = if let Some(subdir) = spec.subdir.as_deref() {
            let path = clone_path.join(subdir);
            if !path.is_dir() {
                return Err(format!("仓库子目录不存在: {}", subdir));
            }
            path
        } else {
            clone_path.clone()
        };
        action(&source_path, &clone_path)
    })();
    let _ = fs::remove_dir_all(&temp_root);
    result
}

fn run_git(args: &[&str], current_dir: Option<&Path>, timeout_secs: u64) -> Result<Output, String> {
    run_command_with_timeout(
        "git",
        args,
        current_dir,
        Duration::from_secs(timeout_secs),
        &[("GIT_TERMINAL_PROMPT", "0"), ("GCM_INTERACTIVE", "Never")],
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

fn run_command_with_timeout(
    cmd: &str,
    args: &[&str],
    current_dir: Option<&Path>,
    timeout: Duration,
    envs: &[(&str, &str)],
) -> Result<Output, String> {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => return child.wait_with_output().map_err(|e| e.to_string()),
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("命令执行超时（{} 秒）", timeout.as_secs()));
            }
            None => sleep(Duration::from_millis(100)),
        }
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("skillmate-install-test-{}-{}", name, now_ms()))
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

        copy_dir_recursive(&src, &dst).unwrap();

        assert_eq!(fs::read_to_string(dst.join("SKILL.md")).unwrap(), "skill");
        assert!(!dst.join("outside-link").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replace_path_with_staging_restores_existing_target_on_failure() {
        use std::cell::Cell;

        let root = test_dir("restore-target");
        let target = root.join("skill");
        let staging = root.join("staging");
        let backup = root.join(".skill.backup");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(target.join("SKILL.md"), "old").unwrap();
        fs::write(staging.join("SKILL.md"), "new").unwrap();

        let calls = Cell::new(0);
        let result = replace_path_with_staging_at(
            &staging,
            &target,
            &backup,
            |from, to| {
                let call = calls.get() + 1;
                calls.set(call);
                if call == 2 {
                    return Err("模拟 staging 落位失败".to_string());
                }
                fs::rename(from, to).map_err(|e| e.to_string())
            },
            remove_existing_path,
        );

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(target.join("SKILL.md")).unwrap(), "old");
        assert!(staging.exists());
        assert!(!backup.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replace_path_with_staging_ignores_backup_cleanup_failure_after_success() {
        use std::cell::Cell;

        let root = test_dir("cleanup-failure");
        let target = root.join("skill");
        let staging = root.join("staging");
        let backup = root.join(".skill.backup");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(target.join("SKILL.md"), "old").unwrap();
        fs::write(staging.join("SKILL.md"), "new").unwrap();

        let cleanup_calls = Cell::new(0);
        let result = replace_path_with_staging_at(
            &staging,
            &target,
            &backup,
            |from, to| fs::rename(from, to).map_err(|e| e.to_string()),
            |path| {
                let call = cleanup_calls.get() + 1;
                cleanup_calls.set(call);
                if call == 1 {
                    return Err("模拟 backup 清理失败".to_string());
                }
                remove_existing_path(path)
            },
        );

        assert!(result.is_ok());
        assert_eq!(fs::read_to_string(target.join("SKILL.md")).unwrap(), "new");
        assert!(backup.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_symlink_install_creates_project_link() {
        let root = test_dir("project-symlink");
        let source = root.join("source");
        let target_root = root.join("project/.codex/skills");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "# Writer\n\n说明").unwrap();
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
            "# Writer\n\n说明"
        );
        let _ = fs::remove_dir_all(root);
    }
}
