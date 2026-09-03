import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import en from "../locales/en.js";
import zhCN from "../locales/zh-CN.js";
import { formatTranslation } from "./i18nCore.mjs";

import {
  SUPPORTED_INSTALL_SOURCES,
  buildImportPreviewSummary,
  buildImportPreviewToken,
  buildAppUpdateProgressText,
  buildAppUpdateView,
  buildInstallDetectionView,
  buildInstallDetectionSummary,
  buildInstallDetectionWarningSummary,
  buildInstallPreviewView,
  buildInstallPreviewSummary,
  buildInstallPreviewToken,
  buildInstallPrimaryAction,
  buildInstallStructureSummary,
  buildPackageDetectionSummary,
  buildScenarioManifestPreviewSummary,
  buildScenarioSkillInventory,
  buildSkillCardView,
  buildUniqueSkillInventory,
  buildDriftGroups,
  buildDashboardStats,
  getMarketInstallRequest,
  getMarketInstallSource,
  buildSkillMateManifestPreviewSummary,
  buildSkillDescription,
  buildStructureWarningSummary,
  buildValidationSummary,
  buildSkillProfilePreviewSummary,
  buildProjectTargetPreviewSummary,
  buildGitBackupPayload,
  buildGitBackupState,
  buildInstallCommandPreview,
  filterSkillsByScenario,
  formatScenarioCopyText,
  getStructureStatusLabel,
  getStructureStatusTone,
  getPackageKindLabel,
  getAppUpdateStatusLabel,
  getAppUpdateStatusTone,
  isInstallPreviewCurrent,
  isImportPreviewCurrent,
  normalizeSkillStructure,
  normalizeScenarioSkillPaths,
  resolveScenarioSkills,
  shouldShowInstallAdvancedOptions,
  shouldShowProjectLinkOption,
} from "./skillmate.mjs";

const translateEnglish = (key, values) => formatTranslation(en, zhCN, key, values);

test("跨助手同名 Skill 只有内容哈希不同时才形成漂移组", () => {
  const assistants = [
    { name: "Codex", icon: "codex", exists: true, skills: [{ name: "writer", path: "/a/writer", content_hash: "one", managed_by_app: true, structure_status: "complete", structure_warnings: [] }] },
    { name: "Cursor", icon: "cursor", exists: true, skills: [{ name: "writer", path: "/b/writer", content_hash: "two", managed_by_app: true, structure_status: "complete", structure_warnings: [] }] },
    { name: "Claude Code", icon: "claude", exists: true, skills: [{ name: "reader", path: "/c/reader", content_hash: "same", structure_status: "complete", structure_warnings: [] }] },
    { name: "OpenCode", icon: "opencode", exists: true, skills: [{ name: "reader", path: "/d/reader", content_hash: "same", structure_status: "complete", structure_warnings: [] }] },
  ];
  const groups = buildDriftGroups(assistants);
  assert.equal(groups.length, 1);
  assert.equal(groups[0].name, "writer");
  assert.equal(groups[0].versionCount, 2);
});

test("工作台统计汇总风险、更新、诊断和内容漂移", () => {
  const stats = buildDashboardStats(
    [
      {
        name: "Codex", exists: true, diagnostics: [{ code: "x" }], skills: [
          { name: "writer", path: "/a/writer", content_hash: "one", sync_state: "behind", managed_by_app: true, structure_status: "partial", structure_warnings: ["contains_scripts", "managed_content_changed"] },
        ],
      },
      { name: "Cursor", exists: true, diagnostics: [], skills: [{ name: "writer", path: "/b/writer", content_hash: "two", managed_by_app: true, structure_status: "complete", structure_warnings: [] }] },
    ],
    [{ name: "reader", path: "/library/reader", structure_status: "complete", structure_warnings: [] }]
  );
  assert.deepEqual(stats, { skills: 3, assistants: 2, updates: 1, structureIssues: 1, securityRisks: 1, localChanges: 1, driftGroups: 1, diagnostics: 1 });
});

test("工作台按 SkillMate 库主副本统计多个启用位置", () => {
  const stats = buildDashboardStats([
    {
      name: "Codex", exists: true, skills: [{
        name: "writer",
        path: "/codex/writer",
        symlink_source: "/library/writer",
        source_type: "deployment",
        sync_state: "behind",
        structure_status: "partial",
        structure_warnings: ["contains_scripts"],
      }],
    },
    {
      name: "Claude Code", exists: true, skills: [{
        name: "writer",
        path: "/claude/writer",
        symlink_source: "/library/writer",
        source_type: "deployment",
        sync_state: "behind",
        structure_status: "partial",
        structure_warnings: ["contains_scripts"],
      }],
    },
  ]);

  assert.equal(stats.skills, 1);
  assert.equal(stats.updates, 1);
  assert.equal(stats.structureIssues, 1);
  assert.equal(stats.securityRisks, 1);
});

