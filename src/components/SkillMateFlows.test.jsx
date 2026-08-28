import React from "react";
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssistantsView, SkillsView, UpdatesView } from "./InventoryViews.jsx";
import DashboardView from "./DashboardView.jsx";
import { InstallModal, PreviewModal } from "./SkillMateModals.jsx";
import SettingsView from "./SettingsView.jsx";
import ScenarioView from "./ScenarioView.jsx";
import { persistPreference } from "../App.jsx";
import { useInstallFlow } from "../lib/useInstallFlow.js";
import { useInstallPolicyFlow } from "../lib/useInstallPolicyFlow.js";
import { useGitBackupFlow } from "../lib/useGitBackupFlow.js";
import { useScenarioFlow } from "../lib/useScenarioFlow.js";
import { useSearchFlow } from "../lib/useSearchFlow.js";
import { useAppUpdateFlow } from "../lib/useAppUpdateFlow.js";
import { skillmateApi } from "../lib/skillmateApi.js";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
const updaterMocks = vi.hoisted(() => ({
  app: { getVersion: vi.fn() },
  updater: { check: vi.fn() },
  process: { relaunch: vi.fn() },
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args) => invoke(...args),
}));

vi.mock("@tauri-apps/api/app", () => updaterMocks.app);
vi.mock("@tauri-apps/plugin-updater", () => updaterMocks.updater);
vi.mock("@tauri-apps/plugin-process", () => updaterMocks.process);

function installFlow(overrides = {}) {
  return {
    source: {
      kind: "git",
      setKind: vi.fn(),
      package: "example/skills",
      setPackage: vi.fn(),
      detectionView: null,
    },
    target: {
      assistant: "Codex",
      setAssistant: vi.fn(),
      mode: "copy",
      setMode: vi.fn(),
      projectPath: "",
      setProjectPath: vi.fn(),
      projectPreview: [],
      previewingProject: false,
      showProjectLinkOption: false,
    },
    preview: {
      structure: {
        can_apply: false,
        structure_status: "complete",
        package_detection: { detected_skills: [], warnings: [] },
        target_actions: [],
        conflicts: [{ target: "/tmp/writer", reason: "install_policy_blocked" }],
        install_policy: {
          mode: "trusted-only",
          allowed: false,
          message: "安装策略阻止了 1 项风险",
          findings: [{ code: "untrusted_git_host", severity: "critical", message: "Git 主机 example.com 不在信任列表" }],
        },
      },
      view: {
        canApply: false,
        tone: "error",
        packageWarnings: "",
        needsModel: false,
        skills: [],
        actions: [],
        conflicts: [{ target: "/tmp/writer", reason: "install_policy_blocked" }],
        policy: {
          mode: "trusted-only",
          allowed: false,
          message: "安装策略阻止了 1 项风险",
          findings: [{ code: "untrusted_git_host", label: "Git 主机不在信任列表", message: "Git 主机 example.com 不在信任列表" }],
        },
      },
      current: true,
      primaryAction: { icon: "preview", label: "检查结构", disabled: false },
      runPrimaryAction: vi.fn(),
    },
    disclosure: {
      detailsOpen: false,
      setDetailsOpen: vi.fn(),
      advancedOpen: false,
      setAdvancedOpen: vi.fn(),
      showAdvancedOptions: false,
    },
    commandPreview: "克隆 Git 仓库到 Codex Skills 目录",
    ...overrides,
  };
}

