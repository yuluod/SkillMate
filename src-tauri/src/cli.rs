use crate::install_policy::load_install_policy;
use crate::operation_coordinator::{
    check_skill_updates, initialize_managed_database, run_exclusive_operation,
    run_readonly_operation,
};
use crate::project_inspection::inspect_project_skills;
use crate::skill_adoption;
use crate::skill_inventory::{collect_known_skill_paths, scan_all_assistants};
use crate::skill_library::scan_library_skills;
use crate::skill_orchestration::{apply_manifest_with_plan, preview_manifest};
use crate::skillmate_manifest::{read_skillmate_manifest, SkillMateManifestPreview};
use crate::{
    build_install_request_preview, install_skill_exclusive, InstallPreviewRequest,
    InstallSkillRequest,
};
use serde::Serialize;
use std::path::PathBuf;

const USAGE: &str = "SkillMate CLI\n\n用法:\n  skillmate-cli scan [--json]\n  skillmate-cli list [--json]\n  skillmate-cli project <项目目录> [--json]\n  skillmate-cli doctor [--json]\n  skillmate-cli add <来源> [--source git|local|claude_marketplace] [--skill <相对路径>]... [--plan-token <令牌>] [--json]\n  skillmate-cli enable <统一库Skill目录> --assistant <Agent> [--project <项目目录>] [--plan-token <令牌>] [--json]\n  skillmate-cli adopt <Skill目录> --assistant <Agent> [--project <项目目录>] [--plan-token <令牌>] [--json]\n  skillmate-cli maintain [--json]\n  skillmate-cli library [--set <绝对路径>] [--json]\n  skillmate-cli agent-skill [--install <目录>]\n  skillmate-cli plan <skillmate.toml> [--json]\n  skillmate-cli verify <skillmate.toml> [--json]\n  skillmate-cli apply <skillmate.toml> --plan-token <令牌> [--json]\n\nadd、enable、adopt 和 apply 不带 --plan-token 时只输出计划。确认后使用计划令牌重新执行。\ndoctor 只读自检：退出码 0 健康、1 有告警、2 有错误。";

#[derive(Debug)]
struct CommandOptions {
    positional: Vec<String>,
    json: bool,
    plan_token: Option<String>,
    source: Option<String>,
    assistant: Option<String>,
    project: Option<String>,
    set: Option<String>,
    install: Option<String>,
    selected_skills: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyOutput {
    success: bool,
    installed: usize,
    kept: usize,
    removed: usize,
    warnings: Vec<String>,
    message: String,
}

/// 返回进程退出码：正常命令恒为 0，doctor 按健康度返回 0/1/2；
/// Err 由入口统一映射为退出码 1。
pub fn run(args: Vec<String>) -> Result<i32, String> {
    let Some(command) = args.first().map(String::as_str) else {
        println!("{USAGE}");
        return Ok(0);
    };
    if matches!(command, "help" | "--help" | "-h") {
        println!("{USAGE}");
        return Ok(0);
    }
    let options = parse_options(&args[1..])?;
    // doctor 全程只读：不走 initialize_database 的恢复、迁移与安装清理，
    // 否则会在生成报告前先抹掉正要诊断的断链与登记。
    if command == "doctor" {
        return run_doctor(options);
    }
    initialize_database()?;
    match command {
        "scan" => run_scan(options),
        "list" => run_list(options),
        "project" => run_project(options),
        "add" => run_add(options),
        "enable" => run_enable(options),
        "adopt" => run_adopt(options),
        "maintain" => run_maintain(options),
        "library" => run_library(options),
        "agent-skill" => run_agent_skill(options),
        "plan" => run_plan(options, false),
        "verify" => run_plan(options, true),
        "apply" => run_apply(options),
        _ => Err(format!("未知命令: {command}\n\n{USAGE}")),
    }
    .map(|_| 0)
}

fn parse_options(args: &[String]) -> Result<CommandOptions, String> {
    let mut positional = Vec::new();
    let mut json = false;
    let mut plan_token = None;
    let mut source = None;
    let mut assistant = None;
    let mut project = None;
    let mut set = None;
    let mut install = None;
    let mut selected_skills = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json = true,
            "--plan-token" => {
                index += 1;
                let value = args
                    .get(index)
                    .map(|value| value.trim())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "--plan-token 缺少令牌值".to_string())?;
                plan_token = Some(value.to_string());
            }
            "--source" => source = Some(option_value(args, &mut index, "--source")?),
            "--assistant" => assistant = Some(option_value(args, &mut index, "--assistant")?),
            "--project" => project = Some(option_value(args, &mut index, "--project")?),
            "--set" => set = Some(option_value(args, &mut index, "--set")?),
            "--install" => install = Some(option_value(args, &mut index, "--install")?),
            "--skill" => selected_skills.push(option_value(args, &mut index, "--skill")?),
            option if option.starts_with('-') => return Err(format!("不支持的参数: {option}")),
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    Ok(CommandOptions {
        positional,
        json,
        plan_token,
        source,
        assistant,
        project,
        set,
        install,
        selected_skills,
    })
}

