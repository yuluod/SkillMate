import React from "react";
import { act, render, renderHook, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useUpdateFlow } from "./useUpdateFlow.js";
import { adaptUpdateResult, getUpdateInfo } from "./updateState.js";
import { I18nProvider } from "./i18n.jsx";
import { UpdatesView } from "../components/InventoryViews.jsx";

const api = vi.hoisted(() => ({ checkAll: vi.fn(), checkOne: vi.fn(), applyOne: vi.fn() }));
vi.mock("./skillmateApi.js", () => ({ skillmateApi: { updates: api } }));

const a = { path: "/a", name: "a", origin_kind: "git", can_check: true, can_sync: true, managed_by_app: true };
const b = { ...a, path: "/b", name: "b" };
const current = { syncState: "current", hasUpdate: false };
const failed = { syncState: "failed", message: "network failed", originKind: "unknown",
  originLocator: "", managedByApp: false, canCheck: false, canSync: false };
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function setup(skills = [a, b], language = "en") {
  localStorage.setItem("skillmate-language", language);
  const showToast = vi.fn();
  const loadData = vi.fn().mockResolvedValue(undefined);
  const hook = renderHook(({ updatable }) => useUpdateFlow({ updatable, showToast, loadData }), {
    initialProps: { updatable: skills }, wrapper: I18nProvider,
  });
  return { ...hook, showToast, loadData };
}
beforeEach(() => {
  Object.values(api).forEach(mock => mock.mockReset());
  api.checkOne.mockResolvedValue(current);
  api.applyOne.mockResolvedValue("done");
});

describe("update state adapter", () => {
  it("preserves failed-response capabilities and accepts explicit false/empty successful fields", () => {
    const prior = { originKind: "git", originLocator: "repo", canCheck: true, canSync: true, managedByApp: true };
    expect(adaptUpdateResult(prior, failed)).toMatchObject({ ...prior, syncState: "failed" });
    const next = adaptUpdateResult(prior, { ...current, canCheck: false, originLocator: "" });
    expect(getUpdateInfo(a, next)).toMatchObject({ canCheck: false, originLocator: "" });
    expect(prior).not.toHaveProperty("syncState");
  });
});