describe("Dashboard 数据加载", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("可选模块失败时仍返回助手和其他可用数据", async () => {
    invoke.mockImplementation(async (command) => {
      if (command === "get_all_assistants") return [{ name: "Codex", skills: [] }];
      if (command === "get_all_tags") throw new Error("标签数据库不可用");
      if (command === "get_scenarios") return [{ id: "writing", name: "写作" }];
      if (command === "get_git_backup") return { enabled: true, repo_path: "/tmp/backup" };
      throw new Error(`未处理命令: ${command}`);
    });

    const result = await skillmateApi.inventory.loadDashboard();

    expect(result.assistants).toEqual([{ name: "Codex", skills: [] }]);
    expect(result.tags).toEqual([]);
    expect(result.scenarios).toEqual([{ id: "writing", name: "写作" }]);
    expect(result.git).toEqual({ enabled: true, repo_path: "/tmp/backup" });
    expect(result.diagnostics).toEqual([{
      section: "tags",
      label: "标签",
      message: "Error: 标签数据库不可用",
    }]);
  });

  it("核心助手扫描失败时不返回成功形状", async () => {
    invoke.mockRejectedValueOnce(new Error("助手目录不可读"));

    await expect(skillmateApi.inventory.loadDashboard()).rejects.toThrow("助手目录不可读");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("概览优先展示 Skill 查找并说明实际检索来源", () => {
    const { container } = render(
      <DashboardView
        stats={{ skills: 21, assistants: 4, updates: 0, structureIssues: 0, securityRisks: 0, localChanges: 0, driftGroups: 0, diagnostics: 0 }}
        tagCount={4}
        driftGroups={[]}
        onNavigate={vi.fn()}
        onMarketInstall={vi.fn()}
        onOpenDrift={vi.fn()}
      />
    );

    const sections = [...container.querySelectorAll(".dashboard-section")];
    expect(sections[0].classList.contains("market-search")).toBe(true);
    expect(screen.getByText(/skills\.sh 公共 Skill 索引/)).toBeTruthy();

    fireEvent.change(screen.getByLabelText("查找来源"), { target: { value: "github" } });
    expect(screen.getByText(/GitHub 仓库/)).toBeTruthy();
  });

  it("市场结果使用系统浏览器打开来源，安装动作只进入检查流程", async () => {
    const onMarketInstall = vi.fn();
    invoke.mockImplementation(async (command) => {
      if (command === "search_market") return {
        items: [{
          id: "github:owner/repo",
          source: "github",
          name: "repo",
          description: "测试 Skill",
          repository: "owner/repo",
          stars: 10,
          installs: 0,
          url: "https://github.com/owner/repo",
          installSource: "https://github.com/owner/repo.git",
        }],
      };
      if (command === "open_external_url") return null;
      throw new Error(`未处理命令: ${command}`);
    });

    render(
      <DashboardView
        stats={{ skills: 0, assistants: 0, updates: 0, structureIssues: 0, securityRisks: 0, localChanges: 0, driftGroups: 0, diagnostics: 0 }}
        tagCount={0}
        driftGroups={[]}
        onNavigate={vi.fn()}
        onMarketInstall={onMarketInstall}
        onOpenDrift={vi.fn()}
      />
    );

    fireEvent.change(screen.getByPlaceholderText("搜索写作、测试、PDF..."), { target: { value: "repo" } });
    fireEvent.click(screen.getByRole("button", { name: "查找" }));
    await screen.findByText("owner/repo");

    fireEvent.click(screen.getByRole("button", { name: "查看来源" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("open_external_url", { url: "https://github.com/owner/repo" }));

    fireEvent.click(screen.getByRole("button", { name: "检查后安装" }));
    expect(onMarketInstall).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalledWith("install_skill", expect.anything());
  });

  it("结构验证失败时仍展示 Skill 文档和诊断", async () => {
    invoke.mockImplementation(async (command) => {
      if (command === "get_skill_readme") return "# Writer\n\n写作说明";
      if (command === "inspect_skill_validation") throw new Error("结构验证暂时不可用");
      throw new Error(`未处理命令: ${command}`);
    });

    const preview = await skillmateApi.inventory.readSkill("/tmp/writer");
    render(<PreviewModal preview={{ title: "writer", ...preview }} onClose={vi.fn()} />);

    expect(screen.getByText(/# Writer/)).toBeTruthy();
    expect(screen.getByText(/结构验证暂时不可用/)).toBeTruthy();
    expect(preview.validation).toBeNull();
  });
});

describe("外观偏好与登记册布局", () => {
  it("URL 临时覆盖不写回本地偏好", () => {
    const storage = { setItem: vi.fn() };

    expect(persistPreference(storage, "?skin=cardbox", "skin", "skillmate-skin", ["ledger", "standard", "cardbox"], "cardbox")).toBe(false);
    expect(persistPreference(storage, "?theme=light", "theme", "skillmate-theme-mode", ["system", "light", "dark"], "light")).toBe(false);
    expect(storage.setItem).not.toHaveBeenCalled();

    expect(persistPreference(storage, "", "skin", "skillmate-skin", ["ledger", "standard", "cardbox"], "standard")).toBe(true);
    expect(storage.setItem).toHaveBeenCalledWith("skillmate-skin", "standard");
  });

  it("Skills 与 Updates 空状态保持四列登记行结构", () => {
    const skillsRender = render(
      <SkillsView
        skills={[]}
        allSkillCount={0}
        selectedTagCount={0}
        tags={[]}
        onInstall={vi.fn()}
        onClearFilters={vi.fn()}
        onEditTags={vi.fn()}
        onOpenDirectory={vi.fn()}
        onPreview={vi.fn()}
        onUnlink={vi.fn()}
        onRemove={vi.fn()}
      />
    );
    const skillCells = [...skillsRender.container.querySelector(".registry-empty").children];
    const skillRegistry = skillsRender.container.querySelector(".registry");
    expect(skillCells).toHaveLength(4);
    expect(skillCells[0].classList.contains("registry-main")).toBe(true);
    expect(skillRegistry?.getAttribute("role")).toBe("table");
    expect(skillRegistry?.querySelectorAll('[role="columnheader"]')).toHaveLength(4);
    expect(skillRegistry?.querySelector(".registry-empty")?.getAttribute("role")).toBe("row");
    expect(skillRegistry?.querySelectorAll('.registry-empty > [role="cell"]')).toHaveLength(4);

    const updatesRender = render(
      <UpdatesView
        skills={[]}
        orderedSkills={[]}
        stats={{ behind: 0, syncable: 0, failed: 0 }}
        updateState={{}}
        getSyncInfo={vi.fn()}
        checkAll={vi.fn()}
        checkOne={vi.fn()}
        updateOne={vi.fn()}
      />
    );
    const updateCells = [...updatesRender.container.querySelector(".registry-empty").children];
    const updateRegistry = updatesRender.container.querySelector(".registry");
    expect(updateCells).toHaveLength(4);
    expect(updateCells[0].classList.contains("registry-main")).toBe(true);
    expect(updateRegistry?.getAttribute("role")).toBe("table");
    expect(updateRegistry?.querySelectorAll('[role="columnheader"]')).toHaveLength(4);
    expect(updateRegistry?.querySelector(".registry-empty")?.getAttribute("role")).toBe("row");
    expect(updateRegistry?.querySelectorAll('.registry-empty > [role="cell"]')).toHaveLength(4);
  });

  it("平台空状态复用登记册与统一页面标题", () => {
    const { container } = render(<AssistantsView assistants={[]} installedCount={0} />);

    expect(container.querySelector(".surface-header .surface-meta")?.textContent).toBe("已发现 0 / 0");
    expect(container.querySelector(".assistant-registry .registry-empty")?.children).toHaveLength(4);
    expect(container.querySelector(".assistant-registry")?.getAttribute("role")).toBe("table");
    expect(container.querySelectorAll('.assistant-registry [role="columnheader"]')).toHaveLength(4);
    expect(container.querySelectorAll('.assistant-registry .registry-empty > [role="cell"]')).toHaveLength(4);
    expect(screen.getByText("尚未发现平台")).toBeTruthy();
  });

  it("来源签章由稳定来源类型决定样式", () => {
    const unmanagedRender = render(
      <SkillsView
        skills={[{
          path: "/tmp/.agents/skills/manual",
          name: "manual",
          source: "未托管",
          source_type: "unknown",
          managed_by_app: false,
          tags: [],
          size: "1 KB",
          ai: "Codex",
          aiIcon: "codex",
          structure_status: "complete",
          structure_warnings: [],
        }]}
        allSkillCount={1}
        selectedTagCount={0}
        tags={[]}
        onInstall={vi.fn()}
        onClearFilters={vi.fn()}
        onEditTags={vi.fn()}
        onOpenDirectory={vi.fn()}
        onPreview={vi.fn()}
        onUnlink={vi.fn()}
        onRemove={vi.fn()}
      />
    );
    expect(unmanagedRender.container.querySelector(".stamp-source")?.classList.contains("unmanaged")).toBe(true);
    expect(unmanagedRender.container.querySelectorAll('.registry-row[role="row"] > [role="cell"]')).toHaveLength(4);
    unmanagedRender.unmount();

    const skill = { path: "/tmp/.agents/skills/package", name: "package" };
    const packageRender = render(
      <UpdatesView
        skills={[skill]}
        orderedSkills={[skill]}
        stats={{ behind: 0, syncable: 1, failed: 0 }}
        updateState={{}}
        getSyncInfo={() => ({
          originKind: "legacy_npm",
          originLocator: "example-package",
          installedRef: "1.0.0",
          latestRef: "1.0.0",
          syncState: "current",
          canSync: true,
        })}
        checkAll={vi.fn()}
        checkOne={vi.fn()}
        updateOne={vi.fn()}
      />
    );
    expect(packageRender.container.querySelector(".stamp-source")?.classList.contains("npm")).toBe(true);
    expect(packageRender.container.querySelectorAll('.registry-row[role="row"] > [role="cell"]')).toHaveLength(4);
  });
});

describe("安装流程交互", () => {
  it("手动选择来源后不再被自动识别结果覆盖", async () => {
    vi.useFakeTimers();
    const detection = {
      detector: "rules",
      source_kind: "git",
      normalized_source: "git",
      original_input: "owner/repo",
      confidence: "high",
      warnings: [],
      needs_model: false,
    };
    invoke.mockImplementation(async (command) => {
      if (command === "detect_install_source") return detection;
      throw new Error(`未处理命令: ${command}`);
    });
    const { result, unmount } = renderHook(() => useInstallFlow({
      installOpen: true,
      assistants: [],
      setInstallOpen: vi.fn(),
      showToast: vi.fn(),
      loadData: vi.fn(),
      setLoading: vi.fn(),
    }));

    try {
      act(() => result.current.source.setPackage("owner/repo"));
      await act(async () => vi.advanceTimersByTimeAsync(250));
      expect(result.current.source.kind).toBe("git");

      act(() => result.current.source.setKind("local"));
      await act(async () => vi.advanceTimersByTimeAsync(250));

      expect(result.current.source.kind).toBe("local");
    } finally {
      unmount();
      vi.useRealTimers();
    }
  });

  it("在执行信息中展示安装策略阻止原因", async () => {
    const user = userEvent.setup();
    const flow = installFlow();
    const setDetailsOpen = flow.disclosure.setDetailsOpen;
    const { rerender } = render(
      <InstallModal flow={flow} assistants={[{ name: "Codex" }]} loading={false} onClose={vi.fn()} />
    );

    await user.click(screen.getByRole("button", { name: "查看执行信息" }));
    expect(setDetailsOpen).toHaveBeenCalledWith(true);

    flow.disclosure.detailsOpen = true;
    rerender(<InstallModal flow={flow} assistants={[{ name: "Codex" }]} loading={false} onClose={vi.fn()} />);
    expect(screen.getByText("安装策略阻止了 1 项风险")).toBeTruthy();
    expect(screen.getByText(/Git 主机 example.com 不在信任列表/)).toBeTruthy();
  });

  it("共享 Skill 删除动作携带全部受影响助手", async () => {
    const user = userEvent.setup();
    const onRemove = vi.fn();
    const availableIn = [
      { name: "Codex", icon: "codex" },
      { name: "Gemini CLI", icon: "gemini" },
    ];
    render(
      <SkillsView
        skills={[{
          path: "/tmp/.agents/skills/writer",
          name: "writer",
          source: "Git",
          source_type: "git",
          managed_by_app: true,
          tags: [],
          size: "1 KB",
          ai: "Codex",
          aiIcon: "codex",
          availableIn,
          structure_status: "complete",
          structure_warnings: [],
        }]}
        allSkillCount={1}
        selectedTagCount={0}
        tags={[]}
        onInstall={vi.fn()}
        onClearFilters={vi.fn()}
        onEditTags={vi.fn()}
        onOpenDirectory={vi.fn()}
        onPreview={vi.fn()}
        onUnlink={vi.fn()}
        onRemove={onRemove}
      />
    );

    expect(screen.getByText("共享 2")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "writer 的更多操作" }));
    await user.click(screen.getByRole("button", { name: "删除 writer" }));
    expect(onRemove).toHaveBeenCalledWith("/tmp/.agents/skills/writer", "writer", availableIn);
  });

  it("风险条目提供明确审查动作，批量标签复用当前选择", async () => {
    const user = userEvent.setup();
    const onPreview = vi.fn();
    const onEditTags = vi.fn();
    const skills = [
      {
        path: "/tmp/.agents/skills/risky",
        name: "risky",
        source: "Git",
        source_type: "git",
        managed_by_app: true,
        tags: [],
        size: "2 KB",
        ai: "Codex",
        aiIcon: "codex",
        structure_status: "nonstandard",
        structure_warnings: ["script_execution"],
      },
      {
        path: "/tmp/.agents/skills/writer",
        name: "writer",
        source: "Git",
        source_type: "git",
        managed_by_app: true,
        tags: [],
        size: "1 KB",
        ai: "Codex",
        aiIcon: "codex",
        structure_status: "complete",
        structure_warnings: [],
      },
    ];
    render(
      <SkillsView
        skills={skills}
        allSkillCount={2}
        selectedTagCount={0}
        tags={[]}
        selectedSkillPaths={skills.map((skill) => skill.path)}
        onToggleSelection={vi.fn()}
        onToggleVisibleSelection={vi.fn()}
        onClearSelection={vi.fn()}
        onInstall={vi.fn()}
        onClearFilters={vi.fn()}
        onEditTags={onEditTags}
        onOpenDirectory={vi.fn()}
        onPreview={onPreview}
        onUnlink={vi.fn()}
        onRemove={vi.fn()}
      />
    );

    await user.click(screen.getByRole("button", { name: "审查 risky" }));
    expect(onPreview).toHaveBeenCalledWith("/tmp/.agents/skills/risky");

    await user.click(screen.getByRole("button", { name: "批量添加标签" }));
    expect(onEditTags).toHaveBeenCalledWith(skills);
  });

  it("筛选后批量标签仍处理完整选择集", async () => {
    const user = userEvent.setup();
    const onEditTags = vi.fn();
    const allSkills = [
      {
        path: "/tmp/.agents/skills/hidden",
        name: "hidden",
        source: "Git",
        source_type: "git",
        managed_by_app: true,
        tags: [],
        size: "1 KB",
        ai: "Codex",
        aiIcon: "codex",
        structure_status: "complete",
        structure_warnings: [],
      },
      {
        path: "/tmp/.agents/skills/visible",
        name: "visible",
        source: "Git",
        source_type: "git",
        managed_by_app: true,
        tags: [],
        size: "1 KB",
        ai: "Codex",
        aiIcon: "codex",
        structure_status: "complete",
        structure_warnings: [],
      },
    ];
    render(
      <SkillsView
        skills={[allSkills[1]]}
        allSkills={allSkills}
        allSkillCount={2}
        selectedTagCount={0}
        tags={[]}
        selectedSkillPaths={allSkills.map((skill) => skill.path)}
        onToggleSelection={vi.fn()}
        onToggleVisibleSelection={vi.fn()}
        onClearSelection={vi.fn()}
        onInstall={vi.fn()}
        onClearFilters={vi.fn()}
        onEditTags={onEditTags}
        onOpenDirectory={vi.fn()}
        onPreview={vi.fn()}
        onUnlink={vi.fn()}
        onRemove={vi.fn()}
      />
    );

    expect(screen.getByText("已选择 2 个 Skill")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "批量添加标签" }));
    expect(onEditTags).toHaveBeenCalledWith(allSkills);
  });
});

describe("安装策略设置", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("保存时通过类型稳定的 config 参数调用 IPC", async () => {
    invoke
      .mockResolvedValueOnce({
        mode: "off",
        block_risky_content: false,
        trusted_git_hosts: [],
        trusted_local_roots: [],
      })
      .mockImplementationOnce(async (_command, args) => args.config);
    const showToast = vi.fn();
    const { result } = renderHook(() => useInstallPolicyFlow({ showToast }));
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => {
      result.current.update("mode", "trusted-only");
      result.current.update("trusted_git_hosts", ["github.com"]);
    });
    await act(async () => {
      await result.current.save();
    });

    expect(invoke).toHaveBeenLastCalledWith("set_install_policy", {
      config: {
        mode: "trusted-only",
        block_risky_content: false,
        trusted_git_hosts: ["github.com"],
        trusted_local_roots: [],
      },
    });
  });

  it("设置页把可信来源输入映射为结构化列表", async () => {
    const update = vi.fn();
    render(
      <SettingsView
        activeTab="install-policy"
        setActiveTab={vi.fn()}
        installPolicy={{
          policy: {
            mode: "trusted-only",
            block_risky_content: false,
            trusted_git_hosts: [],
            trusted_local_roots: [],
          },
          update,
          save: vi.fn(),
          reload: vi.fn(),
          dirty: true,
          loading: false,
          saving: false,
          error: "",
        }}
      />
    );

    fireEvent.change(screen.getByLabelText("可信 Git 主机"), {
      target: { value: "github.com, gitlab.com" },
    });
    expect(update).toHaveBeenLastCalledWith("trusted_git_hosts", ["github.com", "gitlab.com"]);
  });
});