fn option_value(args: &[String], index: &mut usize, name: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{name} 缺少参数值"))
}

fn initialize_database() -> Result<(), String> {
    let (_, report) = initialize_managed_database()?;
    for warning in report.warnings {
        eprintln!("{warning}");
    }
    Ok(())
}

fn run_scan(options: CommandOptions) -> Result<(), String> {
    ensure_positional_count(&options, 0, "scan")?;
    let assistants = run_exclusive_operation(scan_all_assistants)?;
    if options.json {
        print_json(&assistants)
    } else {
        let assistant_count = assistants.iter().filter(|item| item.exists).count();
        let skill_count = assistants
            .iter()
            .map(|item| item.skills.len())
            .sum::<usize>();
        let diagnostic_count = assistants
            .iter()
            .map(|item| item.diagnostics.len())
            .sum::<usize>();
        println!("发现 {skill_count} 个 Skill，覆盖 {assistant_count} 个助手");
        for assistant in assistants
            .iter()
            .filter(|item| item.exists || !item.skills.is_empty())
        {
            println!(
                "- {}: {} 个 Skill，{} 个扫描提示",
                assistant.name,
                assistant.skills.len(),
                assistant.diagnostics.len()
            );
        }
        if diagnostic_count > 0 {
            println!("共 {diagnostic_count} 个扫描提示，可使用 --json 查看详情");
        }
        Ok(())
    }
}

fn run_list(options: CommandOptions) -> Result<(), String> {
    ensure_positional_count(&options, 0, "list")?;
    let skills = run_exclusive_operation(scan_library_skills)?;
    if options.json {
        print_json(&skills)
    } else {
        println!("统一库中有 {} 个 Skill", skills.len());
        for skill in skills {
            println!("- {}  {}", skill.inventory.name, skill.inventory.path);
        }
        Ok(())
    }
}

fn run_project(options: CommandOptions) -> Result<(), String> {
    ensure_positional_count(&options, 1, "project")?;
    let path = crate::app_core::expand_path(&options.positional[0]);
    let inspection = run_exclusive_operation(|db| inspect_project_skills(db, &path))?;
    if options.json {
        print_json(&inspection)
    } else {
        println!("项目：{}", inspection.project_path);
        for assistant in inspection.assistants {
            println!(
                "- {}: {} 个生效（项目 {} / 全局 {}），{} 个同名被覆盖",
                assistant.name,
                assistant.skills.len(),
                assistant.project_count,
                assistant.global_count,
                assistant.shadowed_count
            );
        }
        Ok(())
    }
}

fn run_add(options: CommandOptions) -> Result<(), String> {
    ensure_positional_count(&options, 1, "add")?;
    let package = options.positional[0].clone();
    let source = options.source.clone().unwrap_or_else(|| {
        if crate::skill_install_source::is_git_install_source(&package) {
            "git".to_string()
        } else {
            "local".to_string()
        }
    });
    if !matches!(source.as_str(), "git" | "local" | "claude_marketplace") {
        return Err("add 的 --source 仅支持 git、local 或 claude_marketplace".to_string());
    }
    run_install_command(
        options,
        package,
        source,
        "SkillMate".to_string(),
        "library".to_string(),
        None,
    )
}

fn run_enable(options: CommandOptions) -> Result<(), String> {
    ensure_positional_count(&options, 1, "enable")?;
    let package = crate::app_core::expand_path(&options.positional[0])
        .to_string_lossy()
        .to_string();
    let assistant = options
        .assistant
        .clone()
        .ok_or_else(|| "enable 必须提供 --assistant".to_string())?;
    let project = options.project.clone();
    let mode = if project.is_some() { "symlink" } else { "copy" }.to_string();
    run_install_command(
        options,
        package,
        "local".to_string(),
        assistant,
        mode,
        project,
    )
}

