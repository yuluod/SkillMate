use crate::app_core::{assistant_definitions, expand_path};
use crate::database::{database_path_key, PathColumn};
use crate::install_policy::load_install_policy;
use crate::managed_installation::{
    cleanup_skill_metadata, find_managed_installation, verify_managed_content_unchanged,
};
use crate::managed_state::content_fingerprint;
use crate::operation_plan::{operation_plan_token, verify_operation_plan};
use crate::skill_install::{
    install_selected_local_package_at_digest, preview_selected_local_install_source,
    seal_install_preview, InstallPreview, InstallResult,
};
use crate::skill_library::{find_deployment, resolve_library_path, reuse_library_preview};
use crate::skill_origin::{infer_origin_meta, load_origin_meta, save_origin_meta};
use crate::skill_reconcile::ReconcileTransaction;
use crate::{apply_policy_to_preview, install_result, rollback_install_result};
use rusqlite::{Connection, OptionalExtension};
use std::fs;
use std::path::PathBuf;

struct MaterializationContext {
    target: PathBuf,
    library: PathBuf,
    project: PathBuf,
}

fn materialization_context(
    db: &Connection,
    path: &str,
    assistant_name: &str,
    project_path: &str,
) -> Result<MaterializationContext, String> {
    if project_path.trim().is_empty() {
        return Err("转换项目副本必须指定项目目录".to_string());
    }
    let project = expand_path(project_path.trim())
        .canonicalize()
        .map_err(|error| format!("项目目录无法访问: {error}"))?;
    let assistant = assistant_definitions()
        .iter()
        .find(|assistant| assistant.name == assistant_name)
        .ok_or_else(|| format!("不支持的平台: {assistant_name}"))?;
    let root = assistant
        .project_install_root(&project)
        .ok_or_else(|| format!("{} 不支持项目级 Skills", assistant.name))?
        .canonicalize()
        .map_err(|error| format!("项目 Skill 目录无法访问: {error}"))?;
    let input = expand_path(path.trim());
    let parent = input
        .parent()
        .ok_or_else(|| "Skill 缺少父目录".to_string())?
        .canonicalize()
        .map_err(|error| format!("Skill 父目录无法访问: {error}"))?;
    if parent != root || !root.starts_with(&project) {
        return Err("只能转换所选项目平台目录中的 Skill 链接".to_string());
    }
    let target = parent.join(
        input
            .file_name()
            .ok_or_else(|| "Skill 目录名无效".to_string())?,
    );
    if !fs::symlink_metadata(&target)
        .map_err(|error| format!("Skill 链接无法访问: {error}"))?
        .file_type()
        .is_symlink()
    {
        return Err("当前 Skill 不是软链接，无需转换".to_string());
    }
    let installation = find_managed_installation(db, &target)?
        .ok_or_else(|| "只能转换 SkillMate 受管的项目链接".to_string())?;
    let recorded_project = installation
        .skill
        .project_path
        .as_deref()
        .map(expand_path)
        .map(|path| path.canonicalize())
        .transpose()
        .map_err(|error| format!("登记的项目目录无法访问: {error}"))?;
    if installation.skill.scope.as_deref() != Some("project")
        || installation.skill.assistant != assistant_name
        || recorded_project.as_deref() != Some(project.as_path())
        || find_deployment(db, &target)?.is_none()
    {
        return Err("Skill 的受管启用记录与所选项目或平台不一致".to_string());
    }
    verify_managed_content_unchanged(db, &target)?;
    let library = resolve_library_path(db, &target)?;
    let metadata =
        fs::symlink_metadata(&library).map_err(|error| format!("统一库主副本无法访问: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("统一库主副本必须是实体目录".to_string());
    }
    let library = library.canonicalize().map_err(|error| error.to_string())?;
    if target.canonicalize().map_err(|error| error.to_string())? != library
        || target.file_name() != library.file_name()
    {
        return Err("项目链接已偏离登记的统一库主副本".to_string());
    }
    Ok(MaterializationContext {
        target,
        library,
        project,
    })
}

pub fn preview_materialize_skill(
    db: &Connection,
    path: &str,
    assistant_name: &str,
    project_path: &str,
) -> Result<InstallPreview, String> {
    let context = materialization_context(db, path, assistant_name, project_path)?;
    let source = context.library.to_string_lossy();
    let mut preview = reuse_library_preview(preview_selected_local_install_source(
        &source,
        context.target.parent().ok_or("Skill 缺少父目录")?,
        None,
    ));
    if preview.target_actions.len() != 1
        || preview.target_actions[0].target != context.target.to_string_lossy()
    {
        return Err("项目副本计划必须只替换当前 Skill 链接".to_string());
    }
    let unsafe_copy = preview
        .structure_warnings
        .iter()
        .chain(preview.package_detection.warnings.iter())
        .chain(
            preview
                .package_detection
                .detected_skills
                .iter()
                .flat_map(|skill| &skill.warnings),
        )
        .any(|warning| {
            matches!(
                warning.as_str(),
                "contains_symlinks" | "safety_scan_incomplete"
            )
        });
    if unsafe_copy {
        preview.can_install = false;
        preview.can_apply = false;
        preview.message = "主副本包含内部链接或安全扫描不完整，无法转换为项目副本".to_string();
    } else if preview.can_apply {
        preview.target_actions[0].action = "replace".to_string();
        preview.target_actions[0].source = source.to_string();
        preview.target_actions[0].reason =
            "将受管链接替换为项目独立副本，并解除当前位置的受管登记".to_string();
        preview.message =
            "将复制统一库当前内容到项目，由项目自行维护；统一库主副本保持不变".to_string();
    }
    let policy = load_install_policy(db);
    apply_policy_to_preview(
        &mut preview,
        &source,
        "local",
        policy.as_ref().map_err(Clone::clone),
    );
    preview = seal_install_preview(
        preview,
        path,
        assistant_name,
        "materialize",
        Some(&context.project.to_string_lossy()),
    );
    preview.plan_token = operation_plan_token(
        "materialize",
        &(&preview, content_fingerprint(&context.target)?),
    )?;
    Ok(preview)
}

pub fn materialize_skill(
    db: &Connection,
    path: &str,
    assistant_name: &str,
    project_path: &str,
    plan_token: Option<&str>,
) -> InstallResult {
    let prepared = (|| {
        let context = materialization_context(db, path, assistant_name, project_path)?;
        let preview = preview_materialize_skill(db, path, assistant_name, project_path)?;
        verify_operation_plan(&preview.plan_token, plan_token)?;
        if !preview.can_apply {
            return Err(preview.message);
        }
        let mut origin = match load_origin_meta(db, &context.library.to_string_lossy())? {
            Some(origin) => origin,
            None => {
                let installation = find_managed_installation(db, &context.library)?
                    .ok_or("统一库主副本缺少来源登记，请先重新检查来源")?;
                if installation.skill.source_kind != "local" {
                    return Err("统一库主副本缺少来源信息，请先重新检查来源".to_string());
                }
                let mut origin = infer_origin_meta(&context.library, None);
                origin.origin_kind = "local".to_string();
                origin.origin_locator = installation.skill.source;
                origin.resolved_locator = origin.origin_locator.clone();
                origin
            }
        };
        origin.skill_path = context.target.to_string_lossy().to_string();
        origin.managed_by_app = false;
        let tags_key = database_path_key(db, PathColumn::SkillTags, &context.library)?;
        let tags = db
            .query_row(
                "SELECT tags, tags_json FROM skill_tags WHERE skill_path = ?",
                [tags_key],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        Ok((context, preview, origin, tags))
    })();
    let (context, preview, origin, tags) = match prepared {
        Ok(value) => value,
        Err(error) => return install_result(false, error, "", None),
    };
    let mut transaction = match ReconcileTransaction::prepare_managed(
        db,
        std::slice::from_ref(&context.target),
        std::slice::from_ref(&context.target),
    ) {
        Ok(transaction) => transaction,
        Err(error) => return install_result(false, "无法建立项目副本事务", error, None),
    };
    let result = (|| {
        let structure = install_selected_local_package_at_digest(
            &context.library,
            context.target.parent().ok_or("Skill 缺少父目录")?,
            context
                .target
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("Skill 目录名无效")?,
            assistant_name,
            Some(&preview.source_digest),
            None,
        )?;
        cleanup_skill_metadata(db, &context.target)?;
        save_origin_meta(db, &origin)?;
        if let Some((legacy, json)) = tags {
            let key = database_path_key(db, PathColumn::SkillTags, &context.target)?;
            db.execute(
                "INSERT INTO skill_tags (skill_path, tags, tags_json) VALUES (?, ?, ?)",
                rusqlite::params![key, legacy, json],
            )
            .map_err(|error| error.to_string())?;
        }
        Ok(structure)
    })();
    let structure = match result {
        Ok(structure) => structure,
        Err(error) => return rollback_install_result(&mut transaction, "转换项目副本失败", error),
    };
    match transaction.commit() {
        Ok(warning) => install_result(
            true,
            "已转换为项目独立副本，后续由项目自行维护",
            warning.unwrap_or_default(),
            Some(structure),
        ),
        Err(error) => install_result(false, "提交项目副本事务失败", error, None),
    }
}
