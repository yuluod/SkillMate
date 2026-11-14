import React from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SKILL_CHECK_LAST_KEY, useSkillUpdateSchedule } from "./useSkillUpdateSchedule.js";

describe("useSkillUpdateSchedule", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  function setup({ checkAllSilent = vi.fn(async () => true), checkPaths = ["/a"], frequency = "daily" } = {}) {
    localStorage.setItem("skillmate-skill-check-frequency", frequency);
    return renderHook(() => useSkillUpdateSchedule({
      checkAllSilent,
      checkPaths,
      checkableCount: checkPaths.length,
    }));
  }

  it("到期后静默检查并把完整可检查路径交给引擎，启动后记录时间戳", async () => {
    const checkAllSilent = vi.fn(async () => true);
    setup({ checkAllSilent, checkPaths: ["/a", "/b"] });
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    expect(checkAllSilent).toHaveBeenCalledExactlyOnceWith(["/a", "/b"]);
    expect(Number(localStorage.getItem(SKILL_CHECK_LAST_KEY))).toBeGreaterThan(0);
  });

  it("引擎因批量任务跳过时不消耗检查周期（不记录时间戳）", async () => {
    const checkAllSilent = vi.fn(async () => false);
    setup({ checkAllSilent });
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    expect(checkAllSilent).toHaveBeenCalledTimes(1);
    expect(localStorage.getItem(SKILL_CHECK_LAST_KEY)).toBeNull();
  });

  it("检查失败同样记录时间戳，下个周期才重试", async () => {
    const checkAllSilent = vi.fn(async () => true);
    setup({ checkAllSilent });
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    const stamped = localStorage.getItem(SKILL_CHECK_LAST_KEY);
    expect(stamped).not.toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(60 * 60 * 1000); });
    // 一小时内未到日频窗口，不重复检查
    expect(checkAllSilent).toHaveBeenCalledTimes(1);
    expect(localStorage.getItem(SKILL_CHECK_LAST_KEY)).toBe(stamped);
  });

  it("关闭或无库存时不发起任何检查", async () => {
    const checkAllSilent = vi.fn();
    setup({ checkAllSilent, frequency: "off" });
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    expect(checkAllSilent).not.toHaveBeenCalled();
    const idle = vi.fn();
    setup({ checkAllSilent: idle, checkPaths: [] });
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    expect(idle).not.toHaveBeenCalled();
  });
});