fn run_install_command(
    options: CommandOptions,
    package: String,
    source: String,
    assistant: String,
    mode: String,
    project: Option<String>,
) -> Result<(), String> {
    let selected_skills =
        (!options.selected_skills.is_empty()).then_some(options.selected_skills.clone());
    let preview = run_exclusive_operation(|db| {
        let policy = load_install_policy(db);
        Ok(build_install_request_preview(
            db,
            InstallPreviewRequest {
                package: &package,
                source: &source,
                assistant_name: &assistant,
                mode: &mode,
                project_path: project.as_deref(),
                selected_skill_paths: selected_skills.as_deref(),
                preferred_skill_id: None,
            },
            policy.as_ref().map_err(Clone::clone),
        ))
    })?;
    if options.plan_token.is_none() {
        return print_install_preview(&preview, options.json);
    }
    let result = run_exclusive_operation(|db| {
        Ok(install_skill_exclusive(
            db,
            InstallSkillRequest {
                package,
                source,
                assistant_name: assistant,
                install_mode: Some(mode),
                project_path: project,
                selected_skill_paths: selected_skills,
                preferred_skill_id: None,
                plan_token: options.plan_token,
            },
        ))
    })?;
    if options.json {
        print_json(&result)
    } else if result.success {
        println!("{}", result.message);
        Ok(())
    } else {
        Err(format!("{}：{}", result.message, result.output))
    }
}

fn run_adopt(options: CommandOptions) -> Result<(), String> {
    ensure_positional_count(&options, 1, "adopt")?;
    let assistant = options
        .assistant
        .clone()
        .ok_or_else(|| "adopt 必须提供 --assistant".to_string())?;
    let path = options.positional[0].clone();
    if options.plan_token.is_none() {
        let preview = run_exclusive_operation(|db| {
            Ok(skill_adoption::preview_adopt_skill(
                db,
                &path,
                &assistant,
                options.project.as_deref(),
            ))
        })?;
        return print_install_preview(&preview, options.json);
    }
    let result = run_exclusive_operation(|db| {
        Ok(skill_adoption::adopt_skill(
            db,
            path,
            assistant,
            options.project,
            options.plan_token,
        ))
    })?;
    if options.json {
        print_json(&result)
    } else if result.success {
        println!("{}", result.message);
        Ok(())
    } else {
        Err(format!("{}：{}", result.message, result.output))
    }
}

fn run_maintain(options: CommandOptions) -> Result<(), String> {
    ensure_positional_count(&options, 0, "maintain")?;
    let paths = run_exclusive_operation(collect_known_skill_paths)?
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let results = check_skill_updates(&paths, true)?;
    if options.json {
        let output = results
            .into_iter()
            .map(|(path, result)| (path, result.map(|info| info.has_update)))
            .collect::<Vec<_>>();
        print_json(&output)
    } else {
        let mut updates = 0;
        let mut failures = 0;
        for (_, result) in results {
            match result {
                Ok(info) => updates += usize::from(info.has_update),
                Err(_) => failures += 1,
            }
        }
        println!("发现 {updates} 个可用更新，{failures} 个检查失败");
        Ok(())
    }
}

/// 只读健康自检：与写操作共用并发锁，但不做事务恢复、指纹迁移与
/// 安装清理等维护写；退出码 0 健康 / 1 有告警 / 2 有错误。
fn run_doctor(options: CommandOptions) -> Result<i32, String> {
    run_doctor_with(options, || {
        run_readonly_operation(crate::skill_doctor::collect_doctor_report)
    })
}

fn run_doctor_with<F>(options: CommandOptions, collect: F) -> Result<i32, String>
where
    F: FnOnce() -> Result<crate::skill_doctor::DoctorReport, String>,
{
    ensure_positional_count(&options, 0, "doctor")?;
    let report = collect()?;
    if options.json {
        print_json(&report)?;
    } else {
        println!(
            "体检：{} 个平台、{} 个启用条目、{} 个库内 Skill；链接能力 {}；{} 个错误、{} 个告警",
            report.summary.assistants,
            report.summary.skills,
            report.summary.library_skills,
            report.summary.link_mode,
            report.summary.errors,
            report.summary.warnings,
        );
        for finding in &report.findings {
            let severity = if finding.severity == "error" {
                "错误"
            } else {
                "告警"
            };
            let where_ = if finding.path.is_empty() {
                "-".to_string()
            } else {
                finding.path.clone()
            };
            println!(
                "- [{severity}] {} {}：{} → {}",
                finding.category, where_, finding.message, finding.suggestion
            );
        }
        if report.findings.is_empty() {
            println!("未发现问题");
        }
    }
    Ok(report.exit_code())
}