test("市场结果优先使用后端给出的安全安装来源", () => {
  assert.equal(getMarketInstallSource({ installSource: "owner/repo#main:skills/writer", repository: "owner/repo" }), "owner/repo#main:skills/writer");
  assert.equal(getMarketInstallSource({ repository: "owner/repo" }), "https://github.com/owner/repo.git");
});

test("skills.sh 市场安装请求保留具体 Skill 身份", () => {
  assert.deepEqual(getMarketInstallRequest({
    source: "skills-sh",
    repository: "anthropics/knowledge-work-plugins",
    install_source: "https://github.com/anthropics/knowledge-work-plugins.git",
    skill_id: "review-contract",
  }), {
    source: "https://github.com/anthropics/knowledge-work-plugins.git",
    preferredSkillId: "review-contract",
  });
});

function readAppSource() {
  return readFileSync(new URL("../App.jsx", import.meta.url), "utf8");
}

function readModalShellSource() {
  return readFileSync(new URL("../components/ModalShell.jsx", import.meta.url), "utf8");
}

function readStylesSource() {
  return readFileSync(new URL("../styles.css", import.meta.url), "utf8");
}

test("应用更新状态视图应当映射按钮能力和版本信息", () => {
  assert.deepEqual(
    buildAppUpdateView({
      status: "available",
      currentVersion: "0.0.1",
      update: {
        currentVersion: "0.0.1",
        version: "0.0.2",
        body: "修复安装流程",
      },
    }),
    {
      status: "available",
      statusLabel: "发现更新",
      statusTone: "warn",
      currentVersion: "0.0.1",
      nextVersion: "0.0.2",
      dateLabel: "未知",
      releaseNotes: "修复安装流程",
      progressText: "",
      progressPercent: 0,
      canCheck: true,
      canInstall: true,
      canRestart: false,
      primaryAction: "install",
      primaryActionLabel: "下载并安装后重启",
      primaryActionIcon: "upload",
      canRunPrimaryAction: true,
      showSecondaryCheck: true,
      error: "",
    }
  );
});

test("应用更新进度应当优先展示百分比", () => {
  assert.equal(buildAppUpdateProgressText({ downloaded: 512, contentLength: 1024 }), "50%");
  assert.equal(buildAppUpdateProgressText({ downloaded: 2048, contentLength: 0 }), "2 KB");
  assert.equal(getAppUpdateStatusLabel("restarting"), "正在重启");
  assert.equal(getAppUpdateStatusLabel("ready_to_restart"), "等待重启");
  assert.equal(getAppUpdateStatusTone("error"), "error");
  assert.deepEqual(
    buildAppUpdateView({ status: "restarting" }),
    {
      status: "restarting",
      statusLabel: "正在重启",
      statusTone: "warn",
      currentVersion: "",
      nextVersion: "",
      dateLabel: "未知",
      releaseNotes: "",
      progressText: "",
      progressPercent: 0,
      canCheck: false,
      canInstall: false,
      canRestart: false,
      primaryAction: "restart",
      primaryActionLabel: "正在重启",
      primaryActionIcon: "refresh",
      canRunPrimaryAction: false,
      showSecondaryCheck: false,
      error: "",
    }
  );
});

test("App 可靠性回归点应当避免脆弱写法", () => {
  const source = readAppSource();
  const modalShell = readModalShellSource();

  assert.doesNotMatch(source, /key=\{line\}/);
  assert.doesNotMatch(source, /includes\("已删除"\)/);
  assert.match(source, /toastTimerRef/);
  assert.match(source, /loadRequestRef/);
  assert.match(source, /library:\s*"settings\.tabs\.library"/);
  assert.match(source, /SkillMateModals\.jsx/);
  assert.match(modalShell, /function ModalShell/);
  assert.match(modalShell, /role=\{role\}/);
  assert.match(modalShell, /aria-modal="true"/);
  assert.match(modalShell, /activateModalFocus/);
});

test("桌面端弹窗应当受视口高度约束并可纵向滚动", () => {
  const baseStyles = readStylesSource().split("@media")[0];
  const modalRule = baseStyles.match(/\.modal\s*\{([^}]*)\}/s)?.[1] ?? "";

  assert.match(modalRule, /max-height:\s*calc\(100dvh - 32px\)/);
  assert.match(modalRule, /overflow-y:\s*auto/);
  assert.match(modalRule, /overscroll-behavior:\s*contain/);
});

test("经典主题的页眉与登记册只保留一处分隔线", () => {
  const styles = readStylesSource();
  const surfaceHeaderRule = styles.match(/\[data-skin="ledger"\] \.surface-header\s*\{([^}]*)\}/s)?.[1] ?? "";
  const registryRule = styles.match(/\[data-skin="ledger"\] \.registry\s*\{([^}]*)\}/s)?.[1] ?? "";

  assert.match(surfaceHeaderRule, /border-bottom:\s*3px double var\(--rule\)/);
  assert.match(registryRule, /border-top:\s*0/);
});