describe("useUpdateFlow checks", () => {
  it.each(["en", "zh-CN"])("never reports unsuccessful states as current (%s)", async language => {
    const { result, showToast } = setup([a], language);
    for (const syncState of ["failed", "source_missing", "unsupported", "diverged", "unknown"]) {
      api.checkOne.mockResolvedValueOnce({ syncState, hasUpdate: false });
      await act(() => result.current.checkUpdate(a.path));
      expect(showToast.mock.lastCall[1]).not.toBe("success");
      expect(showToast.mock.lastCall[0]).not.toMatch(/Up to date|已是最新/);
    }
  });

  it("retains inventory and previously checked capabilities after failed IPC and rejection", async () => {
    const { result } = setup();
    api.checkOne.mockResolvedValueOnce(failed);
    await act(() => result.current.checkUpdate(a.path));
    expect(result.current.getSyncInfo(a)).toMatchObject({ canCheck: true, canSync: true, managedByApp: true, originKind: "git", syncState: "failed" });
    api.checkOne.mockResolvedValueOnce({ syncState: "behind", canCheck: true, canSync: true, managedByApp: true, originLocator: "checked-repo" });
    await act(() => result.current.checkUpdate(a.path));
    api.checkOne.mockResolvedValueOnce(failed);
    await act(() => result.current.checkUpdate(a.path));
    expect(result.current.getSyncInfo(a)).toMatchObject({ originLocator: "checked-repo", canCheck: true, managedByApp: true });
    api.checkOne.mockRejectedValueOnce(new Error("offline"));
    await act(() => result.current.checkUpdate(a.path));
    expect(result.current.getSyncInfo(a)).toMatchObject({ syncState: "failed", checking: false, canCheck: true });
  });

  it("locks synchronously against repeated checks and apply while checking", async () => {
    const pending = deferred();
    api.checkOne.mockReturnValueOnce(pending.promise);
    const { result } = setup();
    let run;
    act(() => {
      run = result.current.checkUpdate(a.path);
      result.current.checkUpdate(a.path);
      result.current.updateSkill(a.path);
    });
    expect(api.checkOne).toHaveBeenCalledTimes(1);
    expect(api.applyOne).not.toHaveBeenCalled();
    await act(async () => { pending.resolve(current); await run; });
    await act(() => result.current.checkUpdate(a.path));
    expect(api.checkOne).toHaveBeenCalledTimes(2);
  });

  it("deduplicates batch paths/submissions and counts missing results while ignoring extras", async () => {
    const pending = deferred();
    api.checkAll.mockReturnValueOnce(pending.promise);
    const { result, showToast } = setup([a, a, b]);
    let run;
    act(() => {
      run = result.current.checkAllUpdates();
      result.current.checkAllUpdates();
      result.current.checkUpdate(a.path);
      result.current.updateSkill(b.path);
    });
    expect(api.checkAll).toHaveBeenCalledExactlyOnceWith([a.path, b.path]);
    expect(api.checkOne).not.toHaveBeenCalled();
    expect(api.applyOne).not.toHaveBeenCalled();
    await act(async () => {
      pending.resolve([{ path: a.path, ...current }, { path: "/extra", ...failed }]); await run;
    });
    expect(result.current.getSyncInfo(b)).toMatchObject({ syncState: "failed", canCheck: true, checking: false });
    expect(showToast).toHaveBeenLastCalledWith("Check complete; 1 Skills could not be verified", "warn");
    expect(result.current.updateState).not.toHaveProperty("/extra");
  });

  it("batch rejection preserves abilities and releases locks for retry", async () => {
    const { result, showToast } = setup();
    api.checkAll.mockRejectedValueOnce(new Error("offline"));
    await act(() => result.current.checkAllUpdates());
    expect(result.current.getSyncInfo(a)).toMatchObject({ syncState: "failed", canCheck: true, managedByApp: true, checking: false });
    expect(showToast.mock.lastCall[1]).toBe("error");
    api.checkAll.mockResolvedValueOnce([{ path: a.path, ...failed }, { path: b.path, syncState: "unsupported" }]);
    await act(() => result.current.checkAllUpdates());
    expect(showToast).toHaveBeenLastCalledWith("Check complete; 2 Skills could not be verified", "warn");
  });

  it.each([true, false])("interleaved apply and batch preserve independent busy state (batch first finishes: %s)", async batchFirst => {
    const apply = deferred(), batch = deferred();
    api.applyOne.mockReturnValueOnce(apply.promise);
    api.checkAll.mockReturnValueOnce(batch.promise);
    const { result } = setup();
    let applying, checking;
    act(() => { applying = result.current.updateSkill(a.path); checking = result.current.checkAllUpdates(); });
    expect(api.checkAll).toHaveBeenCalledWith([b.path]);
    const finishBatch = async () => { batch.resolve([{ path: b.path, ...current }, { path: a.path, ...failed }]); await checking; };
    const finishApply = async () => { apply.resolve("done"); await applying; };
    await act(batchFirst ? finishBatch : finishApply);
    expect(result.current.getSyncInfo(batchFirst ? a : b)).toMatchObject(batchFirst ? { updating: true } : { checking: true });
    await act(batchFirst ? finishApply : finishBatch);
    expect(result.current.getSyncInfo(a)).toMatchObject({ syncState: "current", updating: false });
  });
});

