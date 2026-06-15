export const SUPPORTED_INSTALL_SOURCES = ["git", "local"];

const STRUCTURE_STATUS_LABELS = {
  complete: "完整",
  partial: "部分",
  nonstandard: "非标准",
};

const STRUCTURE_STATUS_TONES = {
  complete: "success",
  partial: "warn",
  nonstandard: "error",
};

const STRUCTURE_WARNING_LABELS = {
  path_missing: "路径不存在",
  missing_entry_document: "缺少入口文档",
  missing_skill_md: "缺少 SKILL.md",
  missing_support_dirs: "缺少资源目录",
  empty_skill_md: "SKILL.md 内容为空",
  frontmatter_invalid: "frontmatter 无效",
  structure_preview_failed: "结构预览失败",
  target_exists: "目标目录已存在",
  archive_unsupported: "压缩包安装暂未支持",
  empty_input: "输入为空",
  unrecognized_input: "规则无法识别",
  assistant_bundle_detected: "识别到助手包结构",
  unsafe_paths: "存在异常路径",
};

const INSTALL_SOURCE_LABELS = {
  git: "Git 仓库",
  git_subdir: "Git 仓库子目录",
  local_dir: "本地目录",
  local_symlink: "项目软连接",
  archive: "压缩包",
  unknown: "未知来源",
};

const INSTALL_CONFIDENCE_LABELS = {
  high: "高置信度",
  medium: "中置信度",
  low: "低置信度",
};

const PACKAGE_KIND_LABELS = {
  single_skill: "单 Skill",
  multi_skill: "多 Skill",
  assistant_bundle: "助手包",
  unknown: "未知包",
};

const PREVIEW_ACTION_LABELS = {
  copy: "复制",
  replace: "替换",
  skip: "跳过",
  backup: "备份",
  symlink: "软连接",
};

const VALIDATION_STATUS_LABELS = {
  pass: "通过",
  warning: "提醒",
  fail: "失败",
};

export function buildInstallCommandPreview({ source, assistantName, installMode, projectPath }) {
  if (installMode === "symlink") {
    return `将本地目录软连接到 ${projectPath || "项目"} 的 ${assistantName || "目标"} Skills 目录`;
  }
  if (source === "local") {
    return `复制本地目录到 ${assistantName || "目标"} Skills 目录`;
  }
  return `克隆 Git 仓库到 ${assistantName || "目标"} Skills 目录`;
}

export function buildInstallPreviewToken({ packageValue, source, assistantName, installMode, projectPath }) {
  return {
    packageValue: (packageValue || "").trim(),
    source: source || "",
    assistantName: assistantName || "",
    installMode: installMode || "copy",
    projectPath: (projectPath || "").trim(),
  };
}

export function isInstallPreviewCurrent({ previewToken, packageValue, source, assistantName, installMode, projectPath }) {
  if (!previewToken) {
    return false;
  }
  const current = buildInstallPreviewToken({ packageValue, source, assistantName, installMode, projectPath });
  return previewToken.packageValue === current.packageValue
    && previewToken.source === current.source
    && previewToken.assistantName === current.assistantName
    && previewToken.installMode === current.installMode
    && previewToken.projectPath === current.projectPath;
}

export function shouldShowProjectLinkOption({ source, detection }) {
  const detectedSource = detection?.normalized_source || detection?.source_kind || source;
  return detectedSource === "local" || source === "local";
}

export function shouldShowInstallAdvancedOptions({ advancedOpen, detection }) {
  if (advancedOpen) {
    return true;
  }
  if (!detection) {
    return false;
  }
  const warnings = Array.isArray(detection.warnings) ? detection.warnings : [];
  return detection.confidence === "low"
    || detection.needs_model
    || warnings.includes("unrecognized_input")
    || !SUPPORTED_INSTALL_SOURCES.includes(detection.normalized_source);
}

export function buildInstallPrimaryAction({
  packageValue,
  preview,
  previewCurrent,
  previewingInstall,
  loading,
}) {
  const hasInput = Boolean((packageValue || "").trim());
  const canApply = Boolean(preview?.can_apply ?? preview?.can_install);
  const disabled = !hasInput || previewingInstall || loading;
  if (previewingInstall) {
    return { action: "preview", label: "检查中...", icon: "preview", disabled: true };
  }
  if (preview && previewCurrent && canApply) {
    return { action: "install", label: "安装", icon: "plus", disabled };
  }
  if (preview && previewCurrent && !canApply) {
    return { action: "preview", label: "重新检查", icon: "preview", disabled };
  }
  return { action: "preview", label: "检查结构", icon: "preview", disabled };
}

