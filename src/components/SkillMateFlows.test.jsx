import React from "react";
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SkillsView } from "./InventoryViews.jsx";
import { InstallModal, PreviewModal } from "./SkillMateModals.jsx";
import SettingsView from "./SettingsView.jsx";
import { useInstallPolicyFlow } from "../lib/useInstallPolicyFlow.js";
import { useGitBackupFlow } from "../lib/useGitBackupFlow.js";
import { useScenarioFlow } from "../lib/useScenarioFlow.js";
import { skillmateApi } from "../lib/skillmateApi.js";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args) => invoke(...args),
}));

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
});
