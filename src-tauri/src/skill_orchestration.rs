use crate::app_core::expand_path;
use crate::install_policy::{evaluate_install_policy, load_install_policy, InstallPolicyInput};
use crate::managed_installation::{
    cleanup_skill_metadata, find_managed_installation, is_explicitly_managed,
    list_managed_installations, record_managed_root, refresh_managed_installation,
    register_managed_root, verify_managed_content_unchanged, ManagedMetadataCheckpoint,
};
use crate::managed_state::{content_fingerprint, managed_state_origin};
use crate::operation_plan::{operation_plan_token, verify_operation_plan};
use crate::skill_install::{
    install_git_package_at_ref, install_local_package_at_digest,
    install_local_symlink_package_at_digest, is_git_install_source, parse_git_install_spec,
    InstallPreview,
};
use crate::skill_inventory::scan_all_assistants;
use crate::skill_origin::{load_origin_meta, save_installed_git_meta};
use crate::skill_profile::{
    previous_active_profile_id, read_skill_profiles, rollback_active_profile, set_active_profile,
    upsert_skill_profile, validate_skill_profile, SkillSetProfileDiff, SkillSetProfilePreview,
    SkillSetProfileStore,
};
use crate::skill_reconcile::ReconcileTransaction;
use crate::skillmate_manifest::{
    manifest_target_root, preview_skillmate_manifest_with_existing, resolved_manifest_source,
    sort_manifest_skills, ExistingTargetDisposition, SkillMateManifest, SkillMateManifestAction,
    SkillMateManifestPreview, SkillMateManifestSkill,
};
use rusqlite::Connection;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestApplySummary {
    pub installed: usize,
    pub removed: usize,
    pub kept: usize,
    pub warnings: Vec<String>,
}

impl ManifestApplySummary {
    pub fn message(&self, subject: &str) -> String {
        let mut message = format!(
            "已应用 {}：安装 {} 条，保留 {} 条，移除 {} 条",
            subject, self.installed, self.kept, self.removed
        );
        if !self.warnings.is_empty() {
            message.push_str(&format!("；警告：{}", self.warnings.join("；")));
        }
        message
    }
}

pub fn build_current_manifest(db: &Connection) -> Result<SkillMateManifest, String> {
    let mut skills = Vec::new();
    let mut registered_paths = HashSet::new();
    for installation in list_managed_installations(db)? {
        registered_paths.insert(installation.path.clone());
        if installation.path.exists() || fs::symlink_metadata(&installation.path).is_ok() {
            let mut skill = installation.skill;
            skill.content_hash = Some(content_fingerprint(&installation.path)?);
            skills.push(skill);
        }
    }
    for assistant in scan_all_assistants(db)? {
        for skill in assistant.skills.into_iter().filter(|skill| {
            skill.origin.managed_by_app
                && !registered_paths.contains(Path::new(&skill.inventory.path))
        }) {
            let source_kind = if skill.origin.origin_kind == "git" {
                "git".to_string()
            } else {
                "local".to_string()
            };
            let skill_path = PathBuf::from(&skill.inventory.path);
            let state_origin = match skill_path.parent() {
                Some(root) => managed_state_origin(root, &skill_path)?,
                None => None,
            };
            let source = if let Some(symlink_source) = skill.origin.symlink_source.clone() {
                symlink_source
            } else if skill.origin.origin_kind == "git" {
                if skill.origin.origin_locator.trim().is_empty() {
                    skill.origin.resolved_locator
                } else {
                    skill.origin.origin_locator
                }
            } else if let Some(local_source) = state_origin
                .as_deref()
                .and_then(|value| value.strip_prefix("local:"))
            {
                local_source.to_string()
            } else {
                skill.inventory.path.clone()
            };
            let parsed_git = if source_kind == "git" {
                parse_git_install_spec(&source).ok()
            } else {
                None
            };
            skills.push(SkillMateManifestSkill {
                assistant: assistant.name.clone(),
                source,
                source_kind,
                target_name: Some(skill.inventory.name),
                scope: Some("global".to_string()),
                install_mode: Some(if skill.origin.symlink_source.is_some() {
                    "symlink".to_string()
                } else {
                    "copy".to_string()
                }),
                project_path: None,
                reference: parsed_git.as_ref().and_then(|spec| spec.reference.clone()),
                subdir: parsed_git.as_ref().and_then(|spec| spec.subdir.clone()),
                resolved_ref: (!skill.origin.installed_ref.is_empty())
                    .then_some(skill.origin.installed_ref),
                content_hash: Some(content_fingerprint(&skill_path)?),
            });
        }
    }
    sort_manifest_skills(&mut skills);
    Ok(SkillMateManifest {
        version: 2,
        reconcile: true,
        skills,
    })
}

