use super::*;
use crate::app_core::remove_path;

fn write_skill(path: &Path, name: &str) {
    fs::create_dir_all(path).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: 场景测试\n---\n"),
    )
    .unwrap();
}

fn preview_local(
    db: &Connection,
    path: &Path,
    mode: &str,
    project: Option<&Path>,
) -> InstallPreview {
    let package = path.to_string_lossy();
    let project = project.map(|value| value.to_string_lossy().to_string());
    let policy = load_install_policy(db).unwrap();
    build_install_request_preview(
        db,
        InstallPreviewRequest {
            package: &package,
            source: "local",
            assistant_name: if mode == "library" { "" } else { "Codex" },
            mode,
            project_path: project.as_deref(),
            selected_skill_paths: None,
            preferred_skill_id: None,
        },
        Ok(&policy),
    )
}

fn apply_local(
    db: &Connection,
    path: &Path,
    mode: &str,
    project: Option<&Path>,
    plan_token: String,
) -> InstallResult {
    install_skill_exclusive(
        db,
        InstallSkillRequest {
            package: path.to_string_lossy().to_string(),
            source: "local".into(),
            assistant_name: if mode == "library" { "" } else { "Codex" }.into(),
            install_mode: Some(mode.into()),
            project_path: project.map(|value| value.to_string_lossy().to_string()),
            selected_skill_paths: None,
            preferred_skill_id: None,
            plan_token: Some(plan_token),
        },
    )
}

fn add_library_skill(db: &Connection, root: &Path, name: &str) -> PathBuf {
    let source = root.join("source").join(name);
    write_skill(&source, name);
    let preview = preview_local(db, &source, "library", None);
    assert!(preview.can_apply, "{}", preview.message);
    let result = apply_local(db, &source, "library", None, preview.plan_token);
    assert!(result.success, "{}: {}", result.message, result.output);
    root.join("library").join(name)
}

fn insert_scenario(db: &Connection, id: &str, paths: &[String]) {
    db.execute(
        "INSERT INTO scenarios (id, name, description, skill_ids, skill_ids_json, created_at)
         VALUES (?, ?, '', '', ?, '2026-10-10')",
        params![id, id, serde_json::to_string(paths).unwrap()],
    )
    .unwrap();
}

