import React from "react";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useImportExportFlow } from "./useImportExportFlow.js";
import { I18nProvider } from "./i18n.jsx";

const api = vi.hoisted(() => ({
  profilesGet: vi.fn(),
  libraryExport: vi.fn(),
  libraryPreviewImport: vi.fn(),
  runCommand: vi.fn(),
}));
vi.mock("./skillmateApi.js", () => ({
  invokeSkillMateCommand: api.runCommand,
  skillmateCommands: {
    importLibrary: "import_library",
    applySkillMateManifest: "apply_skillmate_manifest",
    applySkillProfile: "apply_skill_profile",
    importScenarioManifest: "import_scenario_manifest",
  },
  skillmateApi: {
    library: { export: api.libraryExport, previewImport: api.libraryPreviewImport },
    profiles: { get: api.profilesGet },
  },
}));

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function setup(language = "en") {
  localStorage.setItem("skillmate-language", language);
  const showToast = vi.fn();
  const loadData = vi.fn().mockResolvedValue(undefined);
  const hook = renderHook(() => useImportExportFlow({ showToast, loadData }), { wrapper: I18nProvider });
  return { ...hook, showToast, loadData };
}

async function previewImport(result, mode = "merge", path = "/tmp/skills.json") {
  const hook = setup();
  await act(async () => {
    hook.result.current.library.updateImportPath(path);
    hook.result.current.library.updateImportMode(mode);
  });
  await act(async () => {
    api.libraryPreviewImport.mockResolvedValueOnce(result);
    await hook.result.current.library.previewImportLibraryFile();
  });
  return hook;
}

beforeEach(() => {
  Object.values(api).forEach(mock => mock.mockReset());
  api.profilesGet.mockResolvedValue({ version: 1, active_profile_id: null, profiles: [] });
});

describe("useImportExportFlow library import", () => {
  it("sends the REPLACE-mode payload through the plan executor and refreshes inventory", async () => {
    const { result, loadData, showToast } = await previewImport(
      { plan_token: "plan-replace", replace_existing: true }, "replace");
    expect(api.libraryPreviewImport).toHaveBeenLastCalledWith("/tmp/skills.json", "replace");
    expect(result.current.library.importPreviewCurrent).toBe(true);

    const run = deferred();
    api.runCommand.mockReturnValueOnce(run.promise);
    let applying;
    act(() => { applying = result.current.library.importLibraryFile(); });
    await act(async () => { run.resolve("完成"); await applying; });

    expect(api.runCommand).toHaveBeenCalledExactlyOnceWith("import_library", {
      path: "/tmp/skills.json",
      mode: "replace",
      planToken: "plan-replace",
    });
    expect(result.current.library.importPreview).toBeNull();
    expect(result.current.library.applyingImport).toBe(false);
    expect(loadData).toHaveBeenCalledOnce();
    expect(showToast).toHaveBeenLastCalledWith("Organization data imported", "success");
  });

  it("does not execute a stale preview (isImportPreviewCurrent false) and warns instead", async () => {
    const { result, showToast } = await previewImport({ plan_token: "" });
    expect(result.current.library.importPreview).not.toBeNull();
    expect(result.current.library.importPreviewCurrent).toBe(false);

    await act(async () => { await result.current.library.importLibraryFile(); });

    expect(api.runCommand).not.toHaveBeenCalled();
    expect(result.current.library.applyingImport).toBe(false);
    expect(showToast).toHaveBeenLastCalledWith("Import settings changed; preview again", "warn");
  });

  it("rejects a second execute while one import is in flight and runs the command once", async () => {
    const { result, loadData, showToast } = await previewImport({ plan_token: "plan-1" });
    showToast.mockClear();
    const pending = deferred();
    api.runCommand.mockReturnValueOnce(pending.promise);
    let first, second;
    act(() => {
      first = result.current.library.importLibraryFile();
      second = result.current.library.importLibraryFile();
    });
    // 执行器通过微任务真正调用命令，先冲刷一个 tick 再断言单飞生效。
    await act(async () => { await Promise.resolve(); });
    expect(api.runCommand).toHaveBeenCalledTimes(1);
    expect(showToast).not.toHaveBeenCalled();

    await act(async () => { pending.resolve("done"); await Promise.allSettled([first, second]); });
    expect(api.runCommand).toHaveBeenCalledTimes(1);
    expect(showToast).toHaveBeenCalledTimes(1);
    expect(showToast).toHaveBeenLastCalledWith("Organization data imported", "success");
    expect(loadData).toHaveBeenCalledTimes(1);
  });

  it("surfaces the executor error instead of returning silently when the plan cannot start", async () => {
    const { result, showToast } = await previewImport({ plan_token: "   " });
    expect(result.current.library.importPreviewCurrent).toBe(true);

    await act(async () => { await result.current.library.importLibraryFile(); });

    expect(api.runCommand).not.toHaveBeenCalled();
    expect(result.current.library.applyingImport).toBe(false);
    const [message, tone] = showToast.mock.lastCall;
    expect(tone).toBe("error");
    expect(message).toContain("操作计划缺失，请重新预览");
  });
});

describe("useImportExportFlow messages and lifecycle", () => {
  it("falls back to the localized success message when the backend result is not a usable string", async () => {
    const { result, showToast } = setup();
    await act(async () => { result.current.library.setExportPath("/tmp/export.json"); });
    await act(async () => {
      api.libraryExport.mockResolvedValueOnce({ code: 0 });
      await result.current.library.exportLibraryFile();
    });
    expect(api.libraryExport).toHaveBeenCalledWith("/tmp/export.json");
    expect(showToast).toHaveBeenLastCalledWith("Organization data exported", "success");
  });

  it("keeps a non-empty backend string as the success message", async () => {
    const { result, showToast } = setup("zh-CN");
    await act(async () => { result.current.library.setExportPath("/tmp/export.json"); });
    await act(async () => {
      api.libraryExport.mockResolvedValueOnce("已导出到 /tmp/export.json");
      await result.current.library.exportLibraryFile();
    });
    expect(showToast).toHaveBeenLastCalledWith("已导出到 /tmp/export.json", "success");
  });

  it("reports import failures through the sanitized error message", async () => {
    const { result, showToast } = await previewImport({ plan_token: "plan-1" });
    api.runCommand.mockRejectedValueOnce(new TypeError("Cannot read properties of invoke"));
    await act(async () => { await result.current.library.importLibraryFile(); });
    const [message, tone] = showToast.mock.lastCall;
    expect(tone).toBe("error");
    expect(message).not.toContain("TypeError");
    expect(message).toContain("The operation did not finish; retry");
  });

  it("suppresses profile load side effects after unmount", async () => {
    const pending = deferred();
    api.profilesGet.mockReturnValueOnce(pending.promise);
    const { unmount, result, showToast } = setup();
    unmount();
    await act(async () => { pending.reject(new Error("IPC offline")); await pending.promise.catch(() => {}); });
    expect(showToast).not.toHaveBeenCalled();
    expect(result.current.profiles.store.profiles).toEqual([]);
  });

  it("loads skill profiles on mount", async () => {
    api.profilesGet.mockResolvedValueOnce({ version: 1, active_profile_id: "p1", profiles: [{ id: "p1" }] });
    const { result } = setup();
    await act(async () => {});
    expect(api.profilesGet).toHaveBeenCalledTimes(1);
    expect(result.current.profiles.store).toEqual({ version: 1, active_profile_id: "p1", profiles: [{ id: "p1" }] });
  });
});
