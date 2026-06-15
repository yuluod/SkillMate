use crate::app_core::{
    command_exists, detect_npm_package, detect_pip_package, find_git_repo_root, get_git_remote_url,
    git_count, git_output, has_git_upstream, now_ms, run_command_with_timeout,
};
use crate::skill_install::{
    has_git_subdir_spec, probe_git_subdir_latest_ref, sync_git_subdir_skill, GitInstallOutcome,
    GitInstallSpec,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SkillOriginMeta {
    pub skill_path: String,
    pub origin_kind: String,
    pub origin_locator: String,
    pub resolved_locator: String,
    pub tracking_ref: String,
    pub installed_ref: String,
    pub latest_ref: String,
    pub sync_state: String,
    pub sync_message: String,
    pub lag_count: u32,
    pub last_probe_at: Option<i64>,
    pub last_sync_at: Option<i64>,
    pub managed_by_app: bool,
}

#[derive(Debug, Clone)]
pub struct SkillSyncInfo {
    pub meta: SkillOriginMeta,
    pub can_sync: bool,
    pub has_update: bool,
}

pub fn load_origin_meta(db: &Connection, skill_path: &str) -> Option<SkillOriginMeta> {
    db.query_row(
        "SELECT skill_path, origin_kind, origin_locator, resolved_locator, tracking_ref, installed_ref, latest_ref, sync_state, sync_message, lag_count, last_probe_at, last_sync_at, managed_by_app FROM skill_origin_meta WHERE skill_path = ?",
        [skill_path],
        |row| {
            Ok(SkillOriginMeta {
                skill_path: row.get(0)?,
                origin_kind: row.get(1)?,
                origin_locator: row.get(2)?,
                resolved_locator: row.get(3)?,
                tracking_ref: row.get(4)?,
                installed_ref: row.get(5)?,
                latest_ref: row.get(6)?,
                sync_state: row.get(7)?,
                sync_message: row.get(8)?,
                lag_count: row.get::<_, i64>(9)?.max(0) as u32,
                last_probe_at: row.get(10)?,
                last_sync_at: row.get(11)?,
                managed_by_app: row.get::<_, i32>(12)? != 0,
            })
        },
    ).optional().ok().flatten()
}

pub fn save_origin_meta(db: &Connection, meta: &SkillOriginMeta) -> Result<(), String> {
    db.execute(
        "INSERT INTO skill_origin_meta (
            skill_path, origin_kind, origin_locator, resolved_locator, tracking_ref, installed_ref,
            latest_ref, sync_state, sync_message, lag_count, last_probe_at, last_sync_at, managed_by_app
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(skill_path) DO UPDATE SET
            origin_kind = excluded.origin_kind,
            origin_locator = excluded.origin_locator,
            resolved_locator = excluded.resolved_locator,
            tracking_ref = excluded.tracking_ref,
            installed_ref = excluded.installed_ref,
            latest_ref = excluded.latest_ref,
            sync_state = excluded.sync_state,
            sync_message = excluded.sync_message,
            lag_count = excluded.lag_count,
            last_probe_at = excluded.last_probe_at,
            last_sync_at = excluded.last_sync_at,
            managed_by_app = excluded.managed_by_app",
        params![
            &meta.skill_path,
            &meta.origin_kind,
            &meta.origin_locator,
            &meta.resolved_locator,
            &meta.tracking_ref,
            &meta.installed_ref,
            &meta.latest_ref,
            &meta.sync_state,
            &meta.sync_message,
            meta.lag_count as i64,
            meta.last_probe_at,
            meta.last_sync_at,
            if meta.managed_by_app { 1 } else { 0 }
        ],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn should_skip_probe(meta: &SkillOriginMeta, force: bool) -> bool {
    if force {
        return false;
    }
    let Some(last_probe_at) = meta.last_probe_at else {
        return false;
    };
    let stable = matches!(
        meta.sync_state.as_str(),
        "current"
            | "behind"
            | "unsupported"
            | "ahead_local"
            | "diverged"
            | "local_fixed"
            | "source_missing"
    );
    stable && now_ms() - last_probe_at < 60 * 60 * 1000
}

pub fn infer_origin_meta(path: &Path, existing: Option<SkillOriginMeta>) -> SkillOriginMeta {
    let default_path = path.to_string_lossy().to_string();
    let mut meta = existing.unwrap_or(SkillOriginMeta {
        skill_path: default_path.clone(),
        origin_kind: "unknown".to_string(),
        origin_locator: String::new(),
        resolved_locator: String::new(),
        tracking_ref: String::new(),
        installed_ref: String::new(),
        latest_ref: String::new(),
        sync_state: "unsupported".to_string(),
        sync_message: "未记录来源".to_string(),
        lag_count: 0,
        last_probe_at: None,
        last_sync_at: None,
        managed_by_app: false,
    });
    meta.skill_path = default_path.clone();

    if let Some(repo_root) = find_git_repo_root(path) {
        let remote_url = get_git_remote_url(&repo_root).unwrap_or_default();
        let installed_ref = git_output(&repo_root, &["rev-parse", "HEAD"]).unwrap_or_default();
        let tracking_ref =
            git_output(&repo_root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
        meta.origin_kind = "git".to_string();
        if meta.origin_locator.is_empty() {
            meta.origin_locator = remote_url.clone();
        }
        if meta.resolved_locator.is_empty() {
            meta.resolved_locator = remote_url.clone();
        }
        meta.tracking_ref = tracking_ref;
        meta.installed_ref = installed_ref;
        if meta.sync_state.is_empty() {
            meta.sync_state = if remote_url.is_empty() {
                "unsupported".to_string()
            } else {
                "unprobed".to_string()
            };
        }
        if meta.sync_message.is_empty() {
            meta.sync_message = if remote_url.is_empty() {
                "Git 仓库未配置远端".to_string()
            } else {
                "待检查".to_string()
            };
        }
        if !meta.managed_by_app && !remote_url.is_empty() {
            meta.managed_by_app = true;
        }
        return meta;
    }

    if let Some((name, version)) = detect_npm_package(path) {
        meta.origin_kind = "legacy_npm".to_string();
        if meta.origin_locator.is_empty() {
            meta.origin_locator = name.clone();
        }
        if meta.resolved_locator.is_empty() {
            meta.resolved_locator = name;
        }
        if !version.is_empty() {
            meta.installed_ref = version;
        }
        if meta.sync_state.is_empty() || meta.sync_state == "unsupported" {
            meta.sync_state = "unprobed".to_string();
        }
        if meta.sync_message.is_empty() || meta.sync_message == "未记录来源" {
            meta.sync_message = "历史 npm 来源，仅做环境探测".to_string();
        }
        return meta;
    }

    if let Some((name, version)) = detect_pip_package(path) {
        meta.origin_kind = "legacy_pip".to_string();
        if meta.origin_locator.is_empty() {
            meta.origin_locator = name.clone();
        }
        if meta.resolved_locator.is_empty() {
            meta.resolved_locator = name;
        }
        if !version.is_empty() {
            meta.installed_ref = version;
        }
        if meta.sync_state.is_empty() || meta.sync_state == "unsupported" {
            meta.sync_state = "unprobed".to_string();
        }
        if meta.sync_message.is_empty() || meta.sync_message == "未记录来源" {
            meta.sync_message = "历史 PyPI 来源，仅做环境探测".to_string();
        }
        return meta;
    }

    if meta.origin_kind != "git" {
        if meta.origin_kind.is_empty() || meta.origin_kind == "unknown" {
            meta.origin_kind = "local".to_string();
        }
        if meta.origin_locator.is_empty() {
            meta.origin_locator = default_path;
        }
        if meta.resolved_locator.is_empty() {
            meta.resolved_locator = meta.origin_locator.clone();
        }
        if meta.sync_state.is_empty() || meta.sync_state == "unsupported" {
            meta.sync_state = "unprobed".to_string();
        }
        if meta.sync_message.is_empty() || meta.sync_message == "未记录来源" {
            meta.sync_message = "待检查本地来源".to_string();
        }
    }
    meta
}

pub fn can_sync(meta: &SkillOriginMeta, path: &Path) -> bool {
    match meta.origin_kind.as_str() {
        "git" => {
            if meta.sync_state != "behind" {
                return false;
            }
            if find_git_repo_root(path).is_some() {
                return has_git_upstream(path);
            }
            has_git_subdir_spec(&meta.origin_locator, &meta.resolved_locator)
        }
        _ => false,
    }
}

pub fn build_sync_info(db: &Connection, path: &Path) -> SkillSyncInfo {
    let meta = infer_origin_meta(path, load_origin_meta(db, &path.to_string_lossy()));
    let has_update = meta.sync_state == "behind" || meta.lag_count > 0;
    let can_sync_now = can_sync(&meta, path);
    SkillSyncInfo {
        meta,
        can_sync: can_sync_now,
        has_update,
    }
}

pub fn sync_info_json(info: &SkillSyncInfo) -> serde_json::Value {
    json!({
        "originKind": info.meta.origin_kind,
        "originLocator": info.meta.origin_locator,
        "resolvedLocator": info.meta.resolved_locator,
        "trackingRef": info.meta.tracking_ref,
        "installedRef": info.meta.installed_ref,
        "latestRef": info.meta.latest_ref,
        "syncState": info.meta.sync_state,
        "message": info.meta.sync_message,
        "lagCount": info.meta.lag_count,
        "lastProbeAt": info.meta.last_probe_at,
        "lastSyncAt": info.meta.last_sync_at,
        "managedByApp": info.meta.managed_by_app,
        "canSync": info.can_sync,
        "hasUpdate": info.has_update,
        "behindCount": info.meta.lag_count,
        "remoteUrl": if !info.meta.resolved_locator.is_empty() { info.meta.resolved_locator.clone() } else { info.meta.origin_locator.clone() }
    })
}

fn probe_git_meta(path: &Path, meta: &mut SkillOriginMeta) {
    let Some(repo_root) = find_git_repo_root(path) else {
        if has_git_subdir_spec(&meta.origin_locator, &meta.resolved_locator) {
            match probe_git_subdir_latest_ref(
                &meta.origin_locator,
                &meta.resolved_locator,
                &meta.tracking_ref,
            ) {
                Ok(latest_ref) => {
                    meta.latest_ref = latest_ref;
                    meta.lag_count = if !meta.installed_ref.is_empty()
                        && meta.installed_ref == meta.latest_ref
                    {
                        0
                    } else {
                        1
                    };
                    meta.sync_state = if meta.lag_count > 0 {
                        "behind".to_string()
                    } else {
                        "current".to_string()
                    };
                    meta.sync_message = if meta.lag_count > 0 {
                        "Git 子目录来源有新提交".to_string()
                    } else {
                        "Git 子目录来源已同步".to_string()
                    };
                }
                Err(err) => {
                    meta.sync_state = "failed".to_string();
                    meta.sync_message = format!("检查 Git 子目录失败: {}", err);
                    meta.lag_count = 0;
                }
            }
        } else {
            meta.sync_state = "failed".to_string();
            meta.sync_message = "未找到 Git 仓库".to_string();
            meta.lag_count = 0;
        }
        meta.last_probe_at = Some(now_ms());
        return;
    };

    let remote_url = get_git_remote_url(&repo_root).unwrap_or_default();
    meta.origin_kind = "git".to_string();
    meta.origin_locator = if meta.origin_locator.is_empty() {
        remote_url.clone()
    } else {
        meta.origin_locator.clone()
    };
    meta.resolved_locator = remote_url.clone();
    meta.installed_ref = git_output(&repo_root, &["rev-parse", "HEAD"]).unwrap_or_default();
    meta.tracking_ref =
        git_output(&repo_root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();

    if remote_url.is_empty() {
        meta.latest_ref.clear();
        meta.sync_state = "unsupported".to_string();
        meta.sync_message = "Git 仓库未配置远端".to_string();
        meta.lag_count = 0;
        meta.last_probe_at = Some(now_ms());
        return;
    }

    if !has_git_upstream(&repo_root) {
        meta.latest_ref.clear();
        meta.sync_state = "unsupported".to_string();
        meta.sync_message = "当前分支未关联上游".to_string();
        meta.lag_count = 0;
        meta.last_probe_at = Some(now_ms());
        return;
    }

    match run_command_with_timeout(
        "git",
        &["fetch", "--quiet"],
        Some(&repo_root),
        Duration::from_secs(8),
        &[("GIT_TERMINAL_PROMPT", "0"), ("GCM_INTERACTIVE", "Never")],
    ) {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            meta.sync_state = "failed".to_string();
            meta.sync_message =
                format!("检查失败: {}", String::from_utf8_lossy(&out.stderr).trim());
            meta.lag_count = 0;
            meta.last_probe_at = Some(now_ms());
            return;
        }
        Err(err) => {
            meta.sync_state = "failed".to_string();
            meta.sync_message = format!("检查失败: {}", err);
            meta.lag_count = 0;
            meta.last_probe_at = Some(now_ms());
            return;
        }
    }

    meta.latest_ref = git_output(&repo_root, &["rev-parse", "@{upstream}"]).unwrap_or_default();
    let behind = git_count(&repo_root, "HEAD..@{upstream}");
    let ahead = git_count(&repo_root, "@{upstream}..HEAD");
    meta.lag_count = behind;
    meta.last_probe_at = Some(now_ms());

    if behind == 0 && ahead == 0 {
        meta.sync_state = "current".to_string();
        meta.sync_message = "已与远端同步".to_string();
    } else if behind > 0 && ahead == 0 {
        meta.sync_state = "behind".to_string();
        meta.sync_message = format!("落后 {} 个提交", behind);
    } else if behind == 0 && ahead > 0 {
        meta.sync_state = "ahead_local".to_string();
        meta.sync_message = format!("本地领先 {} 个提交", ahead);
    } else {
        meta.sync_state = "diverged".to_string();
        meta.sync_message = format!("本地与远端已分叉（本地 {} / 远端 {}）", ahead, behind);
    }
}

fn probe_legacy_package_meta(path: &Path, meta: &mut SkillOriginMeta, is_pip: bool) {
    let (label, detector, latest) = if is_pip {
        (
            "PyPI",
            detect_pip_package(path),
            pip_latest_version as fn(&str) -> Result<String, String>,
        )
    } else {
        (
            "npm",
            detect_npm_package(path),
            npm_latest_version as fn(&str) -> Result<String, String>,
        )
    };
    let mut detected_version = String::new();
    if let Some((name, version)) = detector {
        meta.origin_kind = if is_pip { "legacy_pip" } else { "legacy_npm" }.to_string();
        meta.origin_locator = name.clone();
        meta.resolved_locator = name;
        detected_version = version;
    }

    if meta.origin_locator.is_empty() {
        meta.sync_state = "unsupported".to_string();
        meta.sync_message = format!("未识别到 {} 包名", label);
        meta.lag_count = 0;
        meta.last_probe_at = Some(now_ms());
        return;
    }

    if meta.installed_ref.is_empty() && !detected_version.is_empty() {
        meta.installed_ref = detected_version;
    }

    match latest(&meta.origin_locator) {
        Ok(latest_ref) => {
            meta.latest_ref = latest_ref.clone();
            if meta.installed_ref.is_empty() {
                meta.lag_count = 0;
                meta.sync_state = "unsupported".to_string();
                meta.sync_message = format!("历史 {} 来源：未获取到本地版本", label);
            } else {
                meta.lag_count = if meta.installed_ref != latest_ref {
                    1
                } else {
                    0
                };
                meta.sync_state = if meta.lag_count > 0 {
                    "behind".to_string()
                } else {
                    "current".to_string()
                };
                meta.sync_message = if meta.lag_count > 0 {
                    format!(
                        "历史 {} 来源发现新版本 {}，需在外部环境处理",
                        label, latest_ref
                    )
                } else {
                    format!("历史 {} 来源已是最新", label)
                };
            }
        }
        Err(err) => {
            meta.sync_state = "failed".to_string();
            meta.sync_message = format!("历史 {} 来源检查失败: {}", label, err);
            meta.lag_count = 0;
        }
    }
    meta.last_probe_at = Some(now_ms());
}

fn probe_local_meta(path: &Path, meta: &mut SkillOriginMeta) {
    meta.origin_kind = "local".to_string();
    if meta.origin_locator.is_empty() {
        meta.origin_locator = path.to_string_lossy().to_string();
    }
    if meta.resolved_locator.is_empty() {
        meta.resolved_locator = meta.origin_locator.clone();
    }
    let exists = Path::new(&meta.origin_locator).exists();
    meta.sync_state = if exists {
        "local_fixed".to_string()
    } else {
        "source_missing".to_string()
    };
    meta.sync_message = if exists {
        "本地来源可用".to_string()
    } else {
        "原始本地来源已不存在".to_string()
    };
    meta.lag_count = 0;
    meta.latest_ref.clear();
    meta.last_probe_at = Some(now_ms());
}

pub fn probe_skill_state(
    db: &Connection,
    path: &Path,
    force: bool,
) -> Result<SkillSyncInfo, String> {
    let mut meta = infer_origin_meta(path, load_origin_meta(db, &path.to_string_lossy()));
    if should_skip_probe(&meta, force) {
        let can_sync_now = can_sync(&meta, path);
        let has_update = meta.sync_state == "behind" || meta.lag_count > 0;
        return Ok(SkillSyncInfo {
            meta,
            can_sync: can_sync_now,
            has_update,
        });
    }

    match meta.origin_kind.as_str() {
        "git" => probe_git_meta(path, &mut meta),
        "npm" | "legacy_npm" => probe_legacy_package_meta(path, &mut meta, false),
        "pip" | "legacy_pip" => probe_legacy_package_meta(path, &mut meta, true),
        "local" => probe_local_meta(path, &mut meta),
        _ => {
            meta.sync_state = "unsupported".to_string();
            meta.sync_message = "当前来源暂不支持自动探测".to_string();
            meta.lag_count = 0;
            meta.last_probe_at = Some(now_ms());
        }
    }

    save_origin_meta(db, &meta)?;
    let can_sync_now = can_sync(&meta, path);
    let has_update = meta.sync_state == "behind" || meta.lag_count > 0;
    Ok(SkillSyncInfo {
        meta,
        can_sync: can_sync_now,
        has_update,
    })
}

pub fn save_installed_git_meta(
    db: &Connection,
    target_path: &Path,
    spec: &GitInstallSpec,
    outcome: &GitInstallOutcome,
) -> Result<(), String> {
    let mut meta = infer_origin_meta(target_path, None);
    meta.skill_path = target_path.to_string_lossy().to_string();
    meta.origin_kind = "git".to_string();
    meta.origin_locator = spec.original.clone();
    meta.resolved_locator = spec.repo_url.clone();
    meta.tracking_ref = spec.reference.clone().unwrap_or(meta.tracking_ref);
    meta.installed_ref = outcome.installed_ref.clone();
    meta.managed_by_app = true;
    if spec.subdir.is_some() && find_git_repo_root(target_path).is_none() {
        meta.sync_state = "unprobed".to_string();
        meta.sync_message = "待检查 Git 子目录来源".to_string();
        meta.lag_count = 0;
    } else if meta.sync_state.is_empty() || meta.sync_state == "unsupported" {
        meta.sync_state = "unprobed".to_string();
        meta.sync_message = "待检查 Git 来源".to_string();
    }
    if outcome.structure.structure_status == "nonstandard" && meta.sync_message.is_empty() {
        meta.sync_message = "已安装，但 Skill 结构非标准".to_string();
    }
    save_origin_meta(db, &meta)
}

pub fn update_skill_from_upstream(db: &Connection, path: &Path) -> Result<String, String> {
    let mut sync_info = probe_skill_state(db, path, true)?;
    if sync_info.meta.sync_state == "current" {
        return Ok("已是最新".to_string());
    }
    if matches!(
        sync_info.meta.sync_state.as_str(),
        "ahead_local" | "diverged"
    ) {
        return Err(sync_info.meta.sync_message.clone());
    }
    if !can_sync(&sync_info.meta, path) {
        return Err(if sync_info.meta.sync_message.is_empty() {
            "当前状态不允许自动更新".to_string()
        } else {
            sync_info.meta.sync_message.clone()
        });
    }

    if sync_info.meta.origin_kind != "git" {
        return Err("当前来源暂不支持一键更新".to_string());
    }

    if let Some(repo_root) = find_git_repo_root(path) {
        let out = run_command_with_timeout(
            "git",
            &["pull", "--ff-only"],
            Some(&repo_root),
            Duration::from_secs(60),
            &[("GIT_TERMINAL_PROMPT", "0"), ("GCM_INTERACTIVE", "Never")],
        )?;
        if !out.status.success() {
            let err = format!("更新失败: {}", String::from_utf8_lossy(&out.stderr).trim());
            sync_info.meta.sync_state = "failed".to_string();
            sync_info.meta.sync_message = err.clone();
            sync_info.meta.last_probe_at = Some(now_ms());
            save_origin_meta(db, &sync_info.meta)?;
            return Err(err);
        }
    } else if has_git_subdir_spec(
        &sync_info.meta.origin_locator,
        &sync_info.meta.resolved_locator,
    ) {
        let outcome = sync_git_subdir_skill(
            &sync_info.meta.origin_locator,
            &sync_info.meta.resolved_locator,
            &sync_info.meta.tracking_ref,
            path,
        )?;
        let now = now_ms();
        sync_info.meta.installed_ref = outcome.installed_ref.clone();
        sync_info.meta.latest_ref = outcome.installed_ref;
        sync_info.meta.sync_state = "current".to_string();
        sync_info.meta.sync_message = "Git 子目录更新成功".to_string();
        sync_info.meta.lag_count = 0;
        sync_info.meta.last_probe_at = Some(now);
        sync_info.meta.last_sync_at = Some(now);
        save_origin_meta(db, &sync_info.meta)?;
        return Ok("更新成功".to_string());
    } else {
        return Err("不是 Git 仓库".to_string());
    }

    sync_info = probe_skill_state(db, path, true)?;
    sync_info.meta.last_sync_at = Some(now_ms());
    if sync_info.meta.sync_state == "current" {
        sync_info.meta.sync_message = "更新成功，已与远端同步".to_string();
    }
    save_origin_meta(db, &sync_info.meta)?;
    Ok("更新成功".to_string())
}

fn npm_latest_version(package: &str) -> Result<String, String> {
    if !command_exists("npm") {
        return Err("未安装 npm".to_string());
    }
    let out = run_command_with_timeout(
        "npm",
        &["view", package, "version", "--json"],
        None,
        Duration::from_secs(8),
        &[("npm_config_loglevel", "error")],
    )?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if stdout.is_empty() {
        return Err("未获取到 npm 版本".to_string());
    }
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&stdout);
    match parsed {
        Ok(serde_json::Value::String(v)) if !v.trim().is_empty() => Ok(v),
        Ok(serde_json::Value::Array(arr)) => arr
            .last()
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "未获取到 npm 版本".to_string()),
        _ => Ok(stdout.trim_matches('"').to_string()),
    }
}

fn pip_latest_version(package: &str) -> Result<String, String> {
    if !command_exists("pip") {
        return Err("未安装 pip".to_string());
    }
    let out = run_command_with_timeout(
        "pip",
        &["index", "versions", package],
        None,
        Duration::from_secs(8),
        &[
            ("PIP_DISABLE_PIP_VERSION_CHECK", "1"),
            ("PIP_NO_INPUT", "1"),
        ],
    )?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Available versions:") {
            let latest = rest
                .split(',')
                .map(|v| v.trim())
                .find(|v| !v.is_empty())
                .unwrap_or("");
            if !latest.is_empty() {
                return Ok(latest.to_string());
            }
        }
    }
    Err("未获取到 PyPI 版本".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("skillmate-origin-test-{}-{}", name, now_ms()))
    }

    #[test]
    fn git_subdir_meta_can_sync_without_local_git_repo() {
        let path = test_dir("git-subdir-sync").join("writer");
        let meta = SkillOriginMeta {
            skill_path: path.to_string_lossy().to_string(),
            origin_kind: "git".into(),
            origin_locator: "example/skills#main:skills/writer".into(),
            resolved_locator: "https://github.com/example/skills.git".into(),
            tracking_ref: "main".into(),
            installed_ref: "old".into(),
            latest_ref: "new".into(),
            sync_state: "behind".into(),
            sync_message: "Git 子目录来源有新提交".into(),
            lag_count: 1,
            last_probe_at: None,
            last_sync_at: None,
            managed_by_app: true,
        };

        assert!(can_sync(&meta, &path));
    }
}