pub fn build_project_manifest(
    db: &Connection,
    project_path: &Path,
) -> Result<SkillMateManifest, String> {
    project_manifest_from(build_current_manifest(db)?, project_path)
}

fn project_manifest_from(
    mut manifest: SkillMateManifest,
    project_path: &Path,
) -> Result<SkillMateManifest, String> {
    let project_identity = comparable_project_path(project_path)?;
    manifest.skills.retain(|skill| {
        if skill.scope.as_deref() != Some("project") {
            return false;
        }
        skill
            .project_path
            .as_deref()
            .and_then(|path| comparable_project_path(&expand_path(path)).ok())
            .map(|path| path == project_identity)
            .unwrap_or(false)
    });
    for skill in &mut manifest.skills {
        skill.project_path = Some(project_path.to_string_lossy().to_string());
    }
    sort_manifest_skills(&mut manifest.skills);
    Ok(manifest)
}

fn comparable_project_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    if absolute.exists() {
        absolute.canonicalize().map_err(|error| error.to_string())
    } else {
        Ok(absolute)
    }
}

pub fn preview_manifest(
    db: &Connection,
    manifest: &SkillMateManifest,
) -> Result<SkillMateManifestPreview, String> {
    let mut preview = preview_skillmate_manifest_with_existing(manifest, |skill, target_path| {
        manifest_target_disposition(db, skill, target_path)
    })?;
    if preview.validation_issues.is_empty() {
        if preview.install_previews.len() != manifest.skills.len() {
            return Err("manifest 策略检查与安装预览数量不一致".to_string());
        }
        let policy = load_install_policy(db)?;
        for (skill, install_preview) in manifest.skills.iter().zip(&mut preview.install_previews) {
            let source = resolved_manifest_source(skill)?;
            let mut warnings = install_preview.structure_warnings.clone();
            warnings.extend(install_preview.package_detection.warnings.iter().cloned());
            for detected in &install_preview.package_detection.detected_skills {
                warnings.extend(detected.warnings.iter().cloned());
            }
            warnings.sort();
            warnings.dedup();
            let decision = evaluate_install_policy(
                &policy,
                InstallPolicyInput {
                    source_kind: &skill.source_kind,
                    source: &source,
                    structure_status: &install_preview.structure_status,
                    warnings: &warnings,
                },
            );
            if !decision.allowed {
                install_preview.can_apply = false;
                install_preview.can_install = false;
                install_preview.message = decision.message.clone();
                install_preview
                    .conflicts
                    .push(crate::skill_install::PreviewConflict {
                        target: install_preview.target_path.clone(),
                        reason: "install_policy_blocked".to_string(),
                    });
                preview.can_apply = false;
                preview
                    .conflicts
                    .push(crate::skillmate_manifest::SkillMateManifestConflict {
                        assistant: skill.assistant.clone(),
                        source: skill.source.clone(),
                        reason: decision.message.clone(),
                    });
            }
            install_preview.install_policy = decision;
        }
    }
    for removal in manifest_removals(db, manifest)? {
        match verify_managed_content_unchanged(db, &removal.path) {
            Ok(()) => preview.actions.push(SkillMateManifestAction {
                kind: "remove".to_string(),
                assistant: removal.skill.assistant,
                source: removal.path.to_string_lossy().to_string(),
                target_name: removal
                    .path
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_default(),
                message: "移除目标状态中不再声明的受管 Skill".to_string(),
            }),
            Err(error) => {
                preview.can_apply = false;
                preview
                    .conflicts
                    .push(crate::skillmate_manifest::SkillMateManifestConflict {
                        assistant: removal.skill.assistant,
                        source: removal.path.to_string_lossy().to_string(),
                        reason: error,
                    });
            }
        }
    }
    preview.plan_token.clear();
    preview.plan_token = operation_plan_token("skillmate-manifest", &(manifest, &preview))?;
    Ok(preview)
}