export function normalizeSkillStructure(skill) {
  const status = skill?.structure_status || "nonstandard";
  return {
    status,
    features: Array.isArray(skill?.structure_features) ? skill.structure_features : [],
    warnings: Array.isArray(skill?.structure_warnings) ? skill.structure_warnings : [],
    manifestTitle: skill?.manifest_title || "",
    manifestDescription: skill?.manifest_description || "",
  };
}

export function getStructureStatusLabel(status) {
  return STRUCTURE_STATUS_LABELS[status] || STRUCTURE_STATUS_LABELS.nonstandard;
}

export function getStructureStatusTone(status) {
  return STRUCTURE_STATUS_TONES[status] || STRUCTURE_STATUS_TONES.nonstandard;
}

export function buildStructureWarningSummary(skill) {
  const { warnings } = normalizeSkillStructure(skill);
  if (warnings.length === 0) {
    return "结构未发现明显问题";
  }
  return warnings.map((warning) => STRUCTURE_WARNING_LABELS[warning] || warning).join("、");
}

export function getInstallSourceLabel(sourceKind) {
  return INSTALL_SOURCE_LABELS[sourceKind] || sourceKind || INSTALL_SOURCE_LABELS.unknown;
}

export function getInstallConfidenceLabel(confidence) {
  return INSTALL_CONFIDENCE_LABELS[confidence] || confidence || INSTALL_CONFIDENCE_LABELS.low;
}

export function buildInstallDetectionSummary(detection) {
  if (!detection) {
    return "";
  }
  const label = getInstallSourceLabel(detection.source_kind);
  const confidence = getInstallConfidenceLabel(detection.confidence);
  const parts = [`识别为${label}`, confidence];
  if (detection.reference) {
    parts.push(`引用 ${detection.reference}`);
  }
  if (detection.subdir) {
    parts.push(`子目录 ${detection.subdir}`);
  }
  if (detection.target_name) {
    parts.push(`目标 ${detection.target_name}`);
  }
  if (detection.needs_model) {
    parts.push("可用模型辅助识别");
  }
  return parts.join(" · ");
}

export function buildInstallDetectionWarningSummary(detection) {
  const warnings = Array.isArray(detection?.warnings) ? detection.warnings : [];
  if (warnings.length === 0) {
    return "";
  }
  return warnings.map((warning) => STRUCTURE_WARNING_LABELS[warning] || warning).join("、");
}

export function buildInstallDetectionView(detection) {
  if (!detection) {
    return null;
  }
  return {
    title: detection.detector === "rules" ? "本地规则" : "模型辅助",
    tone: detection.confidence === "low" ? "warn" : "success",
    summary: buildInstallDetectionSummary(detection),
    warningSummary: buildInstallDetectionWarningSummary(detection),
    sourceLabel: getInstallSourceLabel(detection.source_kind),
    confidenceLabel: getInstallConfidenceLabel(detection.confidence),
    needsModel: Boolean(detection.needs_model),
  };
}

export function getPackageKindLabel(kind) {
  return PACKAGE_KIND_LABELS[kind] || PACKAGE_KIND_LABELS.unknown;
}

export function buildPackageDetectionSummary(detection) {
  if (!detection) {
    return "";
  }
  const count = Array.isArray(detection.detected_skills) ? detection.detected_skills.length : 0;
  const parts = [getPackageKindLabel(detection.package_kind), `${count} 个 Skill`];
  if (detection.needs_model) {
    parts.push("可选模型辅助识别");
  }
  return parts.join(" · ");
}

export function buildInstallPreviewView(preview) {
  if (!preview) {
    return null;
  }
  const packageDetection = preview.package_detection || {
    package_kind: "unknown",
    detected_skills: [],
    warnings: [],
    needs_model: false,
  };
  const actions = Array.isArray(preview.target_actions) ? preview.target_actions : [];
  const conflicts = Array.isArray(preview.conflicts) ? preview.conflicts : [];
  return {
    canApply: Boolean(preview.can_apply ?? preview.can_install),
    tone: conflicts.length > 0 ? "error" : getStructureStatusTone(preview.structure_status),
    message: preview.message || "",
    packageSummary: buildPackageDetectionSummary(packageDetection),
    packageWarnings: (packageDetection.warnings || [])
      .map((warning) => STRUCTURE_WARNING_LABELS[warning] || warning)
      .join("、"),
    skills: packageDetection.detected_skills || [],
    actions: actions.map((action) => ({
      ...action,
      label: PREVIEW_ACTION_LABELS[action.action] || action.action,
    })),
    conflicts,
    needsModel: Boolean(packageDetection.needs_model),
  };
}