describe("场景与 Git 备份流程", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("组合选择器完整展示 Skill 名称并按平台分组", async () => {
    const togglePath = vi.fn();
    render(
      <ScenarioView
        scenarios={[]}
        skills={[
          {
            name: "gh-address-comments",
            path: "/Users/demo/.codex/skills/gh-address-comments",
            ai: "Codex",
            aiIcon: "codex",
          },
          {
            name: "cloudflare-deploy",
            path: "/Users/demo/.codex/skills/cloudflare-deploy",
            ai: "Codex",
            aiIcon: "codex",
          },
          {
            name: "impeccable",
            path: "/Users/demo/.agents/skills/impeccable",
            ai: "Claude Code",
            aiIcon: "claude",
          },
          {
            name: "shared-writer",
            path: "/Users/demo/.agents/skills/shared-writer",
            ai: "Codex",
            aiIcon: "codex",
            availableIn: [
              { name: "Codex", icon: "codex" },
              { name: "Gemini CLI", icon: "gemini" },
            ],
          },
        ]}
        flow={{
          editor: {
            name: "",
            setName: vi.fn(),
            description: "",
            setDescription: vi.fn(),
            manualInput: "",
            setManualInput: vi.fn(),
            selectedPaths: [],
            togglePath,
            clear: vi.fn(),
            create: vi.fn(),
          },
        }}
      />
    );

    expect(screen.getByText("gh-address-comments")).toBeTruthy();
    expect(screen.getByText("~/.codex/skills/gh-address-comments")).toBeTruthy();
    expect(screen.getByRole("group", { name: "Codex" })).toBeTruthy();
    expect(screen.getByRole("group", { name: "Claude Code" })).toBeTruthy();
    expect(screen.getByRole("group", { name: "多个平台共用" })).toBeTruthy();
    expect(screen.getByText("Codex、Gemini CLI")).toBeTruthy();
    expect(screen.getAllByText("添加")).toHaveLength(4);

    await userEvent.click(screen.getByRole("checkbox", { name: /gh-address-comments/ }));
    expect(togglePath).toHaveBeenCalledWith("/Users/demo/.codex/skills/gh-address-comments");
  });

  it("场景 Hook 通过稳定路径创建场景并刷新数据", async () => {
    invoke.mockResolvedValue(undefined);
    const showToast = vi.fn();
    const loadData = vi.fn().mockResolvedValue(undefined);
    const setView = vi.fn();
    const skills = [{ path: "/tmp/writer", name: "writer" }];
    const { result } = renderHook(() => useScenarioFlow({
      scenarios: [],
      allSkills: skills,
      selectableSkills: skills,
      showToast,
      loadData,
      setView,
    }));

    act(() => {
      result.current.editor.setName("写作");
      result.current.editor.togglePath("/tmp/writer");
    });
    await act(async () => {
      await result.current.editor.create();
    });

    expect(invoke).toHaveBeenCalledWith("create_scenario", {
      name: "写作",
      description: "自动生成组合",
      skillIds: ["/tmp/writer"],
    });
    expect(loadData).toHaveBeenCalledOnce();
    expect(setView).toHaveBeenLastCalledWith("scenarios");
  });

  it("Git 备份 Hook 在草稿未保存时阻止同步", async () => {
    const showToast = vi.fn();
    const { result } = renderHook(() => useGitBackupFlow({
      saved: { repo_path: "/tmp/old", remote_url: "", branch: "main" },
      showToast,
      loadData: vi.fn(),
    }));

    act(() => result.current.setRepoPath("/tmp/new"));
    await act(async () => result.current.sync());

    expect(invoke).not.toHaveBeenCalledWith("sync_to_git", expect.anything());
    expect(showToast).toHaveBeenLastCalledWith(
      "Git 备份设置尚未保存，请先保存后再同步",
      "warn"
    );
  });

  it("刷新已保存配置时保留未保存的 Git 备份草稿", () => {
    const { result } = renderHook(() => useGitBackupFlow({
      saved: { repo_path: "/tmp/old", remote_url: "", branch: "main" },
      showToast: vi.fn(),
      loadData: vi.fn(),
    }));

    act(() => result.current.setRepoPath("/tmp/draft"));
    act(() => result.current.hydrate({
      repo_path: "/tmp/refreshed",
      remote_url: "git@example.com:skills.git",
      branch: "stable",
    }));

    expect(result.current.repoPath).toBe("/tmp/draft");
    expect(result.current.branch).toBe("main");
    expect(result.current.dirty).toBe(true);
  });
});

