use crate::database::create_db_connection;
use crate::operation_coordinator::{run_exclusive_operation, run_startup_maintenance};
use crate::skill_inventory::scan_all_assistants;
use crate::skill_orchestration::{apply_manifest_with_plan, preview_manifest};
use crate::skillmate_manifest::{read_skillmate_manifest, SkillMateManifestPreview};
use serde::Serialize;
use std::path::PathBuf;

const USAGE: &str = "SkillMate CLI\n\n用法:\n  skillmate-cli scan [--json]\n  skillmate-cli plan <skillmate.toml> [--json]\n  skillmate-cli verify <skillmate.toml> [--json]\n  skillmate-cli apply <skillmate.toml> --plan-token <令牌> [--json]";

#[derive(Debug)]
struct CommandOptions {
    positional: Vec<String>,
    json: bool,
    plan_token: Option<String>,
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

pub fn run(args: Vec<String>) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        println!("{USAGE}");
        return Ok(());
    };
    if matches!(command, "help" | "--help" | "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let options = parse_options(&args[1..])?;
    initialize_database()?;
    match command {
        "scan" => run_scan(options),
        "plan" => run_plan(options, false),
        "verify" => run_plan(options, true),
        "apply" => run_apply(options),
        _ => Err(format!("未知命令: {command}\n\n{USAGE}")),
    }
}

fn parse_options(args: &[String]) -> Result<CommandOptions, String> {
    let mut positional = Vec::new();
    let mut json = false;
    let mut plan_token = None;
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
            option if option.starts_with('-') => return Err(format!("不支持的参数: {option}")),
            value => positional.push(value.to_string()),
        }
        index += 1;
    }
    Ok(CommandOptions {
        positional,
        json,
        plan_token,
    })
}

fn initialize_database() -> Result<(), String> {
    let db = create_db_connection()?;
    run_startup_maintenance(&db)?;
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
}
