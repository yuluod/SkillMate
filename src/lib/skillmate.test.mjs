import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

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
  buildSkillCardView,
  buildSkillMateManifestPreviewSummary,
  buildSkillDescription,
  buildStructureWarningSummary,
  buildValidationSummary,
  buildSkillProfilePreviewSummary,
  buildProjectTargetPreviewSummary,
  buildGitBackupPayload,
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

function readAppSource() {
  return readFileSync(new URL("../App.jsx", import.meta.url), "utf8");
}

function readModalShellSource() {
  return readFileSync(new URL("../components/ModalShell.jsx", import.meta.url), "utf8");
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
      error: "",
    }
  );
});

test("应用更新进度应当优先展示百分比", () => {
  assert.equal(buildAppUpdateProgressText({ downloaded: 512, contentLength: 1024 }), "50%");
  assert.equal(buildAppUpdateProgressText({ downloaded: 2048, contentLength: 0 }), "2 KB");
  assert.equal(getAppUpdateStatusLabel("ready_to_restart"), "等待重启");
  assert.equal(getAppUpdateStatusTone("error"), "error");
});

test("App 可靠性回归点应当避免脆弱写法", () => {
  const source = readAppSource();
  const modalShell = readModalShellSource();

  assert.doesNotMatch(source, /key=\{line\}/);
  assert.doesNotMatch(source, /includes\("已删除"\)/);
  assert.match(source, /toastTimerRef/);
  assert.match(source, /loadRequestRef/);
  assert.match(source, /SkillMateModals\.jsx/);
  assert.match(modalShell, /function ModalShell/);
  assert.match(modalShell, /role="dialog"/);
  assert.match(modalShell, /aria-modal="true"/);
  assert.match(modalShell, /event\.key === "Escape"/);
});

test("安装来源只保留 Git 仓库和本地目录", () => {
  assert.deepEqual(SUPPORTED_INSTALL_SOURCES, ["git", "local"]);
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
      conflicts: [{ target: "/tmp/writer", reason: "target_exists" }],
      needsModel: false,
    }
  );
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

test("安装主按钮应当在预览通过后从检查切换为安装", () => {
  assert.deepEqual(
    buildInstallPrimaryAction({
      packageValue: "https://github.com/example/cool-skill",
      preview: null,
      previewCurrent: false,
      previewingInstall: false,
      loading: false,
    }),
    { action: "preview", label: "检查结构", icon: "preview", disabled: false }
  );

  assert.deepEqual(
    buildInstallPrimaryAction({
      packageValue: "https://github.com/example/cool-skill",
      preview: { can_apply: true },
      previewCurrent: true,
      previewingInstall: false,
      loading: false,
    }),
    { action: "install", label: "安装", icon: "plus", disabled: false }
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

test("项目软连接入口只应当对本地目录来源展示", () => {
  assert.equal(
    shouldShowProjectLinkOption({
      source: "git",
      detection: { normalized_source: "git" },
    }),
    false
  );
  assert.equal(
    shouldShowProjectLinkOption({
      source: "git",
      detection: { normalized_source: "local" },
    }),
    true
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
      "结构：完整",
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

test("Skill Profile 预览摘要应当提示组合名称和非破坏性应用边界", () => {
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
        conflicts: [],
      },
    }),
    [
      "写作模式 · 2 条 Skill 记录",
      "将安装 1 条 Skill 记录",
      "Codex：writer · 将安装 1 个 Skill",
      "将补齐 1 条缺失记录",
      "1 条记录已存在",
      "应用 Profile 只安装缺失的受管 Skill，不会自动删除手工目录",
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
      "应用 Profile 只安装缺失的受管 Skill，不会自动删除手工目录",
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
      packageValue: "https://github.com/example/cool-skill.git",
      assistantName: "Codex",
    }),
    "克隆 Git 仓库到 Codex Skills 目录"
  );

  assert.equal(
    buildInstallCommandPreview({
      source: "local",
      packageValue: "/tmp/cool-skill",
      assistantName: "Claude Code",
    }),
    "复制本地目录到 Claude Code Skills 目录"
  );

  assert.equal(
    buildInstallCommandPreview({
      source: "local",
      assistantName: "Codex",
      installMode: "symlink",
      projectPath: "/tmp/project",
    }),
    "将本地目录软连接到 /tmp/project 的 Codex Skills 目录"
  );
});

test("结构状态应当映射为稳定中文文案和语义样式", () => {
  assert.equal(getStructureStatusLabel("complete"), "完整");
  assert.equal(getStructureStatusLabel("partial"), "部分");
  assert.equal(getStructureStatusLabel("unknown"), "非标准");
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
      structure_features: ["skill_md"],
      structure_warnings: ["missing_support_dirs"],
      manifest_title: "写作",
      manifest_description: "处理文稿",
    }),
    {
      status: "complete",
      features: ["skill_md"],
      warnings: ["missing_support_dirs"],
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
    "缺少 SKILL.md、frontmatter 无效、目标目录已存在"
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
    "识别为Git 仓库子目录 · 高置信度 · 引用 main · 子目录 skills/writer · 目标 writer"
  );

  assert.equal(
    buildInstallDetectionSummary({
      source_kind: "unknown",
      confidence: "low",
      needs_model: true,
    }),
    "识别为未知来源 · 低置信度 · 可用模型辅助识别"
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
      summary: "识别为Git 仓库子目录 · 高置信度 · 引用 main · 子目录 skills/writer · 目标 writer",
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
    "结构部分：缺少 SKILL.md"
  );

  assert.equal(
    buildInstallStructureSummary({
      structure_status: "complete",
      structure_warnings: [],
    }),
    "结构完整"
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
      structure_warnings: ["missing_support_dirs"],
      manifest_title: "写作助手",
      manifest_description: "处理文稿",
      readme: "# fallback",
    }),
    {
      title: "写作助手",
      description: "处理文稿",
      structureLabel: "部分",
      structureTone: "warn",
      warningSummary: "缺少资源目录",
      sourceLabel: "Git",
      canSync: true,
      hasUpdate: true,
      canDelete: true,
      canUnlink: false,
    }
  );
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
    ["/Users/demo/.codex/skills/a", "/Users/demo/.codex/skills/b"]
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
      "将新增 3 个场景",
      "将覆盖 4 个场景",
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
      "将清空现有 3 个场景",
      "将清空现有 8 条 Skill 标签映射",
      "将新增 1 个标签",
      "将新增 2 个场景",
      "将写入 1 条 Skill 标签映射",
    ]
  );
});

test("场景 manifest 预览摘要应当提示覆盖、清空和缺失引用", () => {
  assert.deepEqual(
    buildScenarioManifestPreviewSummary({
      replace_existing: true,
      scenarios_to_add: 1,
      scenarios_to_replace: 2,
      existing_scenarios_to_remove: 3,
      missing_skill_refs: ["/tmp/missing-a", "/tmp/missing-b"],
    }),
    [
      "将清空现有 3 个场景",
      "将新增 1 个场景",
      "将覆盖 2 个场景",
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
    ["未检测到可导入的场景变化"]
  );
});