pub fn apply_manifest(
    db: &Connection,
    manifest: &SkillMateManifest,
) -> Result<ManifestApplySummary, String> {
    let preview = preview_manifest(db, manifest)?;
    apply_manifest_previewed(db, manifest, preview)
}

pub fn apply_manifest_with_plan(
    db: &Connection,
    manifest: &SkillMateManifest,
    plan_token: Option<&str>,
) -> Result<ManifestApplySummary, String> {
    let preview = preview_manifest(db, manifest)?;
    verify_operation_plan(&preview.plan_token, plan_token)?;
    apply_manifest_previewed(db, manifest, preview)
}

fn apply_manifest_previewed(
    db: &Connection,
    manifest: &SkillMateManifest,
    preview: SkillMateManifestPreview,
) -> Result<ManifestApplySummary, String> {
    if !preview.can_apply {
        return Err(format!(
            "manifest 存在 {} 个冲突和 {} 个格式问题，请先处理预览",
            preview.conflicts.len(),
            preview.validation_issues.len()
        ));
    }
    if preview.install_previews.len() != manifest.skills.len() {
        return Err("manifest 预览与声明记录数量不一致，请重新预览".to_string());
    }
    let removals = manifest_removals(db, manifest)?;
    let desired_paths = preview
        .install_previews
        .iter()
        .map(|item| PathBuf::from(&item.target_path))
        .collect::<HashSet<_>>();
    let pure_removals = removals
        .iter()
        .filter(|removal| !desired_paths.contains(&removal.path))
        .collect::<Vec<_>>();
    let install_targets = preview
        .install_previews
        .iter()
        .flat_map(|item| item.target_actions.iter())
        .filter(|action| matches!(action.action.as_str(), "copy" | "symlink" | "replace"))
        .map(|action| PathBuf::from(&action.target))
        .collect::<Vec<_>>();
    let mut removal_paths = pure_removals
        .iter()
        .map(|removal| removal.path.clone())
        .collect::<Vec<_>>();
    removal_paths.extend(
        preview
            .install_previews
            .iter()
            .flat_map(|item| item.target_actions.iter())
            .filter(|action| action.action == "replace")
            .map(|action| PathBuf::from(&action.target)),
    );
    removal_paths.sort();
    removal_paths.dedup();
    for (skill, install_preview) in manifest.skills.iter().zip(&preview.install_previews) {
        let target = PathBuf::from(&install_preview.target_path);
        let root = target
            .parent()
            .ok_or_else(|| "manifest 目标路径缺少父目录".to_string())?;
        register_managed_root(
            db,
            root,
            skill.scope.as_deref().unwrap_or("global"),
            skill.project_path.as_deref(),
        )?;
    }
    let mut metadata_paths = install_targets.clone();
    metadata_paths.extend(pure_removals.iter().map(|removal| removal.path.clone()));
    metadata_paths.sort();
    metadata_paths.dedup();
    let metadata_checkpoint = ManagedMetadataCheckpoint::capture(db, &metadata_paths)?;
    let mut transaction = ReconcileTransaction::prepare(&removal_paths, &install_targets)?;
    let mut summary = ManifestApplySummary::default();

    for (skill, install_preview) in manifest.skills.iter().zip(&preview.install_previews) {
        let target_path = PathBuf::from(&install_preview.target_path);
        let target_root = target_path
            .parent()
            .ok_or_else(|| "manifest 目标路径缺少父目录".to_string())?
            .to_path_buf();
        let action = install_preview
            .target_actions
            .first()
            .map(|action| action.action.as_str())
            .unwrap_or_default();
        if action == "keep" {
            summary.kept += 1;
            continue;
        }
        if let Err(error) = apply_manifest_skill(db, skill.clone(), install_preview) {
            return Err(rollback_manifest_attempt(
                db,
                &metadata_checkpoint,
                &install_targets,
                &mut transaction,
                "应用 manifest 失败",
                &error,
            ));
        }
        if let Err(error) = record_managed_root(
            db,
            &target_root,
            skill.scope.as_deref().unwrap_or("global"),
            skill.project_path.as_deref(),
        ) {
            return Err(rollback_manifest_attempt(
                db,
                &metadata_checkpoint,
                &install_targets,
                &mut transaction,
                "记录受管安装失败",
                &error,
            ));
        }
        if let Err(error) = refresh_managed_installation(
            db,
            &target_path,
            (!install_preview.resolved_ref.trim().is_empty())
                .then_some(install_preview.resolved_ref.as_str()),
        ) {
            return Err(rollback_manifest_attempt(
                db,
                &metadata_checkpoint,
                &install_targets,
                &mut transaction,
                "刷新受管安装状态失败",
                &error,
            ));
        }
        summary.installed += 1;
    }

    for removal in &pure_removals {
        if let Err(error) = cleanup_skill_metadata(db, &removal.path) {
            return Err(rollback_manifest_attempt(
                db,
                &metadata_checkpoint,
                &install_targets,
                &mut transaction,
                "清理 manifest 移除项失败",
                &error,
            ));
        }
    }
    if let Err(error) = transaction.commit() {
        summary.warnings.push(error);
    }
    summary.removed = pure_removals.len();
    Ok(summary)
}

