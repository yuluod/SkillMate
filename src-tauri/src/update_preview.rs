use crate::install_policy::{evaluate_install_policy, load_install_policy, InstallPolicyInput};
use crate::managed_installation::{
    find_managed_installation, is_explicitly_managed, verify_managed_content_unchanged,
};
use crate::managed_state::content_fingerprint;
use crate::operation_plan::{operation_plan_token, StableHash};
use crate::skill_install::{installable_content_fingerprint, with_git_snapshot};
use crate::skill_library::{deployment_targets_for_library, resolve_library_path};
use crate::skill_origin::load_origin_meta;
use crate::skill_structure::inspect_skill_for_inventory;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateImpact {
    path: String,
    assistant: String,
    scope: String,
    project_path: Option<String>,
    connected: bool,
    link_target: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    path: String,
    kind: String,
    before: Option<String>,
    after: Option<String>,
    text_omitted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePreview {
    pub library_path: String,
    pub installed_ref: String,
    pub latest_ref: String,
    pub source_digest: String,
    pub can_apply: bool,
    pub blocked_reason: Option<String>,
    pub warnings: Vec<String>,
    pub impacts: Vec<UpdateImpact>,
    pub files: Vec<FileChange>,
    pub omitted_files: usize,
    pub plan_token: String,
}

pub fn apply_update(
    db: &Connection,
    requested: &Path,
    token: Option<&str>,
) -> Result<String, String> {
    if token.is_none_or(|value| value.trim().is_empty()) {
        return Err("操作计划缺失，请重新预览".into());
    }
    with_update_preview(db, requested, |preview, source| {
        crate::operation_plan::verify_operation_plan(&preview.plan_token, token)?;
        if !preview.can_apply {
            return Err("当前更新计划不可执行，请重新检查来源和策略".into());
        }
        let path = Path::new(&preview.library_path);
        let expected = crate::skill_install::GitSnapshotProbe {
            latest_ref: preview.latest_ref,
            source_digest: preview.source_digest,
        };
        crate::skill_origin::update_skill_from_prepared_snapshot(db, path, source, &expected)
    })
}

pub fn preview_update(db: &Connection, requested: &Path) -> Result<UpdatePreview, String> {
    with_update_preview(db, requested, |preview, _| Ok(preview))
}

fn with_update_preview<T>(
    db: &Connection,
    requested: &Path,
    action: impl FnOnce(UpdatePreview, &Path) -> Result<T, String>,
) -> Result<T, String> {
    let path = resolve_library_path(db, requested)?;
    if !is_explicitly_managed(db, &path)? {
        return Err("只允许预览 SkillMate 管理的更新".into());
    }
    verify_managed_content_unchanged(db, &path)?;
    let meta = load_origin_meta(db, &path.to_string_lossy())?.ok_or("缺少更新来源信息")?;
    if meta.origin_kind != "git" {
        return Err("当前来源不支持更新预览".into());
    }
    let baseline = content_fingerprint(&path)?;
    with_git_snapshot(
        &meta.origin_locator,
        &meta.resolved_locator,
        &meta.tracking_ref,
        |source, latest| {
            if load_origin_meta(db, &path.to_string_lossy())?.as_ref() != Some(&meta) {
                return Err("来源状态在下载期间发生变化，请重新预览".into());
            }
            // 下载可能耗时较长，使用完成后的策略和真实连接状态生成计划。
            let policy = load_install_policy(db)?;
            let impacts = collect_update_impacts(db, &path)?;
            let structure = inspect_skill_for_inventory(source).structure;
            let decision = evaluate_install_policy(
                &policy,
                InstallPolicyInput {
                    source_kind: "git",
                    source: if meta.origin_locator.trim().is_empty() {
                        &meta.resolved_locator
                    } else {
                        &meta.origin_locator
                    },
                    structure_status: &structure.structure_status,
                    warnings: &structure.structure_warnings,
                },
            );
            let (files, omitted_files) = compare_files(&path, source)?;
            let source_digest = installable_content_fingerprint(source)?;
            let blocked_reason = if !decision.allowed {
                Some("policy")
            } else if structure.structure_status != "complete" {
                Some("structure")
            } else if impacts.iter().any(|impact| !impact.connected) {
                Some("deployments")
            } else if files.is_empty() && omitted_files == 0 && meta.installed_ref == latest {
                Some("unchanged")
            } else {
                None
            };
            // 计划绑定完整内容、策略和启用关系，不能只依赖有界的展示差异。
            let plan_token = operation_plan_token(
                "update",
                &(
                    &path,
                    &meta.origin_locator,
                    &meta.resolved_locator,
                    &meta.tracking_ref,
                    &meta.installed_ref,
                    latest,
                    &baseline,
                    &source_digest,
                    &policy,
                    &impacts,
                ),
            )?;
            if content_fingerprint(&path)? != baseline {
                return Err("预览期间本地内容发生变化，请重新预览".into());
            }
            if load_origin_meta(db, &path.to_string_lossy())?.as_ref() != Some(&meta)
                || load_install_policy(db)? != policy
                || collect_update_impacts(db, &path)? != impacts
            {
                return Err("预览期间来源、策略或启用关系发生变化，请重新预览".into());
            }
            action(
                UpdatePreview {
                    library_path: path.to_string_lossy().into_owned(),
                    installed_ref: meta.installed_ref.clone(),
                    latest_ref: latest.into(),
                    source_digest,
                    can_apply: blocked_reason.is_none(),
                    blocked_reason: blocked_reason.map(str::to_owned),
                    warnings: structure
                        .structure_warnings
                        .into_iter()
                        .chain(decision.findings.into_iter().map(|finding| finding.code))
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    impacts,
                    files,
                    omitted_files,
                    plan_token,
                },
                source,
            )
        },
    )
}

fn collect_update_impacts(db: &Connection, path: &Path) -> Result<Vec<UpdateImpact>, String> {
    let mut impacts = Vec::new();
    for target in deployment_targets_for_library(db, path)? {
        let target_path = Path::new(&target);
        let registered = find_managed_installation(db, target_path)?;
        let link_target = fs::read_link(target_path)
            .ok()
            .map(|value| value.to_string_lossy().into_owned());
        let connected = link_target.is_some()
            && matches!((target_path.canonicalize(), path.canonicalize()), (Ok(left), Ok(right)) if left == right);
        impacts.push(UpdateImpact {
            assistant: registered
                .as_ref()
                .map(|value| value.skill.assistant.clone())
                .unwrap_or_default(),
            scope: registered
                .as_ref()
                .and_then(|value| value.skill.scope.clone())
                .unwrap_or_else(|| "global".into()),
            project_path: registered.and_then(|value| value.skill.project_path),
            path: target,
            connected,
            link_target,
        });
    }
    impacts.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(impacts)
}

struct FileSnapshot {
    digest: String,
    text: Option<String>,
}

fn collect_files(root: &Path, skip_links: bool) -> Result<BTreeMap<String, FileSnapshot>, String> {
    fn walk(
        root: &Path,
        dir: &Path,
        depth: usize,
        skip_links: bool,
        files: &mut BTreeMap<String, FileSnapshot>,
        bytes: &mut u64,
        text_budget: &mut usize,
    ) -> Result<(), String> {
        if depth > 32 {
            return Err("差异目录层级超出限制".into());
        }
        let mut entries = fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if matches!(
                entry.file_name().to_string_lossy().as_ref(),
                ".git" | ".hg" | ".svn"
            ) {
                continue;
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if metadata.file_type().is_symlink() && skip_links {
                continue;
            }
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                walk(
                    root,
                    &path,
                    depth + 1,
                    skip_links,
                    files,
                    bytes,
                    text_budget,
                )?;
                continue;
            }
            if files.len() >= 10_000 {
                return Err("差异文件数超出限制".into());
            }
            let snapshot = if metadata.file_type().is_symlink() {
                FileSnapshot {
                    digest: format!(
                        "link:{}",
                        fs::read_link(&path).map_err(|e| e.to_string())?.display()
                    ),
                    text: None,
                }
            } else if metadata.is_file() {
                let mut data = Vec::new();
                fs::File::open(&path)
                    .map_err(|e| e.to_string())?
                    .take(256 * 1024 * 1024 - *bytes + 1)
                    .read_to_end(&mut data)
                    .map_err(|e| e.to_string())?;
                *bytes += data.len() as u64;
                if *bytes > 256 * 1024 * 1024 {
                    return Err("差异文件大小超出限制".into());
                }
                let mut hash = StableHash::new();
                hash.update(&data);
                let text = if data.len() <= 8192 && data.len() <= *text_budget && !data.contains(&0)
                {
                    String::from_utf8(data).ok()
                } else {
                    None
                };
                *text_budget -= text.as_ref().map_or(0, String::len);
                FileSnapshot {
                    digest: hash.finish(),
                    text,
                }
            } else {
                return Err("不支持的差异文件类型".into());
            };
            files.insert(
                path.strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/"),
                snapshot,
            );
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(
        root,
        root,
        0,
        skip_links,
        &mut files,
        &mut 0,
        &mut (64 * 1024),
    )?;
    Ok(files)
}

fn compare_files(before: &Path, after: &Path) -> Result<(Vec<FileChange>, usize), String> {
    let before = collect_files(before, false)?;
    let after = collect_files(after, true)?;
    let paths = before.keys().chain(after.keys()).collect::<BTreeSet<_>>();
    let mut files = Vec::new();
    let mut omitted = 0;
    for path in paths {
        let old = before.get(path);
        let new = after.get(path);
        if old.map(|value| &value.digest) == new.map(|value| &value.digest) {
            continue;
        }
        if files.len() == 200 {
            omitted += 1;
            continue;
        }
        files.push(FileChange {
            path: path.clone(),
            kind: if old.is_none() {
                "added"
            } else if new.is_none() {
                "removed"
            } else {
                "modified"
            }
            .into(),
            before: old.and_then(|value| value.text.clone()),
            after: new.and_then(|value| value.text.clone()),
            text_omitted: old.is_some_and(|value| value.text.is_none())
                || new.is_some_and(|value| value.text.is_none()),
        });
    }
    Ok((files, omitted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_preview_diff_bounds_text_and_lists_additions_removals_and_changes() {
        let root =
            std::env::temp_dir().join(format!("skillmate-diff-{}", crate::app_core::generate_id()));
        let before = root.join("before");
        let after = root.join("after");
        fs::create_dir_all(&before).unwrap();
        fs::create_dir_all(after.join(".git")).unwrap();
        fs::write(before.join("changed"), "old").unwrap();
        fs::write(after.join("changed"), "new").unwrap();
        fs::write(before.join("removed"), "removed").unwrap();
        fs::write(after.join("added"), "added").unwrap();
        fs::write(after.join("binary"), [0, 1, 2]).unwrap();
        fs::write(after.join("large"), vec![b'x'; 8193]).unwrap();
        fs::write(after.join(".git/config"), "private git metadata").unwrap();
        let (files, omitted) = compare_files(&before, &after).unwrap();
        assert_eq!(omitted, 0);
        assert_eq!(files.len(), 5);
        assert_eq!(files[0].kind, "added");
        assert!(files[1].text_omitted);
        assert_eq!(files[2].before.as_deref(), Some("old"));
        assert_eq!(files[2].after.as_deref(), Some("new"));
        assert!(files[3].text_omitted);
        assert_eq!(files[4].kind, "removed");
        for index in 0..205 {
            fs::write(after.join(format!("file-{index:03}")), "extra").unwrap();
        }
        let (files, omitted) = compare_files(&before, &after).unwrap();
        assert_eq!(files.len(), 200);
        assert_eq!(omitted, 10);
        fs::remove_dir_all(root).unwrap();
    }
}
