use crate::skill_inventory::scan_all_assistants;
use crate::skill_library::{directory_link_mode, read_library_skills, DirectoryLinkMode};
use crate::{AIAssistant, Skill};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeSet;

/// `doctor` 的分类与 GUI 概览的「需处理事项」同源：同一批 scan 数据按
/// 类别重新归纳，只读、不写文件也不改数据库。
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DoctorFinding {
    pub severity: String,
    pub category: String,
    pub code: String,
    pub path: String,
    pub message: String,
    pub suggestion: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSummary {
    pub assistants: usize,
    pub skills: usize,
    pub library_skills: usize,
    pub errors: usize,
    pub warnings: usize,
    pub link_mode: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub status: String,
    pub summary: DoctorSummary,
    pub findings: Vec<DoctorFinding>,
}

impl DoctorReport {
    pub fn exit_code(&self) -> i32 {
        match self.status.as_str() {
            "errors" => 2,
            "warnings" => 1,
            _ => 0,
        }
    }
}

pub fn collect_doctor_report(db: &Connection) -> Result<DoctorReport, String> {
    // 与 GUI 概览同一批只读扫描结果，保证 doctor 与桌面诊断口径一致。
    let assistants = scan_all_assistants(db)?;
    let library_skills = read_library_skills(db)?;
    let link_mode = directory_link_mode();
    Ok(build_doctor_report(&assistants, &library_skills, link_mode))
}

fn build_doctor_report(
    assistants: &[AIAssistant],
    library_skills: &[Skill],
    link_mode: DirectoryLinkMode,
) -> DoctorReport {
    let mut findings = Vec::new();
    let mut seen = BTreeSet::new();
    let mut push = |finding: DoctorFinding| {
        if seen.insert((
            finding.category.clone(),
            finding.path.clone(),
            finding.code.clone(),
        )) {
            findings.push(finding);
        }
    };

    for assistant in assistants {
        for diagnostic in &assistant.diagnostics {
            push(DoctorFinding {
                severity: "warning".into(),
                category: "scan".into(),
                code: diagnostic.code.clone(),
                path: diagnostic.path.clone(),
                message: diagnostic.message.clone(),
                suggestion: "修复目录权限或移除残留项后用 skillmate-cli scan 复查".into(),
            });
        }
        for skill in &assistant.skills {
            collect_skill_findings(skill, false, &mut push);
        }
    }
    for skill in library_skills {
        collect_skill_findings(skill, true, &mut push);
    }

    if let DirectoryLinkMode::Unavailable = link_mode {
        push(DoctorFinding {
            severity: "warning".into(),
            category: "link_capability".into(),
            code: "directory_link_unavailable".into(),
            path: String::new(),
            message: "目录符号链接与 NTFS junction 均不可用，连接启用将被阻断".into(),
            suggestion: "开启系统开发者模式或以具备创建链接权限的账户运行".into(),
        });
    }

    let errors = findings
        .iter()
        .filter(|finding| finding.severity == "error")
        .count();
    let warnings = findings.len() - errors;
    let status = if errors > 0 {
        "errors"
    } else if warnings > 0 {
        "warnings"
    } else {
        "healthy"
    };
    DoctorReport {
        status: status.into(),
        summary: DoctorSummary {
            assistants: assistants.len(),
            skills: assistants.iter().map(|item| item.skills.len()).sum(),
            library_skills: library_skills.len(),
            errors,
            warnings,
            link_mode: format!("{link_mode:?}").to_lowercase(),
        },
        findings,
    }
}

fn collect_skill_findings(skill: &Skill, in_library: bool, push: &mut impl FnMut(DoctorFinding)) {
    let warnings = &skill.structure.structure_warnings;
    let path = skill.inventory.path.clone();
    if warnings.iter().any(|code| code == "path_missing") {
        push(DoctorFinding {
            severity: "error".into(),
            category: "broken_deployment".into(),
            code: "path_missing".into(),
            path: path.clone(),
            message: if in_library {
                "库主副本路径缺失".into()
            } else {
                "启用位置路径缺失，连接或内容已断链".into()
            },
            suggestion: "用 skillmate-cli enable 重新启用，或在应用中停用该位置".into(),
        });
    }
    match skill.origin.sync_state.as_str() {
        "source_missing" => push(DoctorFinding {
            severity: "error".into(),
            category: "missing_origin".into(),
            code: "source_missing".into(),
            path: path.clone(),
            message: "登记的来源已失联".into(),
            suggestion: "确认来源路径后用 skillmate-cli add 重新添加".into(),
        }),
        "failed" => push(DoctorFinding {
            severity: "warning".into(),
            category: "missing_origin".into(),
            code: "probe_failed".into(),
            path: path.clone(),
            message: "来源检查失败，可能是暂时性问题".into(),
            suggestion: "用 skillmate-cli maintain 重新检查".into(),
        }),
        _ => {}
    }
    if skill.structure.structure_status != "complete" {
        push(DoctorFinding {
            severity: "warning".into(),
            category: "structure".into(),
            code: skill.structure.structure_status.clone(),
            path: path.clone(),
            message: format!(
                "结构状态为 {}，不符合完整 Skill 规范",
                skill.structure.structure_status
            ),
            suggestion: "修复入口文档后用 skillmate-cli scan 复查".into(),
        });
    }
    if warnings
        .iter()
        .any(|code| code == "managed_content_changed")
    {
        push(DoctorFinding {
            severity: "warning".into(),
            category: "drift".into(),
            code: "managed_content_changed".into(),
            path: path.clone(),
            message: "本地内容与受管登记状态不一致".into(),
            suggestion: "在应用更新预览中确认差异，或用 skillmate-cli verify 复核清单".into(),
        });
    }
    if warnings
        .iter()
        .any(|code| code == "suspicious_instructions")
    {
        push(DoctorFinding {
            severity: "warning".into(),
            category: "suspicious".into(),
            code: "suspicious_instructions".into(),
            path: path.clone(),
            message: "SKILL.md 正文疑似包含诱导 Agent 的指令".into(),
            suggestion: "在应用安装或更新预览中逐条审阅后决定是否标记".into(),
        });
    }
    if !skill.origin.managed_by_app {
        push(DoctorFinding {
            severity: "warning".into(),
            category: "external".into(),
            code: "externally_managed".into(),
            path,
            message: "由外部工具或手工管理，SkillMate 不跟踪其更新".into(),
            suggestion: "需要受管时用 skillmate-cli adopt 显式接管".into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        SkillInventoryFields, SkillOriginFields, SkillScanDiagnostic, SkillStructureFields,
    };

    fn skill(path: &str) -> Skill {
        Skill {
            inventory: SkillInventoryFields {
                id: path.into(),
                name: "writer".into(),
                path: path.into(),
                skill_type: "skill".into(),
                source: "test".into(),
                source_type: "local".into(),
                content_source: "local".into(),
                manager: "skillmate".into(),
                update_strategy: "manual".into(),
                size: "1 KB".into(),
                modified: "0".into(),
                tags: vec![],
                description: String::new(),
                readme: String::new(),
                version: String::new(),
                content_hash: String::new(),
            },
            origin: SkillOriginFields {
                upstream_url: String::new(),
                has_update: false,
                origin_kind: String::new(),
                origin_locator: String::new(),
                resolved_locator: String::new(),
                tracking_ref: String::new(),
                installed_ref: String::new(),
                latest_ref: String::new(),
                sync_state: "ok".into(),
                sync_message: String::new(),
                lag_count: 0,
                last_probe_at: None,
                last_sync_at: None,
                managed_by_app: true,
                can_check: true,
                can_sync: true,
                symlink_source: None,
            },
            structure: SkillStructureFields {
                structure_status: "complete".into(),
                structure_features: vec![],
                structure_warnings: vec![],
                manifest_title: None,
                manifest_description: None,
            },
        }
    }

    fn assistant(skills: Vec<Skill>) -> AIAssistant {
        AIAssistant {
            name: "Codex".into(),
            path: "/codex".into(),
            paths: vec!["/codex".into()],
            ai_type: "codex".into(),
            icon: "codex".into(),
            skills,
            diagnostics: vec![],
            exists: true,
            supports_project_skills: true,
        }
    }

    #[test]
    fn healthy_library_reports_zero_exit_code() {
        let report = build_doctor_report(
            &[assistant(vec![skill("/lib/writer")])],
            &[skill("/lib/writer")],
            DirectoryLinkMode::Symlink,
        );
        // 受管且结构完整的条目不产生任何 finding
        assert_eq!(report.status, "healthy");
        assert_eq!(report.exit_code(), 0);
    }

    #[test]
    fn broken_deployment_and_missing_origin_are_errors() {
        let mut broken = skill("/claude/writer");
        broken.structure.structure_warnings = vec!["path_missing".into()];
        let mut lost = skill("/lib/lost");
        lost.origin.sync_state = "source_missing".into();
        let report = build_doctor_report(
            &[assistant(vec![broken, lost])],
            &[],
            DirectoryLinkMode::Symlink,
        );
        assert_eq!(report.status, "errors");
        assert_eq!(report.exit_code(), 2);
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.category == "broken_deployment"));
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.category == "missing_origin"));
    }

    #[test]
    fn structure_drift_external_and_scan_issues_are_warnings() {
        let mut rough = skill("/claude/rough");
        rough.structure.structure_status = "needs_fix".into();
        rough.structure.structure_warnings = vec![
            "managed_content_changed".into(),
            "suspicious_instructions".into(),
        ];
        let mut external = skill("/claude/vendor");
        external.origin.managed_by_app = false;
        let mut host = assistant(vec![rough, external.clone()]);
        host.diagnostics.push(SkillScanDiagnostic {
            path: "/claude/blocked".into(),
            code: "unreadable".into(),
            message: "目录不可读".into(),
        });
        let report = build_doctor_report(&[host], &[external], DirectoryLinkMode::Unavailable);
        assert_eq!(report.status, "warnings");
        assert_eq!(report.exit_code(), 1);
        for category in [
            "structure",
            "drift",
            "suspicious",
            "external",
            "scan",
            "link_capability",
        ] {
            assert!(
                report
                    .findings
                    .iter()
                    .any(|finding| finding.category == category),
                "缺少 {category} 类 finding"
            );
        }
        assert!(report
            .findings
            .iter()
            .all(|finding| finding.severity == "warning"));
    }
}