#[test]
fn adoption_migrates_saved_scenario_members_to_library_identity() {
    let db = test_db();
    let root = test_dir("scenario-adoption");
    let project = root.join("project");
    let source = project.join(".agents/skills/writer");
    let library = root.join("library");
    let _guard = skill_library::use_test_library_root(library.clone());
    write_skill(&source, "writer");
    let library_path = library.join("writer").to_string_lossy().to_string();
    let source_path = source.to_string_lossy().to_string();
    insert_scenario(
        &db,
        "review",
        &[
            source_path.clone(),
            "/other/reviewer".into(),
            library_path.clone(),
        ],
    );
    insert_scenario(&db, "unrelated", &["/other/skill".into()]);
    let project_path = project.to_string_lossy().to_string();
    let preview =
        skill_adoption::preview_adopt_skill(&db, &source_path, "Codex", Some(&project_path));
    assert!(preview.can_apply, "{}", preview.message);
    let result = skill_adoption::adopt_skill(
        &db,
        source_path,
        "Codex".into(),
        Some(project_path),
        Some(preview.plan_token),
    );
    assert!(result.success, "{}: {}", result.message, result.output);
    let scenarios = get_scenarios_from_db(&db).unwrap();
    assert_eq!(
        scenarios
            .iter()
            .find(|item| item.id == "review")
            .unwrap()
            .skill_ids,
        [library_path, "/other/reviewer".into()]
    );
    assert_eq!(
        scenarios
            .iter()
            .find(|item| item.id == "unrelated")
            .unwrap()
            .skill_ids,
        ["/other/skill"]
    );
    assert!(fs::symlink_metadata(&source)
        .unwrap()
        .file_type()
        .is_symlink());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn adoption_rolls_back_migrated_scenarios_when_a_later_reference_update_fails() {
    let db = test_db();
    let root = test_dir("scenario-adoption-rollback");
    let project = root.join("project");
    let source = project.join(".agents/skills/writer");
    let library = root.join("library");
    let _guard = skill_library::use_test_library_root(library.clone());
    write_skill(&source, "writer");
    let source_path = source.to_string_lossy().to_string();
    for id in ["first", "second"] {
        insert_scenario(&db, id, std::slice::from_ref(&source_path));
    }
    db.execute_batch(
        "CREATE TRIGGER reject_second_scenario BEFORE UPDATE ON scenarios
         WHEN NEW.id = 'second' AND instr(NEW.skill_ids_json, '/library/') > 0
         BEGIN SELECT RAISE(ABORT, '场景写入失败'); END;",
    )
    .unwrap();
    let project_path = project.to_string_lossy().to_string();
    let preview =
        skill_adoption::preview_adopt_skill(&db, &source_path, "Codex", Some(&project_path));
    let result = skill_adoption::adopt_skill(
        &db,
        source_path.clone(),
        "Codex".into(),
        Some(project_path),
        Some(preview.plan_token),
    );
    assert!(!result.success, "迁移失败必须回滚接管");
    assert!(result.output.contains("场景写入失败"), "{}", result.output);
    for scenario in get_scenarios_from_db(&db).unwrap() {
        assert_eq!(
            scenario.skill_ids.as_slice(),
            std::slice::from_ref(&source_path)
        );
    }
    assert!(!fs::symlink_metadata(&source)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!library.join("writer").exists());
    for table in [
        "library_skills",
        "skill_deployments",
        "managed_installations",
        "skill_origin_meta",
    ] {
        let count: i64 = db
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
    assert!(!source
        .parent()
        .unwrap()
        .join(managed_state::STATE_FILE_NAME)
        .exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn scenario_enablement_keeps_completed_members_and_fills_missing_targets() {
    let db = test_db();
    let root = test_dir("scenario-enable-keep");
    let _guard = skill_library::use_test_library_root(root.join("library"));
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let writer = add_library_skill(&db, &root, "writer");
    let reviewer = add_library_skill(&db, &root, "reviewer");
    let preview = preview_local(&db, &writer, "symlink", Some(&project));
    let result = apply_local(&db, &writer, "symlink", Some(&project), preview.plan_token);
    assert!(result.success, "{}: {}", result.message, result.output);
    let target = project.join(".agents/skills/writer");
    let original_link = fs::read_link(&target).unwrap();
    let original_state = fs::read(
        target
            .parent()
            .unwrap()
            .join(managed_state::STATE_FILE_NAME),
    )
    .unwrap();
    let changes = db.total_changes();

    let keep = preview_local(&db, &writer, "symlink", Some(&project));
    let missing = preview_local(&db, &reviewer, "symlink", Some(&project));
    assert!(keep.can_apply, "{}", keep.message);
    assert!(missing.can_apply, "{}", missing.message);
    assert!(keep
        .target_actions
        .iter()
        .any(|action| action.action == "keep" && Path::new(&action.target) == target));
    let kept = apply_local(&db, &writer, "symlink", Some(&project), keep.plan_token);
    assert!(kept.success, "{}: {}", kept.message, kept.output);
    assert_eq!(fs::read_link(&target).unwrap(), original_link);
    assert_eq!(
        fs::read(
            target
                .parent()
                .unwrap()
                .join(managed_state::STATE_FILE_NAME)
        )
        .unwrap(),
        original_state
    );
    assert_eq!(db.total_changes(), changes);
    let added = apply_local(
        &db,
        &reviewer,
        "symlink",
        Some(&project),
        missing.plan_token,
    );
    assert!(added.success, "{}: {}", added.message, added.output);
    assert!(project.join(".agents/skills/reviewer/SKILL.md").is_file());

    let changes = db.total_changes();
    for path in [&writer, &reviewer] {
        let retry = preview_local(&db, path, "symlink", Some(&project));
        assert!(retry.can_apply, "{}", retry.message);
        let result = apply_local(&db, path, "symlink", Some(&project), retry.plan_token);
        assert!(result.success, "{}: {}", result.message, result.output);
    }
    assert_eq!(db.total_changes(), changes, "重复启用不得重写登记");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn enablement_keeps_external_links_and_entities_protected() {
    let db = test_db();
    let root = test_dir("scenario-enable-external");
    let _guard = skill_library::use_test_library_root(root.join("library"));
    let project = root.join("project");
    let writer = add_library_skill(&db, &root, "writer");
    let target = project.join(".agents/skills/writer");
    skill_library::deploy_library_skill(&writer, &target).unwrap();
    let preview = preview_local(&db, &writer, "symlink", Some(&project));
    assert!(!preview.can_apply);
    assert!(preview
        .conflicts
        .iter()
        .any(|item| item.reason == "external_target_exists"));
    remove_path(&target).unwrap();
    write_skill(&target, "writer");
    let preview = preview_local(&db, &writer, "symlink", Some(&project));
    assert!(!preview.can_apply);
    assert!(preview
        .conflicts
        .iter()
        .any(|item| item.reason == "external_target_exists"));
    assert!(target.join("SKILL.md").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn kept_enablement_plan_is_rejected_when_the_link_changes_after_preview() {
    let db = test_db();
    let root = test_dir("scenario-enable-changed");
    let _guard = skill_library::use_test_library_root(root.join("library"));
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let writer = add_library_skill(&db, &root, "writer");
    let reviewer = add_library_skill(&db, &root, "reviewer");
    let preview = preview_local(&db, &writer, "symlink", Some(&project));
    let result = apply_local(&db, &writer, "symlink", Some(&project), preview.plan_token);
    assert!(result.success);
    let keep = preview_local(&db, &writer, "symlink", Some(&project));
    assert!(keep.can_apply, "{}", keep.message);
    let target = project.join(".agents/skills/writer");
    remove_path(&target).unwrap();
    skill_library::deploy_library_skill(&reviewer, &target).unwrap();
    let result = apply_local(&db, &writer, "symlink", Some(&project), keep.plan_token);
    assert!(!result.success);
    assert_eq!(
        fs::canonicalize(&target).unwrap(),
        fs::canonicalize(&reviewer).unwrap()
    );
    fs::remove_dir_all(root).unwrap();
}
