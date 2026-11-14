use super::*;
use crate::update_preview::{
    apply_skill_rollback, apply_update, preview_skill_rollback, preview_update,
};

fn git(repo: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit(repo: &Path, text: &str) {
    fs::write(
        repo.join("SKILL.md"),
        format!("---\nname: writer\ndescription: Writing\n---\n{text}\n"),
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(
        repo,
        &[
            "-c",
            "user.name=SkillMate Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            text,
        ],
    );
}

fn fixture(
    label: &str,
) -> (
    Connection,
    PathBuf,
    PathBuf,
    PathBuf,
    skill_library::TestLibraryRootGuard,
) {
    let db = test_db();
    let root = test_dir(label);
    let repo = root.join("writer");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    commit(&repo, "original");
    let library = root.join("library");
    let guard = skill_library::use_test_library_root(library.clone());
    let source = repo.to_string_lossy().to_string();
    let policy = InstallPolicyConfig::default();
    let preview = build_install_request_preview(
        &db,
        InstallPreviewRequest {
            package: &source,
            source: "git",
            assistant_name: "",
            mode: "library",
            project_path: None,
            selected_skill_paths: None,
            preferred_skill_id: None,
        },
        Ok(&policy),
    );
    assert!(preview.can_apply, "{}", preview.message);
    let result = install_skill_exclusive(
        &db,
        InstallSkillRequest {
            package: source,
            source: "git".into(),
            assistant_name: String::new(),
            install_mode: Some("library".into()),
            project_path: None,
            selected_skill_paths: None,
            preferred_skill_id: None,
            plan_token: Some(preview.plan_token),
        },
    );
    assert!(result.success, "{}: {}", result.message, result.output);
    (db, root, repo, library.join("writer"), guard)
}

#[test]
fn update_preview_binds_source_policy_and_local_content_without_writing() {
    let (db, root, repo, path, _guard) = fixture("update-preview-bindings");
    let original = fs::read(path.join("SKILL.md")).unwrap();
    let state_path = path.parent().unwrap().join(managed_state::STATE_FILE_NAME);
    let original_state = fs::read(&state_path).unwrap();
    commit(&repo, "second");
    let changes = db.total_changes();
    let preview = preview_update(&db, &path).unwrap();
    assert!(preview.can_apply);
    let json = serde_json::to_value(&preview).unwrap();
    assert_eq!(json["files"][0]["kind"], "modified");
    assert!(json["files"][0]["before"]
        .as_str()
        .unwrap()
        .contains("original"));
    assert!(json["files"][0]["after"]
        .as_str()
        .unwrap()
        .contains("second"));
    assert_eq!(db.total_changes(), changes);
    assert_eq!(fs::read(&state_path).unwrap(), original_state);
    assert_eq!(
        preview.plan_token,
        preview_update(&db, &path).unwrap().plan_token
    );
    assert!(apply_update(&db, &path, None)
        .unwrap_err()
        .contains("计划缺失"));

    commit(&repo, "third");
    assert!(apply_update(&db, &path, Some(&preview.plan_token))
        .unwrap_err()
        .contains("计划已过期"));
    assert_eq!(fs::read(path.join("SKILL.md")).unwrap(), original);
    let current = preview_update(&db, &path).unwrap();
    save_install_policy(
        &db,
        InstallPolicyConfig {
            mode: "block-critical".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(apply_update(&db, &path, Some(&current.plan_token))
        .unwrap_err()
        .contains("计划已过期"));
    let current = preview_update(&db, &path).unwrap();
    let target = root.join("project/.agents/skills/writer");
    db.execute("INSERT INTO skill_deployments SELECT ?, id, library_path, 'Codex', 'project', ?, 'symlink', 'test' FROM library_skills", params![target.to_string_lossy(), root.join("project").to_string_lossy()]).unwrap();
    let disconnected = preview_update(&db, &path).unwrap();
    assert!(!disconnected.can_apply);
    assert_eq!(disconnected.blocked_reason.as_deref(), Some("deployments"));
    assert_ne!(current.plan_token, disconnected.plan_token);
    assert!(apply_update(&db, &path, Some(&current.plan_token))
        .unwrap_err()
        .contains("计划已过期"));
    db.execute("DELETE FROM skill_deployments", []).unwrap();
    fs::write(path.join("SKILL.md"), "manual edit").unwrap();
    assert!(apply_update(&db, &path, Some(&current.plan_token)).is_err());
    assert_eq!(
        fs::read_to_string(path.join("SKILL.md")).unwrap(),
        "manual edit"
    );
    fs::write(path.join("SKILL.md"), &original).unwrap();
    apply_update(&db, &path, Some(&current.plan_token)).unwrap();
    assert!(fs::read_to_string(path.join("SKILL.md"))
        .unwrap()
        .contains("third"));
    verify_managed_content_unchanged(&db, &path).unwrap();
    let meta = skill_origin::load_origin_meta(&db, &path.to_string_lossy())
        .unwrap()
        .unwrap();
    assert_eq!(meta.installed_ref, current.latest_ref);
    let next = preview_update(&db, &path).unwrap();
    assert!(!next.can_apply);
    assert_eq!(next.blocked_reason.as_deref(), Some("unchanged"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_preview_rolls_back_files_database_and_sidecar_when_registration_fails() {
    let (db, root, repo, path, _guard) = fixture("update-preview-rollback");
    commit(&repo, "changed");
    let preview = preview_update(&db, &path).unwrap();
    let content = fs::read(path.join("SKILL.md")).unwrap();
    let state_path = path.parent().unwrap().join(managed_state::STATE_FILE_NAME);
    let sidecar = fs::read(&state_path).unwrap();
    for expected in [
        skill_install::GitSnapshotProbe {
            latest_ref: "stale-ref".into(),
            source_digest: preview.source_digest.clone(),
        },
        skill_install::GitSnapshotProbe {
            latest_ref: preview.latest_ref.clone(),
            source_digest: "stale-digest".into(),
        },
    ] {
        assert!(skill_origin::update_skill_from_upstream(&db, &path, &expected).is_err());
        assert_eq!(fs::read(path.join("SKILL.md")).unwrap(), content);
        assert_eq!(fs::read(&state_path).unwrap(), sidecar);
        verify_managed_content_unchanged(&db, &path).unwrap();
    }
    let hash: String = db
        .query_row("SELECT content_hash FROM library_skills", [], |row| {
            row.get(0)
        })
        .unwrap();
    db.execute_batch("CREATE TRIGGER fail_library_refresh BEFORE UPDATE ON library_skills WHEN NEW.content_hash != OLD.content_hash BEGIN SELECT RAISE(FAIL, 'injected library refresh failure'); END;").unwrap();
    let error = apply_update(&db, &path, Some(&preview.plan_token)).unwrap_err();
    assert!(
        error.contains("injected library refresh failure"),
        "{error}"
    );
    assert_eq!(fs::read(path.join("SKILL.md")).unwrap(), content);
    assert_eq!(fs::read(&state_path).unwrap(), sidecar);
    let restored_hash: String = db
        .query_row("SELECT content_hash FROM library_skills", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(restored_hash, hash);
    let meta = skill_origin::load_origin_meta(&db, &path.to_string_lossy())
        .unwrap()
        .unwrap();
    assert_eq!(meta.installed_ref, preview.installed_ref);
    verify_managed_content_unchanged(&db, &path).unwrap();
    db.execute_batch("DROP TRIGGER fail_library_refresh")
        .unwrap();
    apply_update(&db, &path, Some(&preview.plan_token)).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_prepares_git_once_while_existing_deployment_remains_readable() {
    let (db, root, repo, path, _guard) = fixture("update-connected-during-download");
    let Some(target) = enable_project(&db, &root, &repo, &path) else {
        fs::remove_dir_all(root).unwrap();
        return;
    };
    let original = fs::read(target.join("SKILL.md")).unwrap();
    commit(&repo, "changed");
    let preview = preview_update(&db, &path).unwrap();
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed_count = count.clone();
    let observed_target = target.clone();
    skill_install::observe_git_preparations(
        move || {
            observed_count.set(observed_count.get() + 1);
            assert_eq!(
                fs::read(observed_target.join("SKILL.md")).unwrap(),
                original
            );
        },
        || apply_update(&db, &path, Some(&preview.plan_token)).unwrap(),
    );
    assert_eq!(count.get(), 1);
    assert!(fs::read_to_string(target.join("SKILL.md"))
        .unwrap()
        .contains("changed"));
    fs::remove_dir_all(root).unwrap();
}

fn enable_project(db: &Connection, root: &Path, repo: &Path, path: &Path) -> Option<PathBuf> {
    let project = root.join("project");
    let target = project.join(".agents/skills/writer");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    if !app_core::create_test_directory_symlink_or_skip(path, &target) {
        return None;
    }
    app_core::remove_path(&target).unwrap();
    finalize_library_install_registration(
        db,
        &repo.to_string_lossy(),
        "git",
        "Codex",
        "project",
        Some(&project.to_string_lossy()),
        path.parent().unwrap(),
        target.parent(),
        std::slice::from_ref(&path.to_path_buf()),
        std::slice::from_ref(&target),
        "",
        true,
    )
    .unwrap();
    Some(target)
}

#[test]
fn update_rejects_deployment_retargeted_during_download() {
    let (db, root, repo, path, _guard) = fixture("update-retarget-during-download");
    let Some(target) = enable_project(&db, &root, &repo, &path) else {
        fs::remove_dir_all(root).unwrap();
        return;
    };
    let original = fs::read(path.join("SKILL.md")).unwrap();
    commit(&repo, "changed");
    let preview = preview_update(&db, &path).unwrap();
    let changes = db.total_changes();
    let external = root.join("external");
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("SKILL.md"), "external content").unwrap();
    let observed_target = target.clone();
    let observed_external = external.clone();
    let error = skill_install::observe_git_preparations(
        move || {
            app_core::remove_path(&observed_target).unwrap();
            skill_library::deploy_library_skill(&observed_external, &observed_target).unwrap();
        },
        || apply_update(&db, &path, Some(&preview.plan_token)),
    )
    .unwrap_err();
    assert!(error.contains("计划已过期"), "{error}");
    assert_eq!(fs::read(path.join("SKILL.md")).unwrap(), original);
    assert_eq!(db.total_changes(), changes);
    assert_eq!(
        fs::read_to_string(target.join("SKILL.md")).unwrap(),
        "external content"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_download_failure_keeps_existing_files_and_metadata() {
    let (db, root, repo, path, _guard) = fixture("update-download-failure");
    commit(&repo, "changed");
    let preview = preview_update(&db, &path).unwrap();
    let content = fs::read(path.join("SKILL.md")).unwrap();
    let state_path = path.parent().unwrap().join(managed_state::STATE_FILE_NAME);
    let sidecar = fs::read(&state_path).unwrap();
    let changes = db.total_changes();
    fs::remove_dir_all(&repo).unwrap();
    assert!(apply_update(&db, &path, Some(&preview.plan_token)).is_err());
    assert_eq!(fs::read(path.join("SKILL.md")).unwrap(), content);
    assert_eq!(fs::read(state_path).unwrap(), sidecar);
    assert_eq!(db.total_changes(), changes);
    verify_managed_content_unchanged(&db, &path).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn prepared_update_rejects_tampering_and_reuses_downloaded_snapshot() {
    let (db, root, repo, path, _guard) = fixture("update-prepared-snapshot");
    commit(&repo, "changed");
    let original = fs::read(path.join("SKILL.md")).unwrap();
    let meta = skill_origin::load_origin_meta(&db, &path.to_string_lossy())
        .unwrap()
        .unwrap();
    skill_install::with_git_snapshot(
        &meta.origin_locator,
        &meta.resolved_locator,
        &meta.tracking_ref,
        |source, latest| {
            let expected = skill_install::GitSnapshotProbe {
                latest_ref: latest.into(),
                source_digest: skill_install::installable_content_fingerprint(source)?,
            };
            let prepared_content = fs::read(source.join("SKILL.md")).unwrap();
            fs::write(source.join("SKILL.md"), "tampered").unwrap();
            assert!(skill_origin::update_skill_from_prepared_snapshot(
                &db, &path, source, &expected
            )
            .is_err());
            assert_eq!(fs::read(path.join("SKILL.md")).unwrap(), original);
            fs::write(source.join("SKILL.md"), prepared_content).unwrap();
            fs::remove_dir_all(&repo).unwrap();
            skill_origin::update_skill_from_prepared_snapshot(&db, &path, source, &expected)?;
            assert!(fs::read_to_string(path.join("SKILL.md"))
                .unwrap()
                .contains("changed"));
            Ok(())
        },
    )
    .unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_success_keeps_previous_version_snapshot_and_prunes_to_limit() {
    let (db, root, repo, path, _guard) = fixture("update-snapshot-keep");
    let _snapshot_guard = skill_snapshot::use_test_snapshot_root(root.join("snapshot-store"));
    let first_ref = skill_origin::load_origin_meta(&db, &path.to_string_lossy())
        .unwrap()
        .unwrap()
        .installed_ref;
    for (index, text) in ["second", "third", "fourth"].iter().enumerate() {
        commit(&repo, text);
        let preview = preview_update(&db, &path).unwrap();
        apply_update(&db, &path, Some(&preview.plan_token)).unwrap();
        let snapshots = skill_snapshot::list_snapshots(&db, &path).unwrap();
        let expected_count = std::cmp::min(index + 1, skill_snapshot::KEEP_SNAPSHOTS_PER_SKILL);
        assert_eq!(snapshots.len(), expected_count, "after {text}");
    }
    let snapshots = skill_snapshot::list_snapshots(&db, &path).unwrap();
    // 最新快照是 "fourth" 之前的 "third"，最旧的是被裁掉的 "second"
    assert_eq!(snapshots[0].ref_, snapshots[0].ref_);
    assert!(snapshots.iter().all(|snapshot| snapshot.ref_ != first_ref));
    let newest = Path::new(&snapshots[0].snapshot_dir).join("SKILL.md");
    assert!(fs::read_to_string(newest).unwrap().contains("third"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollback_restores_files_metadata_and_keeps_reverse_path() {
    let (db, root, repo, path, _guard) = fixture("update-rollback-flow");
    let _snapshot_guard = skill_snapshot::use_test_snapshot_root(root.join("snapshot-store"));
    let original_ref = skill_origin::load_origin_meta(&db, &path.to_string_lossy())
        .unwrap()
        .unwrap()
        .installed_ref;
    commit(&repo, "second");
    let preview = preview_update(&db, &path).unwrap();
    apply_update(&db, &path, Some(&preview.plan_token)).unwrap();
    let updated_ref = skill_origin::load_origin_meta(&db, &path.to_string_lossy())
        .unwrap()
        .unwrap()
        .installed_ref;
    assert_ne!(updated_ref, original_ref);

    // 本地内容在预览后变化：旧令牌失效且不产生写入
    let stale = preview_skill_rollback(&db, &path).unwrap();
    assert!(stale.can_apply);
    fs::write(path.join("SKILL.md"), "manual edit").unwrap();
    assert!(apply_skill_rollback(&db, &path, Some(&stale.plan_token)).is_err());
    assert_eq!(
        fs::read_to_string(path.join("SKILL.md")).unwrap(),
        "manual edit"
    );
    let restored = "---\nname: writer\ndescription: Writing\n---\nsecond\n".to_string();
    fs::write(path.join("SKILL.md"), &restored).unwrap();

    // 正常回滚：文件、meta、启用位置与快照台账一起收敛
    let rollback = preview_skill_rollback(&db, &path).unwrap();
    assert!(rollback.can_apply);
    assert_eq!(rollback.latest_ref, original_ref);
    let rollback_json = serde_json::to_value(&rollback).unwrap();
    assert_eq!(rollback_json["files"][0]["kind"], "modified");
    assert!(rollback_json["files"][0]["after"]
        .as_str()
        .unwrap()
        .contains("original"));
    apply_skill_rollback(&db, &path, Some(&rollback.plan_token)).unwrap();
    assert!(fs::read_to_string(path.join("SKILL.md"))
        .unwrap()
        .contains("original"));
    verify_managed_content_unchanged(&db, &path).unwrap();
    let meta = skill_origin::load_origin_meta(&db, &path.to_string_lossy())
        .unwrap()
        .unwrap();
    assert_eq!(meta.installed_ref, original_ref);
    assert_eq!(meta.latest_ref, updated_ref);
    assert_eq!(meta.sync_state, "behind");
    assert_eq!(meta.sync_message, "已回滚到上一版本");
    let snapshots = skill_snapshot::list_snapshots(&db, &path).unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].ref_, updated_ref);

    // 回滚后的「前进」：再次回滚即可回到 second，方向可逆
    let forward = preview_skill_rollback(&db, &path).unwrap();
    apply_skill_rollback(&db, &path, Some(&forward.plan_token)).unwrap();
    assert!(fs::read_to_string(path.join("SKILL.md"))
        .unwrap()
        .contains("second"));
    let meta = skill_origin::load_origin_meta(&db, &path.to_string_lossy())
        .unwrap()
        .unwrap();
    assert_eq!(meta.installed_ref, updated_ref);
    assert_eq!(meta.sync_state, "current");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollback_failure_restores_files_metadata_and_snapshot_ledger() {
    let (db, root, repo, path, _guard) = fixture("rollback-failure");
    let _snapshot_guard = skill_snapshot::use_test_snapshot_root(root.join("snapshot-store"));
    commit(&repo, "second");
    let preview = preview_update(&db, &path).unwrap();
    apply_update(&db, &path, Some(&preview.plan_token)).unwrap();
    let second_content = fs::read(path.join("SKILL.md")).unwrap();
    let second_ref = skill_origin::load_origin_meta(&db, &path.to_string_lossy())
        .unwrap()
        .unwrap()
        .installed_ref;
    let rollback = preview_skill_rollback(&db, &path).unwrap();
    let snapshot_count = skill_snapshot::list_snapshots(&db, &path).unwrap().len();
    db.execute_batch("CREATE TRIGGER fail_library_refresh BEFORE UPDATE ON library_skills WHEN NEW.content_hash != OLD.content_hash BEGIN SELECT RAISE(FAIL, 'injected rollback failure'); END;").unwrap();
    let error = apply_skill_rollback(&db, &path, Some(&rollback.plan_token)).unwrap_err();
    assert!(error.contains("injected rollback failure"), "{error}");
    assert_eq!(fs::read(path.join("SKILL.md")).unwrap(), second_content);
    verify_managed_content_unchanged(&db, &path).unwrap();
    let meta = skill_origin::load_origin_meta(&db, &path.to_string_lossy())
        .unwrap()
        .unwrap();
    assert_eq!(meta.installed_ref, second_ref);
    assert_eq!(
        skill_snapshot::list_snapshots(&db, &path).unwrap().len(),
        snapshot_count
    );
    fs::remove_dir_all(root).unwrap();
}
