use super::*;
use crate::update_preview::{apply_update, preview_update};

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
