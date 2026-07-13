export const SUPPORTED_INSTALL_SOURCES = ["git", "local"];

const STRUCTURE_STATUS_LABELS = {
  complete: "符合规范",
  partial: "需要修复",
  nonstandard: "非 Skill",
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
  legacy_skill_filename: "入口文件必须命名为 SKILL.md",
  readme_only: "README 不能替代 SKILL.md",
  missing_frontmatter: "缺少 YAML frontmatter",
  frontmatter_invalid: "YAML frontmatter 无效",
  missing_name: "缺少必填 name",
  invalid_name: "name 格式不符合规范",
  name_directory_mismatch: "name 与目录名不一致",
  missing_description: "缺少必填 description",
  invalid_description: "description 必须是字符串",
  description_too_long: "description 超过 1024 个字符",
  invalid_compatibility: "compatibility 必须是字符串",
  compatibility_too_long: "compatibility 超过 500 个字符",
  invalid_license: "license 必须是字符串",
  invalid_metadata: "metadata 必须是字符串映射",
  invalid_allowed_tools: "allowed-tools 必须是字符串",
  legacy_compatible_field: "compatible 已废弃，请使用 compatibility",
  invalid_skill_structure: "Skill 不符合 Agent Skills 规范",
  structure_preview_failed: "结构预览失败",
  entry_document_truncated: "入口文档过大，仅分析前 1 MB",
  safety_scan_incomplete: "安全扫描达到上限，结果可能不完整",
  plan_token_failed: "无法生成稳定操作计划",
  duplicate_target: "安装计划包含重复目标",
  target_exists: "目标目录已存在",
  archive_unsupported: "压缩包安装暂未支持",
  empty_input: "输入为空",
  unrecognized_input: "规则无法识别",
  assistant_bundle_detected: "识别到助手包结构",
  unsafe_paths: "存在异常路径",
  managed_state_invalid: "SkillMate 受管状态文件损坏",
  managed_content_changed: "内容已偏离安装时状态",
  skill_tags_invalid: "标签状态损坏，已回退到旧格式",
  skill_tags_unavailable: "暂时无法读取标签状态",
  contains_scripts: "包含可执行脚本",
  declares_dependencies: "包含第三方依赖清单",
  contains_symlinks: "包含软连接，复制时会跳过",
  contains_hidden_files: "包含隐藏文件",
  references_network: "可能访问网络",
  references_environment: "可能读取环境变量或凭据",
  install_policy_blocked: "安装策略已阻止",
  nonstandard_skill: "来源不是标准 Skill",
  untrusted_git_host: "Git 主机不在信任列表",
  untrusted_local_root: "本地来源不在信任根目录",
  policy_unavailable: "安装策略不可用",
};

const SECURITY_WARNING_CODES = new Set([
  "contains_scripts",
  "declares_dependencies",
  "contains_symlinks",
  "contains_hidden_files",
  "references_network",
  "references_environment",
  "safety_scan_incomplete",
]);

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
  keep: "保留",
  remove: "移除",
};

const VALIDATION_STATUS_LABELS = {
  pass: "通过",
  warning: "提醒",
  fail: "失败",
};

const APP_UPDATE_STATUS_LABELS = {
  idle: "未检查",
  checking: "检查中",
  current: "已是最新",
  available: "发现更新",
  downloading: "下载中",
  installing: "安装中",
  restarting: "正在重启",
  ready_to_restart: "等待重启",
  error: "检查失败",
};

const APP_UPDATE_STATUS_TONES = {
  idle: "muted",
  checking: "muted",
  current: "success",
  available: "warn",
  downloading: "warn",
  installing: "warn",
  restarting: "warn",
  ready_to_restart: "success",
  error: "error",
};

