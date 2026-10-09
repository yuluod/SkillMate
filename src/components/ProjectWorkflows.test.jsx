import React from "react";
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { I18nProvider } from "../lib/i18n.jsx";
import { useProjectWorkspace } from "../lib/useProjectWorkspace.js";
import { useScenarioFlow } from "../lib/useScenarioFlow.js";
import { useInstallFlow } from "../lib/useInstallFlow.js";
import ProjectWorkspaceBar from "./ProjectWorkspaceBar.jsx";
import ScenarioView from "./ScenarioView.jsx";
import UpdatePreviewModal from "./UpdatePreviewModal.jsx";
import { AssistantsView } from "./InventoryViews.jsx";

const { invoke, open } = vi.hoisted(() => ({ invoke: vi.fn(), open: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));
const wrapper = I18nProvider;
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
beforeEach(() => { localStorage.clear(); localStorage.setItem("skillmate-language", "zh-CN"); invoke.mockReset(); open.mockReset(); });

it("remembers only successfully inspected projects and persists pins", () => {
  const { result, unmount } = renderHook(useProjectWorkspace);
  act(() => result.current.select(" /not-yet-inspected "));
  expect(result.current.projects).toEqual([]);
  act(() => { result.current.remember("/one"); result.current.togglePin("/one"); result.current.remember("/two"); });
  expect(result.current.projects[0]).toEqual({ path: "/one", pinned: true });
  unmount();
  const restored = renderHook(useProjectWorkspace);
  expect(restored.result.current.projects).toHaveLength(2);
  act(() => restored.result.current.forget("/one"));
  expect(restored.result.current.projects.map(item => item.path)).toEqual(["/two"]);
});

it("opens a project from the directory picker", async () => {
  const select = vi.fn(); const onOpen = vi.fn();
  open.mockResolvedValue("/project");
  render(<ProjectWorkspaceBar workspace={{ path: "", projects: [], select }} onOpen={onOpen} />, { wrapper });
  fireEvent.click(screen.getByText("选择目录"));
  await waitFor(() => expect(select).toHaveBeenCalledWith("/project"));
  expect(onOpen).toHaveBeenCalledOnce();
});

it("ignores a late inspection when switching project", async () => {
  const first = deferred(); const second = deferred(); const remember = vi.fn();
  invoke.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const { rerender } = render(<AssistantsView assistants={[]} workspace={{ path: "/old", request: 1, remember }} />, { wrapper });
  rerender(<AssistantsView assistants={[]} workspace={{ path: "/new", request: 2, remember }} />);
  await act(async () => second.resolve({ project_path: "/new", assistants: [] }));
  await act(async () => first.resolve({ project_path: "/old", assistants: [] }));
  expect(remember).toHaveBeenCalledTimes(1);
  expect(remember).toHaveBeenCalledWith("/new");
});

it.each(["ready", "missing", "external"])("enables only complete library scenarios: %s", state => {
  const skill = { path: "/library/a", name: "a", in_library: state !== "external" };
  const skills = state === "missing" ? [] : [skill];
  const scenario = { id: "review", name: "Review", skill_ids: [skill.path] };
  const onEnable = vi.fn(); const setView = vi.fn();
  function Harness() {
    const flow = useScenarioFlow({ scenarios: [scenario], allSkills: skills, selectableSkills: skills, showToast: vi.fn(), loadData: vi.fn(), setView });
    return <ScenarioView scenarios={[scenario]} skills={skills} flow={flow} onEnable={onEnable} projectPath="/project" />;
  }
  render(<Harness />, { wrapper });
  const button = screen.getByRole("button", { name: "启用此场景" });
  expect(button.disabled).toBe(state !== "ready");
  fireEvent.click(button);
  if (state === "ready") expect(onEnable).toHaveBeenCalledWith([skill]);
  else expect(onEnable).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled(); // Opening a preview never writes.
  fireEvent.click(screen.getByRole("button", { name: "查看组合" }));
  expect(setView).toHaveBeenCalledWith("skills");
});

it("uses a separate plan per scenario target, prevents duplicate batches, and reports partial results", async () => {
  const first = deferred(); const refresh = deferred();
  const showToast = vi.fn(); const setLoading = vi.fn();
  const loadData = vi.fn(() => refresh.promise); const setInstallOpen = vi.fn();
  const assistants = [{ name: "Codex", supports_project_skills: true }, { name: "Claude Code", supports_project_skills: true }];
  let applied = 0;
  invoke.mockImplementation(async (command, args) => {
    if (command === "preview_install_skill") return {
      can_apply: true, can_install: true, structure_status: "complete", structure_warnings: [],
      package_detection: { detected_skills: [], warnings: [] },
      plan_token: `${args.package}:${args.assistantName}`, target_actions: [], conflicts: [],
    };
    if (command === "install_skill") {
      applied += 1;
      if (applied === 1) return first.promise;
      return { success: applied !== 3, message: applied === 3 ? "target denied" : "done" };
    }
    if (command === "preview_project_skill_targets") return [];
    return {};
  });
  const { result } = renderHook(() => useInstallFlow({ installOpen: true, assistants, showToast, loadData, setInstallOpen, setLoading }), { wrapper });
  act(() => {
    result.current.source.prepare(["/library/a", "/library/b"], "", "local", "enable");
    result.current.target.setMode("symlink");
    result.current.target.setProjectPath("/project");
    result.current.target.toggleAssistant("Claude Code");
  });
  await act(() => result.current.preview.runPrimaryAction());
  expect(applied).toBe(0);
  expect(result.current.preview.primaryAction.action).toBe("install");
  let running;
  await act(async () => { running = result.current.preview.runPrimaryAction(); result.current.preview.runPrimaryAction(); await Promise.resolve(); });
  expect(applied).toBe(1);
  expect(setLoading).toHaveBeenCalledExactlyOnceWith(true);
  await act(async () => first.resolve({ success: true }));
  expect(applied).toBe(4);
  expect(setLoading).not.toHaveBeenCalledWith(false);
  await act(async () => { refresh.resolve(); await running; });
  const writes = invoke.mock.calls.filter(([command]) => command === "install_skill");
  for (const [, args] of writes) {
    expect(args.planToken).toBe(`${args.package}:${args.assistantName}`);
    expect(args.projectPath).toBe("/project");
    expect(args.installMode).toBe("symlink");
  }
  expect(loadData).toHaveBeenCalledOnce();
  expect(setInstallOpen).not.toHaveBeenCalled();
  expect(showToast).toHaveBeenLastCalledWith(expect.stringContaining("target denied"), "error");
  expect(result.current.preview.current).toBe(false);
});

const preview = {
  libraryPath: "/library/a", installedRef: "old", latestRef: "new", canApply: true, planToken: "bound-plan",
  impacts: [{ path: "/project/.agents/skills/a", assistant: "Codex", scope: "project", projectPath: "/project", connected: true }],
  files: [{ path: "SKILL.md", kind: "modified", before: "old text", after: "new text" }], omittedFiles: 0, warnings: [],
};
it("shows update content and locations before confirming once with the plan token", async () => {
  invoke.mockResolvedValue(preview); const pending = deferred(); const onConfirm = vi.fn(() => pending.promise); const onClose = vi.fn();
  render(<UpdatePreviewModal path="/library/a" onConfirm={onConfirm} onClose={onClose} />, { wrapper });
  expect(onConfirm).not.toHaveBeenCalled();
  await screen.findByText("old text");
  expect(screen.getByText("new text")).toBeTruthy();
  expect(screen.getByText("/project/.agents/skills/a")).toBeTruthy();
  const button = screen.getByRole("button", { name: "确认更新这些位置" });
  fireEvent.click(button); fireEvent.click(button);
  expect(onConfirm).toHaveBeenCalledExactlyOnceWith("/library/a", "bound-plan");
  fireEvent.click(screen.getByRole("button", { name: "关闭" }));
  expect(onClose).not.toHaveBeenCalled();
  await act(async () => pending.resolve(true));
  expect(onClose).toHaveBeenCalledOnce();
});

it("invalidates a failed update preview and requires refreshing", async () => {
  invoke.mockResolvedValue(preview); const onConfirm = vi.fn().mockResolvedValue(false);
  render(<UpdatePreviewModal path="/library/a" onConfirm={onConfirm} onClose={vi.fn()} />, { wrapper });
  await screen.findByText("old text");
  fireEvent.click(screen.getByRole("button", { name: "确认更新这些位置" }));
  await screen.findByRole("alert");
  expect(screen.getByRole("button", { name: "确认更新这些位置" }).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "刷新" }));
  await screen.findByText("old text");
  expect(invoke).toHaveBeenCalledTimes(2);
});

it("does not allow applying a blocked policy preview", async () => {
  invoke.mockResolvedValue({ ...preview, canApply: false });
  render(<UpdatePreviewModal path="/library/a" onConfirm={vi.fn()} onClose={vi.fn()} />, { wrapper });
  await screen.findByText("old text");
  expect(screen.getByRole("button", { name: "确认更新这些位置" }).disabled).toBe(true);
});