test("安装来源保留 Git、本地目录与 Claude Marketplace", () => {
  assert.deepEqual(SUPPORTED_INSTALL_SOURCES, ["git", "local", "claude_marketplace"]);
});

test("包级识别摘要应当区分单 Skill、多 Skill 和模型辅助", () => {
  assert.equal(getPackageKindLabel("single_skill"), "单 Skill");
  assert.equal(getPackageKindLabel("multi_skill"), "多 Skill");
  assert.equal(
    buildPackageDetectionSummary({
      package_kind: "unknown",
      detected_skills: [],
      needs_model: true,
    }),
    "未知包 · 0 个 Skill · 可选模型辅助识别"
  );
});

test("安装预览视图应当映射动作、冲突和包 warning", () => {
  assert.deepEqual(
    buildInstallPreviewView({
      can_apply: false,
      structure_status: "partial",
      message: "发现 1 个安装冲突",
      package_detection: {
        package_kind: "multi_skill",
        warnings: ["assistant_bundle_detected"],
        needs_model: false,
        detected_skills: [{ relative_path: "writer", structure_status: "complete" }],
      },
      target_actions: [{ action: "skip", source: "writer", target: "/tmp/writer", reason: "目标目录已存在" }],
      conflicts: [{ target: "/tmp/writer", reason: "target_exists" }],
    }),
    {
      canApply: false,
      tone: "error",
      message: "发现 1 个安装冲突",
      packageSummary: "多 Skill · 1 个 Skill",
      packageWarnings: "识别到助手包结构",
      skills: [{ relative_path: "writer", structure_status: "complete" }],
      actions: [{ action: "skip", source: "writer", target: "/tmp/writer", reason: "目标目录已存在", label: "跳过" }],
      conflicts: [{ target: "/tmp/writer", reason: "目标目录已存在" }],
      availableSkills: [{ relative_path: "writer", structure_status: "complete" }],
      selectionRequired: false,
      needsModel: false,
      policy: {
        mode: "off",
        allowed: true,
        message: "",
        findings: [],
      },
    }
  );
});

test("安装预览视图应当展示策略阻止原因", () => {
  const view = buildInstallPreviewView({
    can_apply: false,
    structure_status: "complete",
    package_detection: { detected_skills: [], warnings: [] },
    conflicts: [{ target: "/tmp/writer", reason: "install_policy_blocked" }],
    install_policy: {
      mode: "trusted-only",
      allowed: false,
      message: "安装策略阻止了 1 项风险",
      findings: [{ code: "untrusted_git_host", severity: "critical", message: "Git 主机 example.com 不在信任列表" }],
    },
  });

  assert.equal(view.tone, "error");
  assert.equal(view.policy.allowed, false);
  assert.deepEqual(view.policy.findings, [{
    code: "untrusted_git_host",
    severity: "critical",
    message: "Git 主机 example.com 不在信任列表",
    label: "Git 主机不在信任列表",
  }]);
});

test("安装预览视图应当映射项目软连接动作", () => {
  assert.deepEqual(
    buildInstallPreviewView({
      can_apply: true,
      structure_status: "complete",
      message: "将软连接安装 1 个 Skill",
      package_detection: {
        package_kind: "single_skill",
        warnings: [],
        needs_model: false,
        detected_skills: [{ relative_path: ".", structure_status: "complete" }],
      },
      target_actions: [{ action: "symlink", source: "/tmp/writer", target: "/tmp/project/.codex/skills/writer", reason: "创建项目级软连接" }],
      conflicts: [],
    }).actions,
    [{ action: "symlink", source: "/tmp/writer", target: "/tmp/project/.codex/skills/writer", reason: "创建项目级软连接", label: "软连接" }]
  );
});

test("添加与启用流程应当使用各自的主操作", () => {
  assert.deepEqual(
    buildInstallPrimaryAction({
      packageValue: "https://github.com/example/cool-skill",
      source: "git",
      preview: null,
      previewCurrent: false,
      previewingInstall: false,
      loading: false,
    }),
    { action: "preview", label: "分析仓库", icon: "preview", disabled: false }
  );

  assert.deepEqual(
    buildInstallPrimaryAction({
      packageValue: "https://github.com/example/cool-skill",
      preview: { can_apply: true },
      previewCurrent: true,
      previewingInstall: false,
      loading: false,
    }),
    { action: "install", label: "添加到库", icon: "plus", disabled: false }
  );

  assert.deepEqual(
    buildInstallPrimaryAction({
      packageValue: "/tmp/skillmate/skills/writer",
      source: "local",
      preview: { can_apply: true },
      previewCurrent: true,
      previewingInstall: false,
      loading: false,
      workflow: "enable",
    }),
    { action: "install", label: "启用", icon: "check", disabled: false }
  );
});