fn run_library(options: CommandOptions) -> Result<(), String> {
    ensure_positional_count(&options, 0, "library")?;
    let settings = if let Some(path) = options.set.as_deref() {
        run_exclusive_operation(|db| crate::library_config::set_library_root(db, path))?
    } else {
        crate::library_config::get_library_settings()?
    };
    if options.json {
        print_json(&settings)
    } else {
        println!("{}", settings.path);
        Ok(())
    }
}

fn run_agent_skill(options: CommandOptions) -> Result<(), String> {
    ensure_positional_count(&options, 0, "agent-skill")?;
    const CONTENT: &str = include_str!("../resources/skills/skillmate/SKILL.md");
    if let Some(directory) = options.install {
        let target = crate::app_core::expand_path(&directory).join("skillmate/SKILL.md");
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(format!(
                "目标 Agent Skill 已存在，不会覆盖: {}",
                target.to_string_lossy()
            ));
        }
        crate::app_core::atomic_write(&target, CONTENT.as_bytes())?;
        println!("已安装 SkillMate Agent Skill：{}", target.to_string_lossy());
    } else {
        print!("{CONTENT}");
    }
    Ok(())
}

fn print_install_preview(
    preview: &crate::skill_install::InstallPreview,
    json: bool,
) -> Result<(), String> {
    if json {
        print_json(preview)
    } else {
        println!("{}", preview.message);
        for action in &preview.target_actions {
            println!(
                "- [{}] {} -> {}",
                action.action, action.source, action.target
            );
        }
        for conflict in &preview.conflicts {
            println!("- [冲突] {}: {}", conflict.target, conflict.reason);
        }
        println!("plan-token: {}", preview.plan_token);
        Ok(())
    }
}

fn run_plan(options: CommandOptions, verify_only: bool) -> Result<(), String> {
    ensure_positional_count(&options, 1, if verify_only { "verify" } else { "plan" })?;
    if options.plan_token.is_some() {
        return Err("plan 和 verify 不接受 --plan-token".to_string());
    }
    let manifest_path = PathBuf::from(&options.positional[0]);
    let manifest = read_skillmate_manifest(&manifest_path)?;
    let preview = run_exclusive_operation(|db| preview_manifest(db, &manifest))?;
    if options.json {
        print_json(&preview)?;
    } else {
        print_manifest_preview(&preview);
    }
    if verify_only && !preview.can_apply {
        return Err(format!(
            "验证失败：{} 个格式问题，{} 个冲突",
            preview.validation_issues.len(),
            preview.conflicts.len()
        ));
    }
    Ok(())
}

fn run_apply(options: CommandOptions) -> Result<(), String> {
    ensure_positional_count(&options, 1, "apply")?;
    let token = options
        .plan_token
        .as_deref()
        .ok_or_else(|| "apply 必须提供 plan 命令生成的 --plan-token".to_string())?;
    let manifest = read_skillmate_manifest(&options.positional[0])?;
    let summary =
        run_exclusive_operation(|db| apply_manifest_with_plan(db, &manifest, Some(token)))?;
    let output = ApplyOutput {
        success: true,
        installed: summary.installed,
        kept: summary.kept,
        removed: summary.removed,
        warnings: summary.warnings.clone(),
        message: summary.message("skillmate.toml"),
    };
    if options.json {
        print_json(&output)
    } else {
        println!("{}", output.message);
        Ok(())
    }
}

fn ensure_positional_count(
    options: &CommandOptions,
    expected: usize,
    command: &str,
) -> Result<(), String> {
    if options.positional.len() == expected {
        Ok(())
    } else {
        Err(format!("{command} 命令参数数量不正确\n\n{USAGE}"))
    }
}