pub fn save_current_profile(
    db: &Connection,
    name: &str,
    description: &str,
) -> Result<SkillSetProfileStore, String> {
    let manifest = build_current_manifest(db)?;
    upsert_skill_profile(name, description, manifest.skills)
}

pub fn preview_profile(
    db: &Connection,
    profile_id: &str,
) -> Result<SkillSetProfilePreview, String> {
    let store = read_skill_profiles()?;
    let profile = store
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .cloned()
        .ok_or_else(|| "Profile 不存在".to_string())?;
    let profile_issues = validate_skill_profile(&profile, &store.profiles);
    let current = build_current_manifest(db)?;
    let manifest = SkillMateManifest {
        version: 2,
        reconcile: true,
        skills: profile.skills.clone(),
    };
    let manifest_preview = preview_manifest(db, &manifest)?;
    let diff = build_profile_diff(&current, &manifest, &manifest_preview);
    let mut preview = SkillSetProfilePreview {
        profile,
        profile_issues,
        diff,
        manifest_preview,
        plan_token: String::new(),
    };
    preview.plan_token = operation_plan_token(
        "profile-apply",
        &(
            store.active_profile_id.as_deref(),
            store.previous_active_profile_id.as_deref(),
            &preview,
        ),
    )?;
    Ok(preview)
}

pub fn apply_profile_with_plan(
    db: &Connection,
    profile_id: &str,
    plan_token: Option<&str>,
) -> Result<String, String> {
    let preview = preview_profile(db, profile_id)?;
    verify_operation_plan(&preview.plan_token, plan_token)?;
    apply_profile_previewed(db, profile_id, preview)
}

fn apply_profile_previewed(
    db: &Connection,
    profile_id: &str,
    preview: SkillSetProfilePreview,
) -> Result<String, String> {
    let previous_manifest = build_current_manifest(db)?;
    apply_then_persist(
        || apply_profile_preview_contents(db, preview),
        || set_active_profile(profile_id).map(|_| ()),
        || apply_manifest(db, &previous_manifest).map(|_| ()),
    )
}

