use crate::app_core::{assistant_definitions, expand_path, find_git_repo_root};
use crate::install_policy::{load_install_policy, InstallPolicyConfig};
use crate::operation_plan::verify_operation_plan;
use crate::skill_install::{
    install_selected_local_package_at_digest, preview_selected_local_install_source,
    seal_install_preview, InstallPreview, InstallResult, PreviewAction,
};
use crate::skill_install_source::parse_git_install_spec;
use crate::skill_library::library_root;
use crate::skill_origin::{infer_origin_meta, load_origin_meta, save_origin_meta, SkillOriginMeta};
use crate::skill_reconcile::ReconcileTransaction;
use crate::{
    apply_policy_to_preview, finalize_library_install_registration, install_result,
    rollback_install_result,
};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

pub fn preview_adopt_skill(
    db: &Connection,
    path: &str,
    assistant_name: &str,
    project_path: Option<&str>,
) -> InstallPreview {
    let source_path = expand_path(path.trim());
    let policy = load_install_policy(db);
    let mut preview = match adoption_context(&source_path, assistant_name, project_path) {
        Ok(_) => match library_root() {
            Ok(library) => preview_selected_local_install_source(path.trim(), &library, None),
            Err(error) => adoption_error(error, &source_path),
        },
        Err(error) => adoption_error(error, &source_path),
    };
    if preview.can_apply {
        let library_path = preview
            .target_actions
            .iter()
            .find(|action| matches!(action.action.as_str(), "copy" | "keep"))
            .map(|action| action.target.clone())
            .unwrap_or_default();
        preview.target_actions.push(PreviewAction {
            action: "replace".to_string(),
            source: library_path,
            target: source_path.to_string_lossy().to_string(),
            reason: "将现有实体目录替换为 SkillMate 受管链接".to_string(),
        });
        preview.message = "将现有 Skill 加入统一库，并把当前位置切换为受管链接".to_string();
    }
    apply_adoption_policy(&mut preview, path, policy.as_ref().map_err(Clone::clone));
    seal_install_preview(preview, path, assistant_name, "adopt", project_path)
}

pub fn adopt_skill(
    db: &Connection,
    path: String,
    assistant_name: String,
    project_path: Option<String>,
    plan_token: Option<String>,
) -> InstallResult {
    let source_path = expand_path(path.trim());
    let context = match adoption_context(&source_path, &assistant_name, project_path.as_deref()) {
        Ok(context) => context,
        Err(error) => return install_result(false, error, "", None),
    };
    let preview = preview_adopt_skill(db, &path, &assistant_name, project_path.as_deref());
    if let Err(error) = verify_operation_plan(&preview.plan_token, plan_token.as_deref()) {
        return install_result(false, error, "", None);
    }
    if !preview.can_apply {
        return install_result(false, preview.message, "", None);
    }
    let Some(library_path) = preview
        .target_actions
        .iter()
        .find(|action| action.action == "copy")
        .map(|action| PathBuf::from(&action.target))
    else {
        return install_result(false, "接管计划缺少统一库写入动作", "", None);
    };
    let library = match library_root() {
        Ok(path) => path,
        Err(error) => return install_result(false, error, "", None),
    };
    let source_origin = match load_origin_meta(db, &source_path.to_string_lossy()) {
        Ok(Some(origin)) => origin,
        Ok(None) => infer_origin_meta(&source_path, None),
        Err(error) => return install_result(false, "读取来源信息失败", error, None),
    };
    if let Err(error) = fs::create_dir_all(&library) {
        return install_result(false, format!("无法创建 SkillMate 库: {error}"), "", None);
    }

    let mut transaction = match ReconcileTransaction::prepare_managed(
        db,
        std::slice::from_ref(&source_path),
        &[library_path.clone(), source_path.clone()],
    ) {
        Ok(transaction) => transaction,
        Err(error) => return install_result(false, "无法建立接管事务", error, None),
    };
    let staged_source = match transaction.staged_removal_path(&source_path) {
        Some(path) => path.to_path_buf(),
        None => {
            return rollback_install_result(
                &mut transaction,
                "接管失败",
                "未能暂存现有 Skill".to_string(),
            );
        }
    };
    let fallback_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let structure = match install_selected_local_package_at_digest(
        &staged_source,
        &library,
        fallback_name,
        "SkillMate",
        Some(&preview.source_digest),
        None,
    ) {
        Ok(structure) => structure,
        Err(error) => return rollback_install_result(&mut transaction, "接管失败", error),
    };
    let library_origin = origin_for_adoption(source_origin, &source_path, &library_path);
    if let Err(error) = save_origin_meta(db, &library_origin) {
        return rollback_install_result(&mut transaction, "迁移来源信息失败", error);
    }
    let registration_source = if library_origin.origin_locator.trim().is_empty() {
        path.as_str()
    } else {
        library_origin.origin_locator.as_str()
    };
    let registration = finalize_library_install_registration(
        db,
        registration_source,
        &library_origin.origin_kind,
        &assistant_name,
        context.scope,
        context.project_path.as_deref(),
        &library,
        Some(&context.deployment_root),
        std::slice::from_ref(&library_path),
        std::slice::from_ref(&source_path),
        &library_origin.installed_ref,
        false,
    );
    if let Err(error) = registration {
        return rollback_install_result(&mut transaction, "记录接管状态失败", error);
    }
    match transaction.commit() {
        Ok(None) => install_result(
            true,
            "已加入 SkillMate 库并接管当前位置",
            "",
            Some(structure),
        ),
        Ok(Some(warning)) => install_result(
            true,
            "已加入 SkillMate 库并接管当前位置",
            warning,
            Some(structure),
        ),
        Err(error) => install_result(false, "提交接管事务失败", error, None),
    }
}