test("安装预览 token 应当绑定来源、目标和项目路径", () => {
  const token = buildInstallPreviewToken({
    packageValue: " /tmp/writer ",
    source: "local",
    assistantName: "Codex",
    installMode: "symlink",
    projectPath: " /tmp/project ",
  });

  assert.equal(
    isInstallPreviewCurrent({
      previewToken: token,
      packageValue: "/tmp/writer",
      source: "local",
      assistantName: "Codex",
      installMode: "symlink",
      projectPath: "/tmp/project",
    }),
    true
  );
  assert.equal(
    isInstallPreviewCurrent({
      previewToken: token,
      packageValue: "/tmp/writer",
      source: "local",
      assistantName: "Claude Code",
      installMode: "symlink",
      projectPath: "/tmp/project",
    }),
    false
  );
});

test("项目范围入口由平台能力决定，与来源无关", () => {
  assert.equal(
    shouldShowProjectLinkOption({
      supportsProjectSkills: true,
    }),
    true
  );
  assert.equal(
    shouldShowProjectLinkOption({
      supportsProjectSkills: false,
    }),
    false
  );
});

test("高级来源选择只应当在识别失败或手动展开时展示", () => {
  assert.equal(
    shouldShowInstallAdvancedOptions({
      advancedOpen: false,
      detection: { normalized_source: "git", confidence: "high", warnings: [], needs_model: false },
    }),
    false
  );
  assert.equal(
    shouldShowInstallAdvancedOptions({
      advancedOpen: false,
      detection: { normalized_source: "", confidence: "low", warnings: ["unrecognized_input"], needs_model: true },
    }),
    true
  );
  assert.equal(
    shouldShowInstallAdvancedOptions({
      advancedOpen: true,
      detection: null,
    }),
    true
  );
});

test("安装预览摘要应当保留结构、目标和写入计划", () => {
  assert.deepEqual(
    buildInstallPreviewSummary({
      structure_status: "complete",
      target_path: "/tmp/project/.codex/skills/writer",
      message: "将软连接安装 1 个 Skill",
      package_detection: { package_kind: "single_skill", detected_skills: [{}], needs_model: false },
      target_actions: [{ action: "symlink" }],
      conflicts: [],
    }),
    [
      "结构：符合规范",
      "目标：/tmp/project/.codex/skills/writer",
      "写入：1 个动作",
      "单 Skill · 1 个 Skill",
      "将软连接安装 1 个 Skill",
    ]
  );
});

test("验证报告摘要应当映射检查状态", () => {
  assert.deepEqual(
    buildValidationSummary({
      checks: [
        { code: "entry_document", status: "pass", message: "已识别标准入口文档" },
        { code: "compatibility", status: "warning", message: "未声明 compatible 元数据" },
      ],
    }),
    [
      { code: "entry_document", status: "pass", message: "已识别标准入口文档", label: "通过" },
      { code: "compatibility", status: "warning", message: "未声明 compatible 元数据", label: "提醒" },
    ]
  );
});

test("SkillMate manifest 预览摘要应当提示安装动作和冲突", () => {
  assert.deepEqual(
    buildSkillMateManifestPreviewSummary({
      actions: [{ assistant: "Codex", target_name: "writer", message: "将安装 1 个 Skill" }],
      conflicts: [{ assistant: "Claude Code", reason: "发现 1 个安装冲突" }],
    }),
    [
      "将安装 1 条 Skill 记录",
      "存在 1 个冲突",
      "Codex：writer · 将安装 1 个 Skill",
      "Claude Code：发现 1 个安装冲突",
    ]
  );
});

test("SkillMate manifest 预览摘要应当提示格式问题", () => {
  assert.deepEqual(
    buildSkillMateManifestPreviewSummary({
      validation_issues: [{ index: 0, message: "缺少 assistant" }],
      actions: [],
      conflicts: [],
    }),
    ["存在 1 个格式问题", "#1：缺少 assistant"]
  );
});

test("Skill Profile 预览摘要应当提示组合名称和受管对齐边界", () => {
  assert.deepEqual(
    buildSkillProfilePreviewSummary({
      profile: { name: "写作模式", skills: [{}, {}] },
      manifest_preview: {
        actions: [{ assistant: "Codex", target_name: "writer", message: "将安装 1 个 Skill" }],
        conflicts: [],
      },
      diff: {
        to_install: ["Codex:writer:local"],
        already_present: ["Claude Code:review:local"],
        to_remove: ["Codex:old:local"],
        conflicts: [],
      },
    }),
    [
      "写作模式 · 2 条 Skill 记录",
      "将安装 1 条 Skill 记录",
      "Codex：writer · 将安装 1 个 Skill",
      "将补齐 1 条缺失记录",
      "1 条记录已存在",
      "将移除 1 条不在目标组合中的受管记录",
      "应用 Profile 会对齐 SkillMate 受管 Skill，不会删除手工添加的目录",
    ]
  );
});