describe("useUpdateFlow apply and lifecycle", () => {
  it("holds apply lock through follow-up check and refresh, even across rerenders", async () => {
    const apply = deferred(), probe = deferred(), refresh = deferred();
    api.applyOne.mockReturnValueOnce(apply.promise);
    api.checkOne.mockReturnValueOnce(probe.promise);
    const { result, loadData, rerender } = setup();
    loadData.mockReturnValueOnce(refresh.promise);
    let run;
    act(() => { run = result.current.updateSkill(a.path); result.current.updateSkill(a.path); });
    rerender({ updatable: [b] });
    await act(async () => { apply.resolve("done"); await apply.promise; });
    act(() => { result.current.updateSkill(a.path); result.current.checkUpdate(a.path); });
    expect(result.current.getSyncInfo(a)).toMatchObject({ updating: true, checking: true });
    await act(async () => { probe.resolve(current); await probe.promise; });
    act(() => { result.current.updateSkill(a.path); });
    expect(api.applyOne).toHaveBeenCalledTimes(1);
    expect(api.checkOne).toHaveBeenCalledTimes(1);
    expect(loadData).toHaveBeenCalledExactlyOnceWith({ resetUpdates: false });
    await act(async () => { refresh.resolve(); await run; });
    expect(result.current.getSyncInfo(a)).toMatchObject({ updating: false, checking: false });
  });

  it("apply rejection releases lock; failed follow-up still refreshes inventory", async () => {
    const { result, loadData, showToast } = setup();
    api.applyOne.mockRejectedValueOnce(new Error("write failed"));
    await act(() => result.current.updateSkill(a.path));
    expect(showToast.mock.lastCall[1]).toBe("error");
    expect(loadData).not.toHaveBeenCalled();
    expect(result.current.getSyncInfo(a).updating).toBe(false);
    api.checkOne.mockRejectedValueOnce(new Error("probe failed"));
    await act(() => result.current.updateSkill(a.path));
    expect(result.current.getSyncInfo(a)).toMatchObject({ syncState: "failed", updating: false, checking: false });
    expect(loadData).toHaveBeenCalledTimes(1);
  });

  it.each(["check", "batch", "apply"])("reset invalidates late %s results and side effects without unlocking running IPC", async kind => {
    const pending = deferred();
    const mock = kind === "check" ? api.checkOne : kind === "batch" ? api.checkAll : api.applyOne;
    mock.mockReturnValueOnce(pending.promise);
    const { result, showToast, loadData } = setup();
    const start = () => kind === "check" ? result.current.checkUpdate(a.path) : kind === "batch" ? result.current.checkAllUpdates() : result.current.updateSkill(a.path);
    let run;
    act(() => { run = start(); result.current.resetUpdateState(); start(); });
    expect(mock).toHaveBeenCalledTimes(1);
    expect(result.current.updateState).toEqual({});
    await act(async () => { pending.resolve(kind === "batch" ? [{ path: a.path, ...current }] : current); await run; });
    expect(result.current.updateState).toEqual({});
    expect(showToast).not.toHaveBeenCalled();
    expect(loadData).not.toHaveBeenCalled();
    if (kind === "apply") expect(api.checkOne).not.toHaveBeenCalled();
    mock.mockResolvedValueOnce(kind === "batch" ? [a, b].map(skill => ({ path: skill.path, ...current })) : current);
    await act(start);
    expect(mock).toHaveBeenCalledTimes(2);
  });

  it("ignores stale batch rejection after reset while a new independent check completes", async () => {
    const pending = deferred();
    api.checkAll.mockReturnValueOnce(pending.promise);
    const { result, showToast } = setup([a]);
    let run;
    act(() => { run = result.current.checkAllUpdates(); result.current.resetUpdateState(); });
    await act(() => result.current.checkUpdate(b.path));
    await act(async () => { pending.reject(new Error("late failure")); await run; });
    expect(result.current.updateState[a.path]).toBeUndefined();
    expect(result.current.getSyncInfo(b).syncState).toBe("current");
    expect(showToast).toHaveBeenCalledTimes(1);
  });

  it("reset during follow-up suppresses stale toast and refresh", async () => {
    const pending = deferred();
    api.checkOne.mockReturnValueOnce(pending.promise);
    const { result, showToast, loadData } = setup();
    let run;
    await act(async () => { run = result.current.updateSkill(a.path); await Promise.resolve(); });
    act(() => result.current.resetUpdateState());
    showToast.mockClear();
    await act(async () => { pending.resolve(current); await run; });
    expect(showToast).not.toHaveBeenCalled();
    expect(loadData).not.toHaveBeenCalled();
    expect(result.current.updateState).toEqual({});
  });

  it("unmount suppresses outstanding apply side effects", async () => {
    const pending = deferred();
    api.applyOne.mockReturnValueOnce(pending.promise);
    const { result, unmount, showToast, loadData } = setup();
    let run;
    act(() => { run = result.current.updateSkill(a.path); });
    unmount();
    await act(async () => { pending.resolve("done"); await run; });
    expect(showToast).not.toHaveBeenCalled();
    expect(api.checkOne).not.toHaveBeenCalled();
    expect(loadData).not.toHaveBeenCalled();
  });
});

it("disables batch button for an updating path outside the visible filter", () => {
  render(<UpdatesView skills={[b]} orderedSkills={[]} stats={{ behind: 0, syncable: 0, failed: 0 }}
    updateState={{ [a.path]: { updating: true } }} getSyncInfo={() => ({})}
    checkAll={vi.fn()} checkOne={vi.fn()} updateOne={vi.fn()} />);
  expect(screen.getByRole("button", { name: "检查更新" }).disabled).toBe(true);
});