fn print_manifest_preview(preview: &SkillMateManifestPreview) {
    println!(
        "计划{}应用：{} 个动作，{} 个格式问题，{} 个冲突",
        if preview.can_apply {
            "可以"
        } else {
            "不能"
        },
        preview.actions.len(),
        preview.validation_issues.len(),
        preview.conflicts.len()
    );
    for action in &preview.actions {
        println!(
            "- [{}] {} / {}: {}",
            action.kind, action.assistant, action.target_name, action.message
        );
    }
    for issue in &preview.validation_issues {
        println!(
            "- [格式问题] #{} {}: {}",
            issue.index + 1,
            issue.code,
            issue.message
        );
    }
    for conflict in &preview.conflicts {
        println!(
            "- [冲突] {} / {}: {}",
            conflict.assistant, conflict.source, conflict.reason
        );
    }
    println!("plan-token: {}", preview.plan_token);
}

fn print_json(value: &impl Serialize) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_and_plan_token_in_any_order() {
        let options = parse_options(&[
            "manifest.toml".to_string(),
            "--json".to_string(),
            "--plan-token".to_string(),
            "token".to_string(),
        ])
        .unwrap();
        assert!(options.json);
        assert_eq!(options.positional, vec!["manifest.toml"]);
        assert_eq!(options.plan_token.as_deref(), Some("token"));
    }

    #[test]
    fn rejects_unknown_option() {
        assert!(parse_options(&["--force".to_string()]).is_err());
    }

    #[test]
    fn parses_extended_command_options() {
        let options = parse_options(&[
            "writer@official".to_string(),
            "--source".to_string(),
            "claude_marketplace".to_string(),
            "--assistant".to_string(),
            "Claude Code".to_string(),
            "--project".to_string(),
            "/tmp/project".to_string(),
        ])
        .unwrap();

        assert_eq!(options.positional, vec!["writer@official"]);
        assert_eq!(options.source.as_deref(), Some("claude_marketplace"));
        assert_eq!(options.assistant.as_deref(), Some("Claude Code"));
        assert_eq!(options.project.as_deref(), Some("/tmp/project"));
    }

    #[test]
    fn parses_repeated_skill_selection() {
        let options = parse_options(&[
            "example/skills".to_string(),
            "--skill".to_string(),
            "skills/writer".to_string(),
            "--skill".to_string(),
            "skills/reviewer".to_string(),
        ])
        .unwrap();

        assert_eq!(
            options.selected_skills,
            vec!["skills/writer", "skills/reviewer"]
        );
    }

    struct DoctorFixture {
        root: PathBuf,
        deployment_root: PathBuf,
        library_path: PathBuf,
        deployment_path: PathBuf,
        db_file: PathBuf,
    }

    /// 登记完好的库 + 部署链接，删掉任意一边后即 doctor 要诊断的原始坏态。
    fn doctor_readonly_fixture() -> Option<DoctorFixture> {
        use crate::managed_installation::record_managed_root;
        use crate::managed_state::{mark_managed_skill, LIBRARY_OWNER_NAME};

        let root = std::env::temp_dir().join(format!(
            "skillmate-doctor-readonly-{}-{}",
            std::process::id(),
            crate::app_core::generate_id()
        ));
        let library_root = root.join("a-library");
        let deployment_root = root.join("z-deployments");
        let library_path = library_root.join("writer");
        let deployment_path = deployment_root.join("writer");
        let db_file = root.join("data.db");
        std::fs::create_dir_all(&library_path).unwrap();
        std::fs::create_dir_all(&deployment_root).unwrap();
        std::fs::write(library_path.join("SKILL.md"), "writer").unwrap();
        if !crate::app_core::create_test_directory_symlink_or_skip(&library_path, &deployment_path)
        {
            let _ = std::fs::remove_dir_all(root);
            return None;
        }
        {
            let db = rusqlite::Connection::open(&db_file).unwrap();
            db.execute_batch(
                "CREATE TABLE managed_installations (
                    skill_path TEXT PRIMARY KEY, assistant TEXT NOT NULL, source TEXT NOT NULL,
                    source_kind TEXT NOT NULL, target_name TEXT NOT NULL, scope TEXT NOT NULL,
                    install_mode TEXT NOT NULL, project_path TEXT, tracking_ref TEXT, subdir TEXT,
                    resolved_ref TEXT, content_hash TEXT, installed_at TEXT NOT NULL
                );
                CREATE TABLE skill_origin_meta (
                    skill_path TEXT PRIMARY KEY, origin_kind TEXT NOT NULL, origin_locator TEXT NOT NULL,
                    resolved_locator TEXT NOT NULL, tracking_ref TEXT NOT NULL, installed_ref TEXT NOT NULL,
                    latest_ref TEXT NOT NULL, sync_state TEXT NOT NULL, sync_message TEXT NOT NULL,
                    lag_count INTEGER NOT NULL, last_probe_at INTEGER, last_sync_at INTEGER,
                    managed_by_app INTEGER NOT NULL
                );
                CREATE TABLE skill_tags (
                    skill_path TEXT PRIMARY KEY, tags TEXT, tags_json TEXT NOT NULL DEFAULT '[]'
                );
                CREATE TABLE managed_roots (
                    root_path TEXT PRIMARY KEY, scope TEXT NOT NULL,
                    project_path TEXT, updated_at TEXT NOT NULL
                );
                CREATE TABLE library_skills (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, library_path TEXT NOT NULL UNIQUE,
                    source TEXT NOT NULL, source_kind TEXT NOT NULL, resolved_ref TEXT,
                    content_hash TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
                );
                CREATE TABLE skill_deployments (
                    target_path TEXT PRIMARY KEY, skill_id TEXT NOT NULL, library_path TEXT NOT NULL,
                    assistant TEXT NOT NULL, scope TEXT NOT NULL, project_path TEXT,
                    deploy_mode TEXT NOT NULL, deployed_at TEXT NOT NULL,
                    FOREIGN KEY(skill_id) REFERENCES library_skills(id) ON DELETE RESTRICT
                );",
            )
            .unwrap();
            mark_managed_skill(
                &library_root,
                LIBRARY_OWNER_NAME,
                &library_path,
                "local:/tmp/writer",
            )
            .unwrap();
            mark_managed_skill(
                &deployment_root,
                "Codex",
                &deployment_path,
                &format!("symlink:{}", library_path.to_string_lossy()),
            )
            .unwrap();
            record_managed_root(&db, &library_root, "library", None).unwrap();
            record_managed_root(&db, &deployment_root, "global", None).unwrap();
            db.execute(
                "INSERT INTO library_skills VALUES (?, 'writer', ?, 'local:/tmp/writer', 'local',
                 NULL, 'hash', 'now', 'now')",
                rusqlite::params!["skill-1", library_path.to_string_lossy().to_string()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO skill_deployments VALUES (?, 'skill-1', ?, 'Codex', 'global', NULL,
                 'symlink', 'now')",
                rusqlite::params![
                    deployment_path.to_string_lossy().to_string(),
                    library_path.to_string_lossy().to_string()
                ],
            )
            .unwrap();
        }
        Some(DoctorFixture {
            root,
            deployment_root,
            library_path,
            deployment_path,
            db_file,
        })
    }

    /// 走与线上一致的只读组装路径（锁与库来源注入为测试替身），返回退出码与 findings。
    fn run_doctor_collecting(db_file: &std::path::Path) -> (i32, Vec<(String, String)>) {
        let db_path = db_file.to_path_buf();
        let seen = std::cell::RefCell::new(Vec::<(String, String)>::new());
        let code = run_doctor_with(
            CommandOptions {
                positional: vec![],
                json: true,
                plan_token: None,
                source: None,
                assistant: None,
                project: None,
                set: None,
                install: None,
                selected_skills: vec![],
            },
            || {
                let report = crate::operation_coordinator::run_readonly_operation_with(
                    || Ok(()),
                    move || crate::database::open_readonly_db_connection_at(&db_path),
                    crate::skill_doctor::collect_doctor_report,
                )?;
                *seen.borrow_mut() = report
                    .findings
                    .iter()
                    .map(|finding| (finding.category.clone(), finding.path.clone()))
                    .collect();
                Ok(report)
            },
        )
        .unwrap();
        (code, seen.into_inner())
    }

    /// 数据库文件字节、登记表行数与 sidecar 全部原样：doctor 不做恢复、迁移与清理。
    fn assert_doctor_left_no_trace(fixture: &DoctorFixture, db_bytes_before: &[u8]) {
        use crate::managed_installation::list_managed_installations;
        use crate::managed_state::read_managed_state;

        assert_eq!(
            std::fs::read(&fixture.db_file).unwrap(),
            db_bytes_before,
            "data.db 内容被改动"
        );
        let verify = rusqlite::Connection::open(&fixture.db_file).unwrap();
        assert_eq!(list_managed_installations(&verify).unwrap().len(), 2);
        for (table, expected) in [("library_skills", 1i64), ("skill_deployments", 1)] {
            assert_eq!(
                verify
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                expected
            );
        }
        assert_eq!(
            read_managed_state(&fixture.deployment_root)
                .unwrap()
                .managed_skills
                .len(),
            1
        );
    }

    #[test]
    fn doctor_reports_broken_state_without_mutating_registration_or_files() {
        let Some(fixture) = doctor_readonly_fixture() else {
            return;
        };
        // 用户删掉库主副本后：部署链接悬挂、登记保持原样——正是 doctor 要报告的状态。
        std::fs::remove_dir_all(&fixture.library_path).unwrap();
        let db_bytes_before = std::fs::read(&fixture.db_file).unwrap();

        let (code, findings) = run_doctor_collecting(&fixture.db_file);

        assert_eq!(code, 2);
        for broken in [&fixture.deployment_path, &fixture.library_path] {
            assert!(
                findings.iter().any(|(category, path)| {
                    category == "broken_deployment" && path == &broken.to_string_lossy()
                }),
                "缺少 {} 的断链 finding",
                broken.to_string_lossy()
            );
        }
        assert_eq!(
            std::fs::read_link(&fixture.deployment_path).unwrap(),
            fixture.library_path
        );
        assert_doctor_left_no_trace(&fixture, &db_bytes_before);
        let _ = std::fs::remove_dir_all(fixture.root);
    }

    #[test]
    fn doctor_reports_removed_deployment_target() {
        let Some(fixture) = doctor_readonly_fixture() else {
            return;
        };
        // 只删启用链接、主副本完好：路径整个消失，扫描按 exists/symlink 过滤收不到它，
        // 必须靠原始登记核验出来。Windows 上目录链接须用 remove_dir，走跨平台删除工具。
        crate::app_core::remove_path(&fixture.deployment_path).unwrap();
        let db_bytes_before = std::fs::read(&fixture.db_file).unwrap();

        let (code, findings) = run_doctor_collecting(&fixture.db_file);

        assert_eq!(code, 2);
        assert!(findings.iter().any(|(category, path)| {
            category == "broken_deployment" && path == &fixture.deployment_path.to_string_lossy()
        }));
        assert!(!findings.iter().any(|(category, path)| {
            category == "broken_deployment" && path == &fixture.library_path.to_string_lossy()
        }));
        assert!(fixture.library_path.join("SKILL.md").is_file());
        assert_doctor_left_no_trace(&fixture, &db_bytes_before);
        let _ = std::fs::remove_dir_all(fixture.root);
    }

    #[test]
    fn readonly_entry_points_leave_no_files_on_first_run() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-doctor-first-run-{}-{}",
            std::process::id(),
            crate::app_core::generate_id()
        ));
        let lock =
            crate::operation_coordinator::acquire_readonly_lock_at(&root.join("operations.lock"))
                .unwrap();
        assert!(lock.is_none());
        let db = crate::database::open_readonly_db_connection_at(&root.join("data.db")).unwrap();
        db.query_row("SELECT COUNT(*) FROM managed_installations", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
        drop(db);
        assert!(!root.exists(), "首次运行不应在磁盘留下任何文件");
    }

    #[test]
    fn agent_skill_install_refuses_to_overwrite_existing_file() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-agent-skill-{}",
            crate::app_core::generate_id()
        ));
        let options = || CommandOptions {
            positional: vec![],
            json: false,
            plan_token: None,
            source: None,
            assistant: None,
            project: None,
            set: None,
            install: Some(root.to_string_lossy().to_string()),
            selected_skills: vec![],
        };

        run_agent_skill(options()).unwrap();
        let error = run_agent_skill(options()).unwrap_err();

        assert!(error.contains("已存在"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn agent_skill_install_refuses_to_overwrite_a_broken_symlink() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-agent-skill-link-{}",
            crate::app_core::generate_id()
        ));
        let target = root.join("skillmate/SKILL.md");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(root.join("missing.md"), &target).unwrap();
        let options = CommandOptions {
            positional: vec![],
            json: false,
            plan_token: None,
            source: None,
            assistant: None,
            project: None,
            set: None,
            install: Some(root.to_string_lossy().to_string()),
            selected_skills: vec![],
        };

        let error = run_agent_skill(options).unwrap_err();

        assert!(error.contains("已存在"));
        assert!(std::fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink());
        std::fs::remove_dir_all(root).unwrap();
    }
}