const APP_UPDATE_PRIMARY_ACTIONS = {
  idle: { action: "check", label: "检查更新", icon: "refresh", enabled: true },
  checking: { action: "check", label: "检查中", icon: "refresh", enabled: false },
  current: { action: "check", label: "重新检查", icon: "refresh", enabled: true },
  available: { action: "install", label: "下载并安装后重启", icon: "upload", enabled: true },
  downloading: { action: "install", label: "下载中", icon: "upload", enabled: false },
  installing: { action: "install", label: "安装中", icon: "upload", enabled: false },
  restarting: { action: "restart", label: "正在重启", icon: "refresh", enabled: false },
  ready_to_restart: { action: "restart", label: "重启应用", icon: "refresh", enabled: true },
  error: { action: "check", label: "重新检查", icon: "refresh", enabled: true },
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
  const policy = preview.install_policy || {
    mode: "off",
    allowed: true,
    findings: [],
    message: "",
  };
  return {
    canApply: Boolean(preview.can_apply ?? preview.can_install),
    tone: conflicts.length > 0 || policy.allowed === false ? "error" : getStructureStatusTone(preview.structure_status),
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
    policy: {
      mode: policy.mode || "off",
      allowed: policy.allowed !== false,
      message: policy.message || "",
      findings: (policy.findings || []).map((finding) => ({
        ...finding,
        label: STRUCTURE_WARNING_LABELS[finding.code] || finding.message || finding.code,
      })),
    },
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
  if (preview.install_policy?.message) {
    lines.push(`策略：${preview.install_policy.message}`);
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
  const isSymlink = skill?.source_type === "symlink";
  const isManaged = Boolean(skill?.managed_by_app);
  const securityWarnings = structure.warnings.filter((warning) => SECURITY_WARNING_CODES.has(warning));
  const availableIn = Array.isArray(skill?.availableIn) && skill.availableIn.length > 0
    ? skill.availableIn
    : (skill?.ai ? [{ name: skill.ai, icon: skill.aiIcon || "" }] : []);
  return {
    title: skill?.manifest_title || skill?.name || "",
    description: buildSkillDescription(skill),
    structureLabel: getStructureStatusLabel(structure.status),
    structureTone: getStructureStatusTone(structure.status),
    warningSummary: buildStructureWarningSummary(skill),
    securityWarningCount: securityWarnings.length,
    securityWarningSummary: securityWarnings
      .map((warning) => STRUCTURE_WARNING_LABELS[warning] || warning)
      .join("、"),
    hasManagedDrift: structure.warnings.includes("managed_content_changed"),
    sourceLabel: skill?.source || "未托管",
    canSync: Boolean(skill?.can_sync),
    hasUpdate: Boolean(skill?.has_update),
    canDelete: isManaged && !isSymlink,
    canUnlink: isManaged && isSymlink,
    availableIn,
    availabilityLabel: availableIn.map((assistant) => assistant.name).join("、"),
    isShared: availableIn.length > 1,
  };
}

export function buildUniqueSkillInventory(assistants) {
  const byPath = new Map();
  for (const assistant of Array.isArray(assistants) ? assistants : []) {
    for (const skill of Array.isArray(assistant?.skills) ? assistant.skills : []) {
      const path = typeof skill?.path === "string" ? skill.path : "";
      const identity = path || `${assistant?.name || "unknown"}:${skill?.id || skill?.name || "unknown"}`;
      const availability = {
        name: assistant?.name || "未知助手",
        icon: assistant?.icon || "",
      };
      const existing = byPath.get(identity);
      if (existing) {
        if (!existing.availableIn.some((item) => item.name === availability.name)) {
          existing.availableIn.push(availability);
        }
        continue;
      }
      byPath.set(identity, {
        ...skill,
        ai: availability.name,
        aiIcon: availability.icon,
        availableIn: [availability],
      });
    }
  }
  return [...byPath.values()];
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

  return [];
}

export function buildGitBackupPayload({ repoPath, remoteUrl, branch }) {
  return {
    repoPath: repoPath.trim(),
    remoteUrl: remoteUrl.trim(),
    branch: branch.trim() || "main",
  };
}

export function buildGitBackupState({ draft, saved, saving = false, syncing = false }) {
  const payload = buildGitBackupPayload(draft);
  const savedPayload = buildGitBackupPayload({
    repoPath: saved?.repoPath ?? saved?.repo_path ?? "",
    remoteUrl: saved?.remoteUrl ?? saved?.remote_url ?? "",
    branch: saved?.branch ?? "main",
  });
  const dirty = payload.repoPath !== savedPayload.repoPath
    || payload.remoteUrl !== savedPayload.remoteUrl
    || payload.branch !== savedPayload.branch;
  const configured = Boolean(savedPayload.repoPath);
  const busy = Boolean(saving || syncing);
  return {
    payload,
    dirty,
    configured,
    saving: Boolean(saving),
    syncing: Boolean(syncing),
    canSave: Boolean(payload.repoPath) && dirty && !busy,
    canSync: configured && !dirty && !busy,
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
  const installs = actions.filter((action) => !action.kind || action.kind === "install");
  const keeps = actions.filter((action) => action.kind === "keep");
  const removals = actions.filter((action) => action.kind === "remove");
  if (installs.length > 0) lines.push(`将安装 ${installs.length} 条 Skill 记录`);
  if (keeps.length > 0) lines.push(`将保留 ${keeps.length} 条来源一致的 Skill`);
  if (removals.length > 0) lines.push(`将移除 ${removals.length} 条多余的受管 Skill`);
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
  if (Array.isArray(diff.to_remove) && diff.to_remove.length > 0) {
    lines.push(`将移除 ${diff.to_remove.length} 条不在目标组合中的受管记录`);
  }
  if (Array.isArray(diff.conflicts) && diff.conflicts.length > 0) {
    lines.push(`Profile diff 有 ${diff.conflicts.length} 个冲突`);
  }
  lines.push("应用 Profile 会对齐 SkillMate 受管 Skill，不会删除手工添加的目录");
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

export function getAppUpdateStatusLabel(status) {
  return APP_UPDATE_STATUS_LABELS[status] || APP_UPDATE_STATUS_LABELS.idle;
}

export function getAppUpdateStatusTone(status) {
  return APP_UPDATE_STATUS_TONES[status] || APP_UPDATE_STATUS_TONES.idle;
}

export function formatAppUpdateDate(value) {
  if (!value) {
    return "未知";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return String(value);
  }
  return date.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function buildAppUpdateProgressText(progress) {
  if (!progress) {
    return "";
  }
  const downloaded = Number(progress.downloaded || 0);
  const total = Number(progress.contentLength || 0);
  if (total > 0) {
    const percent = Math.min(100, Math.round((downloaded / total) * 100));
    return `${percent}%`;
  }
  if (downloaded > 0) {
    return `${Math.round(downloaded / 1024)} KB`;
  }
  return "";
}

export function buildAppUpdateView(state) {
  const status = state?.status || "idle";
  const update = state?.update || null;
  const progress = state?.progress || null;
  const primaryAction = APP_UPDATE_PRIMARY_ACTIONS[status] || APP_UPDATE_PRIMARY_ACTIONS.idle;
  return {
    status,
    statusLabel: getAppUpdateStatusLabel(status),
    statusTone: getAppUpdateStatusTone(status),
    currentVersion: state?.currentVersion || update?.currentVersion || "",
    nextVersion: update?.version || "",
    dateLabel: formatAppUpdateDate(update?.date),
    releaseNotes: update?.body || "",
    progressText: buildAppUpdateProgressText(progress),
    progressPercent: progress?.contentLength
      ? Math.min(100, Math.round(((progress.downloaded || 0) / progress.contentLength) * 100))
      : 0,
    canCheck: !["checking", "downloading", "installing", "restarting"].includes(status),
    canInstall: status === "available",
    canRestart: status === "ready_to_restart",
    primaryAction: primaryAction.action,
    primaryActionLabel: primaryAction.label,
    primaryActionIcon: primaryAction.icon,
    canRunPrimaryAction: primaryAction.enabled,
    showSecondaryCheck: status === "available",
    error: state?.error || "",
  };
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
  const allowed = new Set(activeScenarioPaths);
  return skills.filter((skill) => allowed.has(skill.path));
}