pub fn rollback_profile(db: &Connection) -> Result<String, String> {
    let previous_profile_id = previous_active_profile_id()?;
    let previous_manifest = build_current_manifest(db)?;
    let result = apply_then_persist(
        || apply_profile_contents(db, &previous_profile_id),
        || rollback_active_profile().map(|_| ()),
        || apply_manifest(db, &previous_manifest).map(|_| ()),
    )?;
    Ok(format!("{}；已回滚到上一个 Profile", result))
}

fn apply_then_persist(
    apply: impl FnOnce() -> Result<String, String>,
    persist: impl FnOnce() -> Result<(), String>,
    restore: impl FnOnce() -> Result<(), String>,
) -> Result<String, String> {
    let result = apply()?;
    if let Err(error) = persist() {
        return match restore() {
            Ok(()) => Err(format!(
                "保存 Profile 激活状态失败，已恢复原组合: {}",
                error
            )),
            Err(restore_error) => Err(format!(
                "保存 Profile 激活状态失败: {}；恢复原组合失败: {}",
                error, restore_error
            )),
        };
    }
    Ok(result)
}

fn apply_profile_contents(db: &Connection, profile_id: &str) -> Result<String, String> {
    let preview = preview_profile(db, profile_id)?;
    apply_profile_preview_contents(db, preview)
}

fn apply_profile_preview_contents(
    db: &Connection,
    preview: SkillSetProfilePreview,
) -> Result<String, String> {
    if !preview.profile_issues.is_empty() {
        return Err("Profile 格式存在问题，请先处理预览".to_string());
    }
    if !preview.manifest_preview.can_apply {
        return Err(format!(
            "Profile 存在 {} 个冲突，请先处理预览",
            preview.manifest_preview.conflicts.len()
        ));
    }
    let manifest = SkillMateManifest {
        version: 2,
        reconcile: true,
        skills: preview.profile.skills,
    };
    Ok(
        apply_manifest_with_plan(db, &manifest, Some(&preview.manifest_preview.plan_token))?
            .message("Profile"),
    )
}