test("Skill Profile 预览摘要应当提示 Profile 格式问题", () => {
  assert.deepEqual(
    buildSkillProfilePreviewSummary({
      profile: { name: "", skills: [] },
      profile_issues: [{ code: "empty_skills", message: "Profile 至少需要包含一条 Skill 记录" }],
      manifest_preview: { validation_issues: [], actions: [], conflicts: [] },
      diff: {},
    }),
    [
      "未命名 Profile · 0 条 Skill 记录",
      "manifest 没有可执行动作",
      "Profile 有 1 个格式问题",
      "Profile 至少需要包含一条 Skill 记录",
      "应用 Profile 会对齐 SkillMate 受管 Skill，不会删除手工添加的目录",
    ]
  );
});

test("项目目标预览摘要应当展示推荐和已存在状态", () => {
  assert.deepEqual(
    buildProjectTargetPreviewSummary([
      { assistant: "Codex", target_path: "/tmp/project/.codex/skills", exists: true, recommended: true },
      { assistant: "Claude Code", target_path: "/tmp/project/.claude/skills", exists: false, recommended: false },
    ]),
    [
      "Codex：/tmp/project/.codex/skills · 已存在 · 推荐",
      "Claude Code：/tmp/project/.claude/skills",
    ]
  );
});

test("安装预览必须体现目标助手和来源", () => {
  assert.equal(
    buildInstallCommandPreview({
      source: "git",
      installMode: "library",
    }),
    "添加到 SkillMate 库，暂不启用"
  );

  assert.equal(
    buildInstallCommandPreview({
      source: "git",
      packageValue: "https://github.com/example/cool-skill.git",
      assistantName: "Codex",
    }),
    "在 Codex 中全局启用"
  );

  assert.equal(
    buildInstallCommandPreview({
      source: "local",
      packageValue: "/tmp/cool-skill",
      assistantName: "Claude Code",
    }),
    "在 Claude Code 中全局启用"
  );

  assert.equal(
    buildInstallCommandPreview({
      source: "local",
      assistantName: "Codex",
      installMode: "symlink",
      projectPath: "/tmp/project",
    }),
    "在 /tmp/project 的 Codex 中启用"
  );
});

test("结构状态应当映射为稳定中文文案和语义样式", () => {
  assert.equal(getStructureStatusLabel("complete"), "符合规范");
  assert.equal(getStructureStatusLabel("partial"), "需要修复");
  assert.equal(getStructureStatusLabel("unknown"), "非 Skill");
  assert.equal(getStructureStatusTone("complete"), "success");
  assert.equal(getStructureStatusTone("partial"), "warn");
  assert.equal(getStructureStatusTone("unknown"), "error");
});

test("Skill 结构数据适配应当容忍缺失字段", () => {
  assert.deepEqual(normalizeSkillStructure({}), {
    status: "nonstandard",
    features: [],
    warnings: [],
    manifestTitle: "",
    manifestDescription: "",
  });

  assert.deepEqual(
    normalizeSkillStructure({
      structure_status: "complete",
      structure_features: ["skill_md", "name", "description"],
      structure_warnings: [],
      manifest_title: "写作",
      manifest_description: "处理文稿",
    }),
    {
      status: "complete",
      features: ["skill_md", "name", "description"],
      warnings: [],
      manifestTitle: "写作",
      manifestDescription: "处理文稿",
    }
  );
});

test("结构 warning 摘要应当输出可读中文", () => {
  assert.equal(
    buildStructureWarningSummary({
      structure_warnings: ["missing_skill_md", "frontmatter_invalid", "target_exists"],
    }),
    "缺少 SKILL.md、YAML frontmatter 无效、目标目录已存在"
  );

  assert.equal(
    buildStructureWarningSummary({ structure_warnings: [] }),
    "结构未发现明显问题"
  );
});

test("安装来源识别摘要应当输出来源、引用和目标", () => {
  assert.equal(
    buildInstallDetectionSummary({
      source_kind: "git_subdir",
      confidence: "high",
      reference: "main",
      subdir: "skills/writer",
      target_name: "writer",
      needs_model: false,
    }),
    "已识别为 Git 仓库子目录 · 引用 main · 子目录 skills/writer · 目标 writer"
  );

  assert.equal(
    buildInstallDetectionSummary({
      source_kind: "unknown",
      confidence: "low",
      needs_model: true,
    }),
    "尚未识别来源 · 可用模型辅助识别"
  );
});

test("安装来源识别 warning 应当复用稳定中文映射", () => {
  assert.equal(
    buildInstallDetectionWarningSummary({
      warnings: ["archive_unsupported", "unrecognized_input"],
    }),
    "压缩包安装暂未支持、规则无法识别"
  );

  assert.equal(buildInstallDetectionWarningSummary({ warnings: [] }), "");
});

test("安装来源识别视图应当集中卡片所需展示数据", () => {
  assert.deepEqual(
    buildInstallDetectionView({
      detector: "rules",
      source_kind: "git_subdir",
      confidence: "high",
      reference: "main",
      subdir: "skills/writer",
      target_name: "writer",
      warnings: [],
      needs_model: false,
    }),
    {
      title: "本地规则",
      tone: "success",
      summary: "已识别为 Git 仓库子目录 · 引用 main · 子目录 skills/writer · 目标 writer",
      warningSummary: "",
      sourceLabel: "Git 仓库子目录",
      confidenceLabel: "高置信度",
      needsModel: false,
    }
  );

  assert.deepEqual(buildInstallDetectionView(null), null);
});

