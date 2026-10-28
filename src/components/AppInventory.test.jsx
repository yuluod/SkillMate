import React from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "../App.jsx";
import { I18nProvider } from "../lib/i18n.jsx";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}), emit: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(async () => "0.0.17") }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn(async () => null) }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));

const healthy = {
  name: "healthy", path: "/tmp/skillmate/healthy", tags: [], size: "1 KB",
  source_type: "git", origin_kind: "git", managed_by_app: true,
  structure_status: "complete", structure_warnings: [],
  can_check: true, can_sync: false, has_update: false, sync_state: "current",
};
const risky = {
  ...healthy, name: "risky", path: "/tmp/skillmate/risky",
  structure_status: "partial", structure_warnings: ["missing_entry_document"],
  can_sync: true, has_update: true, sync_state: "behind",
};

function start(skills = [healthy, risky], checkedState = "behind") {
  invoke.mockImplementation(async (command, args) => {
    if (command === "get_all_assistants") return [];
    if (command === "get_library_skills") return skills;
    if (["get_all_tags", "get_scenarios", "list_trashed_skills"].includes(command)) return [];
    if (command === "get_git_backup") return { enabled: false, repo_path: "", remote_url: "" };
    if (command === "get_library_settings") return { path: "/tmp/skillmate", configurable: true };
    if (command === "get_install_policy") return { mode: "warn" };
    if (command === "check_updates") return args.paths.map(path => ({
      path, syncState: checkedState, hasUpdate: checkedState === "behind",
      canCheck: true, canSync: checkedState === "behind",
    }));
    throw new Error(`测试未配置命令：${command}`);
  });
  return render(<I18nProvider><App /></I18nProvider>);
}

beforeEach(() => {
  window.history.replaceState({}, "", "/#/skills");
  localStorage.clear();
  localStorage.setItem("skillmate-language", "zh-CN");
  localStorage.setItem("skillmate-auto-check-updates", "false");
  vi.stubGlobal("matchMedia", vi.fn(() => ({
    matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn(),
  })));
  invoke.mockReset();
});
afterEach(() => vi.unstubAllGlobals());

describe("技能库与父页面协作", () => {
  it("概览常用入口显示全部技能，问题入口保留需处理筛选", async () => {
    const user = userEvent.setup();
    start();
    await screen.findByText("healthy");
    const navigation = within(screen.getByRole("navigation"));
    await user.click(navigation.getByRole("button", { name: "概览" }));
    await user.click(within(screen.getByLabelText("开始工作")).getByRole("button", { name: /技能库/ }));
    expect(await screen.findByText("healthy")).toBeTruthy();
    await user.click(navigation.getByRole("button", { name: "概览" }));
    await user.click(screen.getByRole("button", { name: /有 1 个结构问题/ }));
    expect(screen.queryByText("healthy")).toBeNull();
    expect(screen.getByText("risky")).toBeTruthy();
    await user.click(navigation.getByRole("button", { name: "概览" }));
    await user.click(within(screen.getByLabelText("本机 Skill 状态摘要")).getByRole("button", { name: /库中 Skills/ }));
    expect(await screen.findByText("healthy")).toBeTruthy();
  });

  it("状态筛选后的全选与取消只修改可见条目", async () => {
    const user = userEvent.setup();
    start();
    await screen.findByText("healthy");
    await user.click(screen.getByRole("button", { name: /需处理/ }));
    await user.click(screen.getByRole("button", { name: "选择当前结果" }));
    expect(screen.getByText("已选择 1 个 Skill")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "取消选择当前结果" }));
    expect(screen.queryByRole("region", { name: "批量操作" })).toBeNull();
    await user.click(screen.getByRole("button", { name: /全部/ }));
    expect(screen.getByRole("checkbox", { name: "选择 healthy" }).checked).toBe(false);
    expect(screen.getByRole("checkbox", { name: "选择 risky" }).checked).toBe(false);
  });

  it("外部来源有更新时保留说明但不显示受管更新按钮", async () => {
    const user = userEvent.setup();
    start([{ ...risky, managed_by_app: false, can_sync: false, update_strategy: "external_cli" }]);
    const row = (await screen.findByText("risky")).closest('[role="row"]');
    expect(within(row).queryByRole("button", { name: "更新" })).toBeNull();
    await user.click(row);
    const pane = screen.getByRole("complementary", { name: "risky 详情" });
    expect(within(pane).getByText("发现更新，请通过原安装器更新")).toBeTruthy();
    expect(within(pane).queryByRole("button", { name: "更新" })).toBeNull();
  });

  it("检查后返回技能库，更新筛选和详情立即显示实时结果", async () => {
    const user = userEvent.setup();
    start([healthy]);
    await screen.findByText("healthy");
    await user.click(screen.getByRole("button", { name: "可更新", exact: true }));
    await user.click(screen.getByRole("button", { name: "检查更新" }));
    await screen.findByText("全部检查完成");
    await user.click(screen.getByRole("button", { name: /^技能库/ }));
    await user.click(within(screen.getByRole("main")).getByRole("button", { name: /^可更新\s*1$/ }));
    const row = screen.getByText("healthy").closest('[role="row"]');
    expect(within(row).getByRole("button", { name: "更新" })).toBeTruthy();
    await user.click(row);
    expect(screen.getByText("发现更新，可由 SkillMate 安装")).toBeTruthy();
    expect(invoke.mock.calls.filter(([command]) => command === "get_library_skills")).toHaveLength(1);
  });

  it.each([
    ["current", "已是最新，可随时重新检查"],
    ["failed", "上次检查失败，可以重新检查"],
  ])("检查变为 %s 后清除旧更新提示并刷新详情", async (state, summary) => {
    const user = userEvent.setup();
    start([risky], state);
    await screen.findByText("risky");
    await user.click(within(screen.getByRole("navigation")).getByRole("button", { name: /^可更新/ }));
    await user.click(screen.getByRole("button", { name: "检查更新" }));
    await screen.findByText(state === "failed" ? "检查完成，1 个 Skill 未能确认更新状态" : "全部检查完成");
    await user.click(screen.getByRole("button", { name: /^技能库/ }));
    const main = within(screen.getByRole("main"));
    expect(main.getByRole("button", { name: /^可更新\s*0$/ })).toBeTruthy();
    expect(main.queryByRole("button", { name: "更新" })).toBeNull();
    await user.click(screen.getByText("risky").closest('[role="row"]'));
    expect(screen.getByText(summary)).toBeTruthy();
  });
});