export function buildInstallPreviewSummary(preview) {
  if (!preview) {
    return [];
  }
  const packageDetection = preview.package_detection || {};
  const actions = Array.isArray(preview.target_actions) ? preview.target_actions : [];
  const conflicts = Array.isArray(preview.conflicts) ? preview.conflicts : [];
  const lines = [];
  lines.push(`结构：${getStructureStatusLabel(preview.structure_status)}`);
  if (preview.target_path) {
    lines.push(`目标：${preview.target_path}`);
  }
  if (actions.length > 0) {
    lines.push(`写入：${actions.length} 个动作`);
  }
  if (conflicts.length > 0) {
    lines.push(`冲突：${conflicts.length} 个`);
  }
  if (packageDetection.package_kind) {
    lines.push(buildPackageDetectionSummary(packageDetection));
  }
  if (preview.message) {
    lines.push(preview.message);
  }
  return lines;
}

export function buildValidationSummary(report) {
  if (!report) {
    return [];
  }
  return (report.checks || []).map((check) => ({
    ...check,
    label: VALIDATION_STATUS_LABELS[check.status] || check.status,
  }));
}

export function buildSkillCardView(skill) {
  const structure = normalizeSkillStructure(skill);
  return {
    title: skill?.manifest_title || skill?.name || "",
    description: buildSkillDescription(skill),
    structureLabel: getStructureStatusLabel(structure.status),
    structureTone: getStructureStatusTone(structure.status),
    warningSummary: buildStructureWarningSummary(skill),
    sourceLabel: skill?.source || "未托管",
    canSync: Boolean(skill?.can_sync),
    hasUpdate: Boolean(skill?.has_update),
  };
}

export function buildSkillDescription(skill) {
  const { manifestDescription } = normalizeSkillStructure(skill);
  if (manifestDescription) {
    return manifestDescription;
  }
  const readme = skill?.readme || "";
  return readme
    .split("\n")
    .find((line) => {
      const trimmed = line.trim();
      return trimmed && !trimmed.startsWith("#") && !trimmed.startsWith("!");
    })
    ?.slice(0, 80) || "";
}

export function buildInstallStructureSummary(result) {
  if (!result?.structure_status) {
    return "";
  }
  const label = getStructureStatusLabel(result.structure_status);
  const warnings = normalizeSkillStructure(result).warnings;
  if (warnings.length === 0) {
    return `结构${label}`;
  }
  return `结构${label}：${buildStructureWarningSummary(result)}`;
}

export function normalizeScenarioSkillPaths({ selectedPaths, manualInput, skills }) {
  if (selectedPaths.length > 0) {
    return selectedPaths;
  }

  const trimmedInput = manualInput.trim();
  if (trimmedInput) {
    return trimmedInput.split(/[,\s]+/).filter(Boolean);
  }

  return skills.slice(0, Math.min(5, skills.length)).map((skill) => skill.path);
}

export function buildGitBackupPayload({ repoPath, remoteUrl, branch }) {
  return {
    repoPath: repoPath.trim(),
    remoteUrl: remoteUrl.trim(),
    branch: branch.trim() || "main",
  };
}

export function buildImportPreviewToken({ path, mode }) {
  return {
    path: path.trim(),
    mode: mode || "merge",
  };
}

export function isImportPreviewCurrent({ previewToken, path, mode }) {
  if (!previewToken) {
    return false;
  }
  const current = buildImportPreviewToken({ path, mode });
  return previewToken.path === current.path && previewToken.mode === current.mode;
}

export function buildImportPreviewSummary(preview) {
  const lines = [];

  if (preview.replace_existing) {
    if (preview.existing_tags_to_remove > 0) {
      lines.push(`将清空现有 ${preview.existing_tags_to_remove} 个标签`);
    }
    if (preview.existing_scenarios_to_remove > 0) {
      lines.push(`将清空现有 ${preview.existing_scenarios_to_remove} 个场景`);
    }
    if (preview.existing_skill_tag_mappings_to_remove > 0) {
      lines.push(`将清空现有 ${preview.existing_skill_tag_mappings_to_remove} 条 Skill 标签映射`);
    }
  }

  if (preview.tags_to_add > 0) {
    lines.push(`将新增 ${preview.tags_to_add} 个标签`);
  }
  if (preview.tags_to_replace > 0) {
    lines.push(`将覆盖 ${preview.tags_to_replace} 个标签`);
  }
  if (preview.scenarios_to_add > 0) {
    lines.push(`将新增 ${preview.scenarios_to_add} 个场景`);
  }
  if (preview.scenarios_to_replace > 0) {
    lines.push(`将覆盖 ${preview.scenarios_to_replace} 个场景`);
  }
  if (preview.skill_tag_writes > 0) {
    lines.push(`将写入 ${preview.skill_tag_writes} 条 Skill 标签映射`);
  }

  return lines.length > 0 ? lines : ["未检测到可导入的组织数据变化"];
}