test("Skill 描述优先使用 manifest description", () => {
  assert.equal(
    buildSkillDescription({
      manifest_description: "来自 frontmatter 的说明",
      readme: "# 标题\n\nREADME 说明",
    }),
    "来自 frontmatter 的说明"
  );

  assert.equal(
    buildSkillDescription({
      readme: "# 标题\n\nREADME 说明",
    }),
    "README 说明"
  );
});

test("安装结果摘要应当包含结构状态和风险", () => {
  assert.equal(
    buildInstallStructureSummary({
      structure_status: "partial",
      structure_warnings: ["missing_skill_md"],
    }),
    "结构需要修复：缺少 SKILL.md"
  );

  assert.equal(
    buildInstallStructureSummary({
      structure_status: "complete",
      structure_warnings: [],
    }),
    "结构符合规范"
  );
});

test("Skill 卡片视图应当优先使用 manifest 标题和说明", () => {
  assert.deepEqual(
    buildSkillCardView({
      name: "fallback",
      source: "Git",
      source_type: "git",
      managed_by_app: true,
      has_update: true,
      can_sync: true,
      structure_status: "partial",
      structure_warnings: ["missing_description"],
      manifest_title: "写作助手",
      manifest_description: "处理文稿",
      readme: "# fallback",
    }),
    {
      title: "写作助手",
      description: "处理文稿",
      structureLabel: "需要修复",
      structureTone: "warn",
      warningSummary: "缺少必填 description",
      securityWarningCount: 0,
      securityWarningSummary: "",
      hasManagedDrift: false,
      sourceLabel: "Git",
      contentSourceLabel: "Git 仓库",
      managerLabel: "SkillMate",
      updateStrategyLabel: "由 SkillMate 更新",
      canSync: true,
      hasUpdate: true,
      canEnable: false,
      canDelete: true,
      canUnlink: false,
      canAdopt: false,
      availableIn: [],
      availabilityLabel: "",
      isShared: false,
    }
  );
});

test("共享目录中的 Skill 在总览只保留一条并聚合可用助手", () => {
  const inventory = buildUniqueSkillInventory([
    {
      name: "Codex",
      icon: "codex",
      skills: [{ id: "/Users/demo/.agents/skills/writer", path: "/Users/demo/.agents/skills/writer", name: "writer" }],
    },
    {
      name: "Gemini CLI",
      icon: "gemini",
      skills: [{ id: "/Users/demo/.agents/skills/writer", path: "/Users/demo/.agents/skills/writer", name: "writer" }],
    },
  ]);

  assert.equal(inventory.length, 1);
  assert.deepEqual(inventory[0].availableIn, [
    { name: "Codex", icon: "codex" },
    { name: "Gemini CLI", icon: "gemini" },
  ]);
  assert.equal(buildSkillCardView(inventory[0]).availabilityLabel, "Codex、Gemini CLI");
  assert.equal(buildSkillCardView(inventory[0]).isShared, true);
});

test("不同逻辑路径即使名称相同也保持为独立安装位置", () => {
  const inventory = buildUniqueSkillInventory([
    { name: "Claude Code", icon: "claude", skills: [{ path: "/project/.claude/skills/writer", name: "writer" }] },
    { name: "Gemini CLI", icon: "gemini", skills: [{ path: "/project/.gemini/skills/writer", name: "writer" }] },
  ]);

  assert.equal(inventory.length, 2);
  assert.deepEqual(inventory.map((skill) => skill.path), [
    "/project/.claude/skills/writer",
    "/project/.gemini/skills/writer",
  ]);
});

test("场景按 SkillMate 库主副本合并多个启用位置", () => {
  const inventory = buildScenarioSkillInventory([
    {
      path: "/project/.agents/skills/writer",
      symlink_source: "/library/writer",
      source_type: "deployment",
      name: "writer",
      availableIn: [{ name: "Codex", icon: "codex" }],
    },
    {
      path: "/project/.claude/skills/writer",
      symlink_source: "/library/writer",
      source_type: "deployment",
      name: "writer",
      availableIn: [{ name: "Claude Code", icon: "claude" }],
    },
  ]);

  assert.equal(inventory.length, 1);
  assert.equal(inventory[0].path, "/library/writer");
  assert.deepEqual(inventory[0].availableIn, [
    { name: "Codex", icon: "codex" },
    { name: "Claude Code", icon: "claude" },
  ]);
  assert.deepEqual(
    filterSkillsByScenario({
      skills: [{
        path: "/project/.agents/skills/writer",
        symlink_source: "/library/writer",
        source_type: "deployment",
      }],
      activeScenarioPaths: ["/library/writer"],
    }).map((skill) => skill.path),
    ["/project/.agents/skills/writer"]
  );
});