#[derive(Debug, Clone)]
struct ManifestRemoval {
    skill: SkillMateManifestSkill,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ManifestReconcileScope {
    All,
    Project(PathBuf),
}

fn manifest_removals(
    db: &Connection,
    target: &SkillMateManifest,
) -> Result<Vec<ManifestRemoval>, String> {
    if !target.reconcile {
        return Ok(Vec::new());
    }
    let reconcile_scope = manifest_reconcile_scope(target)?;
    let target_keys = target
        .skills
        .iter()
        .map(manifest_skill_key)
        .collect::<HashSet<_>>();
    Ok(build_current_manifest(db)?
        .skills
        .into_iter()
        .filter(|skill| match &reconcile_scope {
            ManifestReconcileScope::All => true,
            ManifestReconcileScope::Project(project) => {
                skill.scope.as_deref() == Some("project")
                    && skill
                        .project_path
                        .as_deref()
                        .and_then(|path| comparable_project_path(&expand_path(path)).ok())
                        .as_ref()
                        == Some(project)
            }
        })
        .filter(|skill| !target_keys.contains(&manifest_skill_key(skill)))
        .filter_map(|skill| {
            let root = manifest_target_root(&skill).ok()?;
            let target_name = skill.target_name.as_deref()?;
            Some(ManifestRemoval {
                path: root.join(target_name),
                skill,
            })
        })
        .collect())
}

fn manifest_reconcile_scope(
    manifest: &SkillMateManifest,
) -> Result<ManifestReconcileScope, String> {
    if manifest.skills.is_empty()
        || manifest
            .skills
            .iter()
            .any(|skill| skill.scope.as_deref() != Some("project") || skill.project_path.is_none())
    {
        return Ok(ManifestReconcileScope::All);
    }
    let mut projects = manifest
        .skills
        .iter()
        .filter_map(|skill| skill.project_path.as_deref())
        .map(|path| comparable_project_path(&expand_path(path)));
    let Some(first) = projects.next().transpose()? else {
        return Ok(ManifestReconcileScope::All);
    };
    for project in projects {
        if project? != first {
            return Ok(ManifestReconcileScope::All);
        }
    }
    Ok(ManifestReconcileScope::Project(first))
}

fn manifest_target_matches(
    db: &Connection,
    skill: &SkillMateManifestSkill,
    target_path: &Path,
) -> Result<bool, String> {
    let effective_source = resolved_manifest_source(skill).unwrap_or_else(|_| skill.source.clone());
    if let Some(installation) = find_managed_installation(db, target_path)? {
        if !target_path.exists() && fs::symlink_metadata(target_path).is_err() {
            return Ok(false);
        }
        let registered_source = resolved_manifest_source(&installation.skill)
            .unwrap_or_else(|_| installation.skill.source.clone());
        let metadata_matches = installation.skill.assistant == skill.assistant
            && installation.skill.source_kind == skill.source_kind
            && registered_source == effective_source
            && installation.skill.scope.as_deref().unwrap_or("global")
                == skill.scope.as_deref().unwrap_or("global")
            && installation.skill.install_mode.as_deref().unwrap_or("copy")
                == skill.install_mode.as_deref().unwrap_or("copy");
        let content_matches = match skill.content_hash.as_deref() {
            Some(expected) => content_fingerprint(target_path)? == expected,
            None => true,
        };
        return Ok(metadata_matches && content_matches);
    }
    if is_git_install_source(&skill.source_kind) {
        return Ok(load_origin_meta(db, &target_path.to_string_lossy())?
            .filter(|meta| meta.managed_by_app)
            .map(|meta| {
                [meta.origin_locator, meta.resolved_locator]
                    .iter()
                    .any(|origin| origin == &skill.source || origin == &effective_source)
            })
            .unwrap_or(false));
    }

    let expected_mode = if skill.install_mode.as_deref() == Some("symlink") {
        "symlink:"
    } else {
        "local:"
    };
    let origin = match target_path.parent() {
        Some(root) => managed_state_origin(root, target_path)?,
        None => None,
    };
    let Some(origin) = origin else {
        return Ok(false);
    };
    let Some(origin_path) = origin.strip_prefix(expected_mode) else {
        return Ok(false);
    };
    Ok(paths_refer_to_same_location(
        &expand_path(origin_path),
        &expand_path(&effective_source),
    ))
}

fn manifest_target_disposition(
    db: &Connection,
    skill: &SkillMateManifestSkill,
    target_path: &Path,
) -> Result<ExistingTargetDisposition, String> {
    if !target_path.exists() && fs::symlink_metadata(target_path).is_err() {
        return Ok(ExistingTargetDisposition::Missing);
    }
    if manifest_target_matches(db, skill, target_path)? {
        return Ok(ExistingTargetDisposition::Matching);
    }
    if is_explicitly_managed(db, target_path)? {
        match verify_managed_content_unchanged(db, target_path) {
            Ok(()) => Ok(ExistingTargetDisposition::Replaceable),
            Err(_) => Ok(ExistingTargetDisposition::Drifted),
        }
    } else {
        Ok(ExistingTargetDisposition::Unmanaged)
    }
}

fn paths_refer_to_same_location(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn build_profile_diff(
    current: &SkillMateManifest,
    target: &SkillMateManifest,
    preview: &SkillMateManifestPreview,
) -> SkillSetProfileDiff {
    let current_keys = current
        .skills
        .iter()
        .map(manifest_skill_key)
        .collect::<HashSet<_>>();
    let target_keys = target
        .skills
        .iter()
        .map(manifest_skill_key)
        .collect::<HashSet<_>>();
    let conflicts = preview
        .conflicts
        .iter()
        .map(|conflict| format!("{}: {}", conflict.assistant, conflict.reason))
        .chain(
            preview
                .validation_issues
                .iter()
                .map(|issue| format!("#{}: {}", issue.index + 1, issue.message)),
        )
        .collect();
    let mut to_install = Vec::new();
    let mut already_present = Vec::new();
    for skill in &target.skills {
        let key = manifest_skill_key(skill);
        if current_keys.contains(&key) {
            already_present.push(key);
        } else {
            to_install.push(key);
        }
    }
    let mut to_remove = current_keys
        .difference(&target_keys)
        .cloned()
        .collect::<Vec<_>>();
    to_remove.sort();
    SkillSetProfileDiff {
        to_install,
        already_present,
        to_remove,
        conflicts,
    }
}

fn manifest_skill_key(skill: &SkillMateManifestSkill) -> String {
    let source = resolved_manifest_source(skill).unwrap_or_else(|_| skill.source.clone());
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}",
        skill.assistant,
        skill
            .target_name
            .clone()
            .unwrap_or_else(|| skill.source.clone()),
        skill.source_kind,
        skill.scope.as_deref().unwrap_or("global"),
        skill.install_mode.as_deref().unwrap_or("copy"),
        skill.project_path.as_deref().unwrap_or(""),
        source,
        skill.reference.as_deref().unwrap_or(""),
        skill.subdir.as_deref().unwrap_or("")
    )
}