fn retarget_origin(mut origin: SkillOriginMeta, library_path: &Path) -> SkillOriginMeta {
    origin.skill_path = library_path.to_string_lossy().to_string();
    origin.managed_by_app = true;
    origin
}

fn origin_for_adoption(
    mut origin: SkillOriginMeta,
    source_path: &Path,
    library_path: &Path,
) -> SkillOriginMeta {
    if origin.origin_kind == "git" {
        let locator = if origin.origin_locator.trim().is_empty() {
            origin.resolved_locator.as_str()
        } else {
            origin.origin_locator.as_str()
        };
        if let Some(repo_root) = find_git_repo_root(source_path) {
            if let Ok(relative) = source_path.strip_prefix(repo_root) {
                if !relative.as_os_str().is_empty() {
                    if let Ok(spec) = parse_git_install_spec(locator) {
                        if spec.subdir.is_none() {
                            let subdir = relative.to_string_lossy().replace('\\', "/");
                            origin.origin_locator = match spec.reference {
                                Some(reference) => {
                                    format!("{}#{}:{}", spec.repo_url, reference, subdir)
                                }
                                None => format!("{}#:{}", spec.repo_url, subdir),
                            };
                        }
                    }
                }
            }
        }
    }
    retarget_origin(origin, library_path)
}

struct AdoptionContext {
    scope: &'static str,
    project_path: Option<String>,
    deployment_root: PathBuf,
}

fn adoption_context(
    source_path: &Path,
    assistant_name: &str,
    project_path: Option<&str>,
) -> Result<AdoptionContext, String> {
    let metadata = fs::symlink_metadata(source_path)
        .map_err(|error| format!("无法读取待接管 Skill: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("当前只接管实体 Skill 目录，链接和普通文件保持外部管理".to_string());
    }
    let assistant = assistant_definitions()
        .iter()
        .find(|assistant| assistant.name == assistant_name)
        .ok_or_else(|| format!("不支持的平台: {assistant_name}"))?;
    let deployment_root = source_path
        .parent()
        .ok_or_else(|| "待接管 Skill 缺少父目录".to_string())?
        .to_path_buf();
    if let Some(project_value) = project_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let project = expand_path(project_value);
        let project = project
            .canonicalize()
            .map_err(|error| format!("项目路径不存在或无法访问: {error}"))?;
        let root = assistant
            .project_install_root(&project)
            .ok_or_else(|| format!("{} 不支持项目级 Skills", assistant.name))?;
        if !path_is_below(source_path, &root) {
            return Err("待接管 Skill 不在所选项目的平台目录中".to_string());
        }
        return Ok(AdoptionContext {
            scope: "project",
            project_path: Some(project.to_string_lossy().to_string()),
            deployment_root,
        });
    }
    if !assistant
        .global_discovery_roots()
        .any(|root| path_is_below(source_path, &root))
    {
        return Err("待接管 Skill 不在所选平台的全局发现目录中".to_string());
    }
    Ok(AdoptionContext {
        scope: "global",
        project_path: None,
        deployment_root,
    })
}