test("Skill 卡片应当显式汇总静态风险与受管漂移", () => {
  const view = buildSkillCardView({
    name: "network-skill",
    structure_status: "complete",
    structure_warnings: ["contains_scripts", "references_network", "managed_content_changed"],
  });

  assert.equal(view.securityWarningCount, 2);
  assert.equal(view.securityWarningSummary, "包含可执行脚本、可能访问网络");
  assert.equal(view.hasManagedDrift, true);
});

test("Skill 卡片动作只应暴露受管删除或软连接解除", () => {
  assert.deepEqual(
    {
      unmanaged: buildSkillCardView({
        name: "manual",
        source_type: "local",
        managed_by_app: false,
      }).canDelete,
      managed: buildSkillCardView({
        name: "managed",
        source_type: "git",
        managed_by_app: true,
      }).canDelete,
      symlinkDelete: buildSkillCardView({
        name: "linked",
        source_type: "symlink",
        managed_by_app: true,
      }).canDelete,
      symlinkUnlink: buildSkillCardView({
        name: "linked",
        source_type: "symlink",
        managed_by_app: true,
      }).canUnlink,
    },
    {
      unmanaged: false,
      managed: true,
      symlinkDelete: false,
      symlinkUnlink: true,
    }
  );
});

test("SkillMate 库主副本与部署都可以继续启用到其他范围", () => {
  assert.equal(buildSkillCardView({ in_library: true }).canEnable, true);
  assert.equal(buildSkillCardView({ source_type: "deployment" }).canEnable, true);
  assert.equal(buildSkillCardView({ source_type: "local" }).canEnable, false);
});

test("场景选择必须保存稳定路径而不是临时 ID", () => {
  const skills = [
    { id: "temp-1", path: "/Users/demo/.codex/skills/a" },
    { id: "temp-2", path: "/Users/demo/.codex/skills/b" },
  ];

  assert.deepEqual(
    normalizeScenarioSkillPaths({
      selectedPaths: [],
      manualInput: "",
      skills,
    }),
    []
  );

  assert.deepEqual(
    normalizeScenarioSkillPaths({
      selectedPaths: ["/Users/demo/.codex/skills/b"],
      manualInput: "",
      skills,
    }),
    ["/Users/demo/.codex/skills/b"]
  );
});

test("Git 备份保存时必须保留仓库路径、远端地址和分支", () => {
  assert.deepEqual(
    buildGitBackupPayload({
      repoPath: " /tmp/skillmate-backup ",
      remoteUrl: " git@github.com:demo/skills.git ",
      branch: " backup/main ",
    }),
    {
      repoPath: "/tmp/skillmate-backup",
      remoteUrl: "git@github.com:demo/skills.git",
      branch: "backup/main",
    }
  );
});

test("Git 备份按钮能力统一处理空白、默认分支、未保存和忙碌状态", () => {
  const saved = { repo_path: "~/backup", remote_url: "", branch: "main" };
  assert.deepEqual(
    buildGitBackupState({
      draft: { repoPath: " ~/backup ", remoteUrl: " ", branch: " " },
      saved,
    }),
    {
      payload: { repoPath: "~/backup", remoteUrl: "", branch: "main" },
      dirty: false,
      configured: true,
      saving: false,
      syncing: false,
      canSave: false,
      canSync: true,
    }
  );
  const dirty = buildGitBackupState({
    draft: { repoPath: "~/other", remoteUrl: "", branch: "main" },
    saved,
  });
  assert.equal(dirty.dirty, true);
  assert.equal(dirty.canSave, true);
  assert.equal(dirty.canSync, false);
  const unconfigured = buildGitBackupState({
    draft: { repoPath: "", remoteUrl: "", branch: "main" },
    saved: {},
  });
  assert.equal(unconfigured.configured, false);
  assert.equal(unconfigured.canSync, false);
  assert.equal(buildGitBackupState({
    draft: { repoPath: "~/backup", remoteUrl: "", branch: "main" },
    saved,
    syncing: true,
  }).canSync, false);
});

test("导入预览 token 应当绑定路径和模式", () => {
  const token = buildImportPreviewToken({
    path: " ~/skillmate-export.json ",
    mode: "replace",
  });

  assert.deepEqual(token, {
    path: "~/skillmate-export.json",
    mode: "replace",
  });
  assert.equal(
    isImportPreviewCurrent({
      previewToken: token,
      path: "~/skillmate-export.json",
      mode: "replace",
    }),
    true
  );
  assert.equal(
    isImportPreviewCurrent({
      previewToken: token,
      path: "~/other.json",
      mode: "replace",
    }),
    false
  );
  assert.equal(
    isImportPreviewCurrent({
      previewToken: token,
      path: "~/skillmate-export.json",
      mode: "merge",
    }),
    false
  );
});