fn apply_manifest_skill(
    db: &Connection,
    skill: SkillMateManifestSkill,
    preview: &InstallPreview,
) -> Result<(), String> {
    let target_path = PathBuf::from(&preview.target_path);
    let target_root = target_path
        .parent()
        .ok_or_else(|| "manifest 目标路径缺少父目录".to_string())?
        .to_path_buf();
    fs::create_dir_all(&target_root).map_err(|error| error.to_string())?;
    let effective_source = resolved_manifest_source(&skill)?;
    let fallback_name = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "manifest 目标路径缺少目录名".to_string())?
        .to_string();
    if skill.install_mode.as_deref() == Some("symlink") {
        let source_path = expand_path(effective_source.trim());
        install_local_symlink_package_at_digest(
            &source_path,
            &target_root,
            &fallback_name,
            &skill.assistant,
            Some(&preview.source_digest),
        )?;
        return Ok(());
    }
    match skill.source_kind.as_str() {
        source_kind if is_git_install_source(source_kind) => {
            let spec = parse_git_install_spec(&effective_source)?;
            install_git_package_at_ref(
                spec,
                &target_root,
                &fallback_name,
                &skill.assistant,
                Some(&preview.resolved_ref),
                |target_path, spec, outcome| {
                    save_installed_git_meta(db, target_path, spec, outcome)
                },
            )?;
        }
        "local" => {
            let source_path = expand_path(effective_source.trim());
            install_local_package_at_digest(
                &source_path,
                &target_root,
                &fallback_name,
                &skill.assistant,
                Some(&preview.source_digest),
            )?;
        }
        _ => return Err("当前 manifest 仅支持 Git 仓库和本地目录来源".to_string()),
    }
    Ok(())
}

fn cleanup_targets(db: &Connection, targets: &[PathBuf]) -> Vec<String> {
    targets
        .iter()
        .filter_map(|target| {
            cleanup_skill_metadata(db, target)
                .err()
                .map(|error| format!("{}: {}", target.to_string_lossy(), error))
        })
        .collect()
}

fn rollback_manifest_attempt(
    db: &Connection,
    metadata_checkpoint: &ManagedMetadataCheckpoint,
    install_targets: &[PathBuf],
    transaction: &mut ReconcileTransaction,
    subject: &str,
    error: &str,
) -> String {
    let mut cleanup_errors = cleanup_targets(db, install_targets);
    if let Err(rollback_error) = transaction.rollback() {
        cleanup_errors.push(format!("文件回滚失败: {}", rollback_error));
    }
    if let Err(metadata_error) = metadata_checkpoint.restore(db) {
        cleanup_errors.push(format!("元数据回滚失败: {}", metadata_error));
    }
    rollback_error(subject, error, cleanup_errors)
}