describe("搜索流程", () => {
  it("清空搜索时取消尚未执行的防抖更新", () => {
    vi.useFakeTimers();
    const { result, unmount } = renderHook(() => useSearchFlow());

    try {
      act(() => result.current.update("writer"));
      act(() => vi.advanceTimersByTime(200));
      expect(result.current.query).toBe("writer");

      act(() => result.current.update("reader"));
      act(() => result.current.clear());
      act(() => vi.advanceTimersByTime(200));

      expect(result.current.input).toBe("");
      expect(result.current.query).toBe("");
    } finally {
      unmount();
      vi.useRealTimers();
    }
  });
});

describe("应用更新流程", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updaterMocks.app.getVersion.mockResolvedValue("0.0.7");
  });

  function renderAppUpdateFlow() {
    const showToast = vi.fn();
    const { result, unmount } = renderHook(() => useAppUpdateFlow({ showToast }));
    return { showToast, result, unmount };
  }

  it("启动后自动静默检查,发现新版本时提示一次且不重复检查", async () => {
    vi.useFakeTimers();
    updaterMocks.updater.check.mockResolvedValue({
      currentVersion: "0.0.7",
      version: "0.1.0",
      date: "2026-08-15",
      body: "release notes",
    });

    const { showToast, unmount } = renderAppUpdateFlow();
    try {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });

      expect(updaterMocks.updater.check).toHaveBeenCalledTimes(1);
      expect(showToast).toHaveBeenCalledTimes(1);
      expect(showToast).toHaveBeenCalledWith("发现新版本 0.1.0,可在设置中更新", "success");

      await act(async () => {
        await vi.advanceTimersByTimeAsync(10_000);
      });
      expect(updaterMocks.updater.check).toHaveBeenCalledTimes(1);
    } finally {
      unmount();
      vi.useRealTimers();
    }
  });

  it("启动自动检查已是最新或失败时保持静默", async () => {
    vi.useFakeTimers();
    updaterMocks.updater.check
      .mockResolvedValueOnce(null)
      .mockRejectedValueOnce(new Error("network down"));

    const { showToast, unmount } = renderAppUpdateFlow();
    try {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(showToast).not.toHaveBeenCalled();

      unmount();
      const second = renderAppUpdateFlow();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(second.showToast).not.toHaveBeenCalled();
      second.unmount();
    } finally {
      vi.useRealTimers();
    }
  });

  it("启动检查失败不影响设置页手动检查展示错误", async () => {
    vi.useFakeTimers();
    updaterMocks.updater.check
      .mockRejectedValueOnce(new Error("network down"))
      .mockResolvedValueOnce(null);

    const { result, showToast, unmount } = renderAppUpdateFlow();
    try {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(showToast).not.toHaveBeenCalled();

      await act(async () => {
        await result.current.checkAppUpdate();
      });
      expect(result.current.appUpdateState.status).toBe("current");
      expect(showToast).toHaveBeenCalledWith("当前已是最新版本", "success");
    } finally {
      unmount();
      vi.useRealTimers();
    }
  });

  it("忽略托盘或按钮重复触发的并发更新检查", async () => {
    let finishCheck;
    updaterMocks.updater.check.mockImplementation(() => new Promise((resolve) => {
      finishCheck = resolve;
    }));

    const { result, unmount } = renderAppUpdateFlow();
    try {
      await waitFor(() => expect(result.current.appUpdateState.currentVersion).toBe("0.0.7"));
      let firstCheck;
      await act(async () => {
        firstCheck = result.current.checkAppUpdate();
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
      expect(result.current.appUpdateState.status).toBe("checking");
      await waitFor(() => expect(updaterMocks.updater.check).toHaveBeenCalledTimes(1));

      let secondCheck;
      await act(async () => {
        secondCheck = result.current.checkAppUpdate();
        await Promise.resolve();
      });
      expect(updaterMocks.updater.check).toHaveBeenCalledTimes(1);

      await act(async () => {
        finishCheck(null);
        await Promise.all([firstCheck, secondCheck]);
      });
      expect(result.current.appUpdateState.status).toBe("current");
    } finally {
      unmount();
    }
  });

  it("手动检查会复用尚未完成的启动检查", async () => {
    vi.useFakeTimers();
    let finishCheck;
    const update = {
      currentVersion: "0.0.7",
      version: "0.1.0",
      date: "2026-03-21T00:00:00Z",
      body: "notes",
    };
    updaterMocks.updater.check.mockImplementation(() => new Promise((resolve) => {
      finishCheck = resolve;
    }));

    const { result, showToast, unmount } = renderAppUpdateFlow();
    try {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(updaterMocks.updater.check).toHaveBeenCalledTimes(1);

      let manualCheck;
      let settled = false;
      await act(async () => {
        manualCheck = result.current.checkAppUpdate();
        manualCheck.then(() => {
          settled = true;
        });
        await Promise.resolve();
      });
      expect(settled).toBe(false);
      expect(updaterMocks.updater.check).toHaveBeenCalledTimes(1);

      let checkedUpdate;
      await act(async () => {
        finishCheck(update);
        checkedUpdate = await manualCheck;
      });
      expect(checkedUpdate).toBe(update);
      expect(result.current.appUpdateState.status).toBe("available");
      expect(showToast).toHaveBeenCalledWith("发现新版本 0.1.0", "success");
    } finally {
      unmount();
      vi.useRealTimers();
    }
  });
});