test("场景详情应当能解析出存在与缺失的 Skill 路径", () => {
  const skills = [
    { path: "/Users/demo/.codex/skills/a", name: "A", ai: "Codex" },
    { path: "/Users/demo/.codex/skills/b", name: "B", ai: "Codex" },
  ];

  assert.deepEqual(
    resolveScenarioSkills({
      scenario: {
        skill_ids: [
          "/Users/demo/.codex/skills/a",
          "/Users/demo/.codex/skills/missing",
        ],
      },
      allSkills: skills,
    }),
    [
      {
        path: "/Users/demo/.codex/skills/a",
        exists: true,
        skill: { path: "/Users/demo/.codex/skills/a", name: "A", ai: "Codex" },
      },
      {
        path: "/Users/demo/.codex/skills/missing",
        exists: false,
        skill: null,
      },
    ]
  );
});

test("场景复制文本应当保留每个路径独立成行", () => {
  assert.equal(
    formatScenarioCopyText([
      "/Users/demo/.codex/skills/a",
      "/Users/demo/.codex/skills/b",
    ]),
    "/Users/demo/.codex/skills/a\n/Users/demo/.codex/skills/b"
  );
});

test("应用场景后只保留场景内的 Skill", () => {
  const skills = [
    { path: "/Users/demo/.codex/skills/a", name: "A" },
    { path: "/Users/demo/.codex/skills/b", name: "B" },
  ];

  assert.deepEqual(
    filterSkillsByScenario({
      skills,
      activeScenarioPaths: ["/Users/demo/.codex/skills/b"],
    }),
    [{ path: "/Users/demo/.codex/skills/b", name: "B" }]
  );
  assert.deepEqual(
    filterSkillsByScenario({ skills, activeScenarioPaths: [] }),
    []
  );
});

test("导入预览摘要应当给出新增、覆盖和标签写入数量", () => {
  assert.deepEqual(
    buildImportPreviewSummary({
      replace_existing: false,
      tags_to_add: 1,
      tags_to_replace: 2,
      scenarios_to_add: 3,
      scenarios_to_replace: 4,
      skill_tag_writes: 5,
      existing_tags_to_remove: 0,
      existing_scenarios_to_remove: 0,
      existing_skill_tag_mappings_to_remove: 0,
    }),
    [
      "将新增 1 个标签",
      "将覆盖 2 个标签",
      "将新增 3 个组合",
      "将覆盖 4 个组合",
      "将写入 5 条 Skill 标签映射",
    ]
  );
});

test("替换导入预览应当额外提示将清空的现有数据", () => {
  assert.deepEqual(
    buildImportPreviewSummary({
      replace_existing: true,
      tags_to_add: 1,
      tags_to_replace: 0,
      scenarios_to_add: 2,
      scenarios_to_replace: 0,
      skill_tag_writes: 1,
      existing_tags_to_remove: 6,
      existing_scenarios_to_remove: 3,
      existing_skill_tag_mappings_to_remove: 8,
    }),
    [
      "将清空现有 6 个标签",
      "将清空现有 3 个组合",
      "将清空现有 8 条 Skill 标签映射",
      "将新增 1 个标签",
      "将新增 2 个组合",
      "将写入 1 条 Skill 标签映射",
    ]
  );
});

test("组合清单预览摘要应当提示覆盖、清空和缺失引用", () => {
  assert.deepEqual(
    buildScenarioManifestPreviewSummary({
      replace_existing: true,
      scenarios_to_add: 1,
      scenarios_to_replace: 2,
      existing_scenarios_to_remove: 3,
      missing_skill_refs: ["/tmp/missing-a", "/tmp/missing-b"],
    }),
    [
      "将清空现有 3 个组合",
      "将新增 1 个组合",
      "将覆盖 2 个组合",
      "有 2 个 Skill 路径当前不存在",
    ]
  );

  assert.deepEqual(
    buildScenarioManifestPreviewSummary({
      replace_existing: false,
      scenarios_to_add: 0,
      scenarios_to_replace: 0,
      existing_scenarios_to_remove: 0,
      missing_skill_refs: [],
    }),
    ["未检测到可导入的组合变化"]
  );
});

test("英文界面应当本地化结构风险和声明式预览摘要", () => {
  const card = buildSkillCardView({
    name: "writer",
    path: "/tmp/writer",
    structure_status: "partial",
    structure_warnings: ["contains_scripts", "managed_content_changed"],
  }, translateEnglish);
  assert.equal(card.structureLabel, "Needs fixes");
  assert.equal(card.securityWarningSummary, "Contains executable scripts");

  assert.deepEqual(
    buildImportPreviewSummary({
      replace_existing: false,
      tags_to_add: 2,
      tags_to_replace: 0,
      scenarios_to_add: 0,
      scenarios_to_replace: 0,
      skill_tag_writes: 0,
    }, translateEnglish),
    ["Add 2 tags"],
  );
  assert.deepEqual(
    buildProjectTargetPreviewSummary([{ assistant: "Cursor", target_path: "/tmp/.cursor/skills", exists: true, recommended: true }], translateEnglish),
    ["Cursor: /tmp/.cursor/skills · exists · recommended"],
  );
});