fn rollback_error(subject: &str, error: &str, cleanup_errors: Vec<String>) -> String {
    if cleanup_errors.is_empty() {
        format!("{}，已回滚文件变更: {}", subject, error)
    } else {
        format!(
            "{}，文件已回滚但元数据清理不完整: {}；{}",
            subject,
            error,
            cleanup_errors.join("；")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn manifest_skill(source: &str, reference: &str) -> SkillMateManifestSkill {
        SkillMateManifestSkill {
            assistant: "Codex".to_string(),
            source: source.to_string(),
            source_kind: "git".to_string(),
            target_name: Some("writer".to_string()),
            scope: Some("global".to_string()),
            install_mode: Some("copy".to_string()),
            reference: Some(reference.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn manifest_identity_tracks_source_and_reference() {
        let main = manifest_skill("example/skills", "main");
        let release = manifest_skill("example/skills", "v2");
        let fork = manifest_skill("other/skills", "main");

        assert_ne!(manifest_skill_key(&main), manifest_skill_key(&release));
        assert_ne!(manifest_skill_key(&main), manifest_skill_key(&fork));
    }

    #[test]
    fn profile_transition_restores_content_when_state_persist_fails() {
        let restored = Cell::new(false);

        let error = apply_then_persist(
            || Ok("已应用".to_string()),
            || Err("磁盘已满".to_string()),
            || {
                restored.set(true);
                Ok(())
            },
        )
        .unwrap_err();

        assert!(restored.get());
        assert!(error.contains("已恢复原组合"));
    }

    #[test]
    fn project_manifest_keeps_only_requested_project() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-project-manifest-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let other = root.join("other");
        let manifest = SkillMateManifest {
            version: 2,
            reconcile: true,
            skills: vec![
                SkillMateManifestSkill {
                    assistant: "Codex".to_string(),
                    source: "owner/project".to_string(),
                    source_kind: "git".to_string(),
                    target_name: Some("project".to_string()),
                    scope: Some("project".to_string()),
                    project_path: Some(root.to_string_lossy().to_string()),
                    ..Default::default()
                },
                SkillMateManifestSkill {
                    assistant: "Gemini CLI".to_string(),
                    source: "owner/other".to_string(),
                    source_kind: "git".to_string(),
                    target_name: Some("other".to_string()),
                    scope: Some("project".to_string()),
                    project_path: Some(other.to_string_lossy().to_string()),
                    ..Default::default()
                },
                SkillMateManifestSkill {
                    assistant: "Claude Code".to_string(),
                    source: "owner/global".to_string(),
                    source_kind: "git".to_string(),
                    target_name: Some("global".to_string()),
                    scope: Some("global".to_string()),
                    ..Default::default()
                },
            ],
        };

        let project = project_manifest_from(manifest, &root).unwrap();

        assert_eq!(project.skills.len(), 1);
        assert_eq!(project.skills[0].target_name.as_deref(), Some("project"));
        assert_eq!(
            project.skills[0].project_path.as_deref(),
            Some(root.to_string_lossy().as_ref())
        );
        assert_eq!(
            manifest_reconcile_scope(&project).unwrap(),
            ManifestReconcileScope::Project(comparable_project_path(&root).unwrap())
        );
    }

    #[test]
    fn mixed_manifest_keeps_global_reconcile_scope() {
        let manifest = SkillMateManifest {
            version: 2,
            reconcile: true,
            skills: vec![
                SkillMateManifestSkill {
                    assistant: "Codex".to_string(),
                    source: "owner/project".to_string(),
                    source_kind: "git".to_string(),
                    scope: Some("project".to_string()),
                    project_path: Some("/tmp/project".to_string()),
                    ..Default::default()
                },
                SkillMateManifestSkill {
                    assistant: "Claude Code".to_string(),
                    source: "owner/global".to_string(),
                    source_kind: "git".to_string(),
                    scope: Some("global".to_string()),
                    ..Default::default()
                },
            ],
        };

        assert_eq!(
            manifest_reconcile_scope(&manifest).unwrap(),
            ManifestReconcileScope::All
        );
    }
}
