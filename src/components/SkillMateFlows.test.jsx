import React from "react";
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SkillsView } from "./InventoryViews.jsx";
import { InstallModal, PreviewModal } from "./SkillMateModals.jsx";
import SettingsView from "./SettingsView.jsx";
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
    await user.click(screen.getByRole("button", { name: "删除 writer" }));
    expect(onRemove).toHaveBeenCalledWith("/tmp/.agents/skills/writer", "writer", availableIn);
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
      description: "自动生成场景",
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
});