fn path_is_below(path: &Path, root: &Path) -> bool {
    let normalized_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let normalized_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    normalized_path != normalized_root && normalized_path.starts_with(normalized_root)
}

fn apply_adoption_policy(
    preview: &mut InstallPreview,
    package: &str,
    policy: Result<&InstallPolicyConfig, String>,
) {
    apply_policy_to_preview(preview, package, "local", policy);
}

fn adoption_error(message: impl Into<String>, source: &Path) -> InstallPreview {
    let source_value = source.to_string_lossy();
    let mut preview = preview_selected_local_install_source(&source_value, Path::new("."), None);
    preview.can_install = false;
    preview.can_apply = false;
    preview.message = message.into();
    preview.target_actions.clear();
    preview.conflicts.clear();
    preview
}

pub fn preview_adopt_skill_fallback(path: &str, message: impl Into<String>) -> InstallPreview {
    adoption_error(message, &expand_path(path.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "skillmate-adoption-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn path_is_below_rejects_the_root_itself() {
        let root = Path::new("/tmp/skills");
        assert!(!path_is_below(root, root));
        assert!(path_is_below(&root.join("writer"), root));
    }

    #[test]
    fn project_adoption_requires_the_matching_agent_directory() {
        let root = temp_dir("project-scope");
        let project = root.join("project");
        let skill = project.join(".agents/skills/writer");
        fs::create_dir_all(&skill).unwrap();

        let context =
            adoption_context(&skill, "Codex", Some(project.to_string_lossy().as_ref())).unwrap();
        assert_eq!(context.scope, "project");
        assert_eq!(
            context.project_path.as_deref(),
            Some(project.canonicalize().unwrap().to_string_lossy().as_ref())
        );

        let error = adoption_context(
            &skill,
            "Claude Code",
            Some(project.to_string_lossy().as_ref()),
        )
        .err()
        .unwrap();
        assert!(error.contains("不在所选项目"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn retargeting_preserves_git_origin_and_update_state() {
        let origin = SkillOriginMeta {
            skill_path: "/source/writer".to_string(),
            origin_kind: "git".to_string(),
            origin_locator: "https://github.com/example/skills.git#main:writer".to_string(),
            resolved_locator: "https://github.com/example/skills.git".to_string(),
            tracking_ref: "main".to_string(),
            installed_ref: "abc123".to_string(),
            latest_ref: "def456".to_string(),
            sync_state: "behind".to_string(),
            sync_message: "落后 1 个提交".to_string(),
            lag_count: 1,
            last_probe_at: Some(1),
            last_sync_at: None,
            managed_by_app: false,
        };

        let migrated = retarget_origin(origin, Path::new("/library/writer"));

        assert_eq!(migrated.skill_path, "/library/writer");
        assert_eq!(migrated.origin_kind, "git");
        assert_eq!(
            migrated.origin_locator,
            "https://github.com/example/skills.git#main:writer"
        );
        assert_eq!(migrated.installed_ref, "abc123");
        assert!(migrated.managed_by_app);
    }

    #[test]
    fn adoption_preserves_the_git_subdirectory_needed_for_updates() {
        let root = temp_dir("git-subdir");
        fs::create_dir_all(root.join(".git")).unwrap();
        let source = root.join("plugins/writer");
        fs::create_dir_all(&source).unwrap();
        let origin = SkillOriginMeta {
            skill_path: source.to_string_lossy().to_string(),
            origin_kind: "git".to_string(),
            origin_locator: "https://github.com/example/skills.git".to_string(),
            resolved_locator: "https://github.com/example/skills.git".to_string(),
            tracking_ref: "main".to_string(),
            installed_ref: "abc123".to_string(),
            latest_ref: String::new(),
            sync_state: "current".to_string(),
            sync_message: String::new(),
            lag_count: 0,
            last_probe_at: None,
            last_sync_at: None,
            managed_by_app: false,
        };

        let migrated = origin_for_adoption(origin, &source, Path::new("/library/writer"));

        assert_eq!(
            migrated.origin_locator,
            "https://github.com/example/skills.git#:plugins/writer"
        );
        let _ = fs::remove_dir_all(root);
    }
}