export function buildScenarioManifestPreviewSummary(preview) {
  const lines = [];

  if (preview.replace_existing && preview.existing_scenarios_to_remove > 0) {
    lines.push(`将清空现有 ${preview.existing_scenarios_to_remove} 个场景`);
  }
  if (preview.scenarios_to_add > 0) {
    lines.push(`将新增 ${preview.scenarios_to_add} 个场景`);
  }
  if (preview.scenarios_to_replace > 0) {
    lines.push(`将覆盖 ${preview.scenarios_to_replace} 个场景`);
  }
  if (Array.isArray(preview.missing_skill_refs) && preview.missing_skill_refs.length > 0) {
    lines.push(`有 ${preview.missing_skill_refs.length} 个 Skill 路径当前不存在`);
  }

  return lines.length > 0 ? lines : ["未检测到可导入的场景变化"];
}

export function buildSkillMateManifestPreviewSummary(preview) {
  const lines = [];
  const validationIssues = Array.isArray(preview?.validation_issues) ? preview.validation_issues : [];
  const actions = Array.isArray(preview?.actions) ? preview.actions : [];
  const conflicts = Array.isArray(preview?.conflicts) ? preview.conflicts : [];
  if (validationIssues.length > 0) {
    lines.push(`存在 ${validationIssues.length} 个格式问题`);
  }
  if (actions.length > 0) {
    lines.push(`将安装 ${actions.length} 条 Skill 记录`);
  }
  if (conflicts.length > 0) {
    lines.push(`存在 ${conflicts.length} 个冲突`);
  }
  actions.slice(0, 5).forEach((action) => {
    lines.push(`${action.assistant}：${action.target_name} · ${action.message}`);
  });
  conflicts.slice(0, 5).forEach((conflict) => {
    lines.push(`${conflict.assistant}：${conflict.reason}`);
  });
  validationIssues.slice(0, 5).forEach((issue) => {
    lines.push(`#${issue.index + 1}：${issue.message}`);
  });
  return lines.length > 0 ? lines : ["manifest 没有可执行动作"];
}

export function buildSkillProfilePreviewSummary(preview) {
  if (!preview) {
    return [];
  }
  const profile = preview.profile || {};
  const profileIssues = Array.isArray(preview.profile_issues) ? preview.profile_issues : [];
  const manifestPreview = preview.manifest_preview || {};
  const diff = preview.diff || {};
  const lines = [
    `${profile.name || "未命名 Profile"} · ${(profile.skills || []).length} 条 Skill 记录`,
    ...buildSkillMateManifestPreviewSummary(manifestPreview),
  ];
  if (profileIssues.length > 0) {
    lines.push(`Profile 有 ${profileIssues.length} 个格式问题`);
  }
  profileIssues.slice(0, 5).forEach((issue) => {
    lines.push(issue.message);
  });
  if (Array.isArray(diff.to_install) && diff.to_install.length > 0) {
    lines.push(`将补齐 ${diff.to_install.length} 条缺失记录`);
  }
  if (Array.isArray(diff.already_present) && diff.already_present.length > 0) {
    lines.push(`${diff.already_present.length} 条记录已存在`);
  }
  if (Array.isArray(diff.conflicts) && diff.conflicts.length > 0) {
    lines.push(`Profile diff 有 ${diff.conflicts.length} 个冲突`);
  }
  lines.push("应用 Profile 只安装缺失的受管 Skill，不会自动删除手工目录");
  return lines;
}

export function buildProjectTargetPreviewSummary(targets) {
  if (!Array.isArray(targets) || targets.length === 0) {
    return ["未识别到项目目标目录"];
  }
  return targets.map((target) => (
    `${target.assistant}：${target.target_path}${target.exists ? " · 已存在" : ""}${target.recommended ? " · 推荐" : ""}`
  ));
}

export function resolveScenarioSkills({ scenario, allSkills }) {
  const skillMap = new Map(allSkills.map((skill) => [skill.path, skill]));
  return scenario.skill_ids.map((path) => ({
    path,
    exists: skillMap.has(path),
    skill: skillMap.get(path) || null,
  }));
}

export function formatScenarioCopyText(paths) {
  return paths.join("\n");
}

export function filterSkillsByScenario({ skills, activeScenarioPaths }) {
  if (!activeScenarioPaths.length) {
    return skills;
  }

  const allowed = new Set(activeScenarioPaths);
  return skills.filter((skill) => allowed.has(skill.path));
}
