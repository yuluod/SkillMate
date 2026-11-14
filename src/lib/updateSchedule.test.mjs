import test from "node:test";
import assert from "node:assert/strict";
import { isCheckDue, SKILL_CHECK_INTERVALS } from "./updateSchedule.mjs";

const DAY = SKILL_CHECK_INTERVALS.daily;
const WEEK = SKILL_CHECK_INTERVALS.weekly;

test("off 与未知频次永不到期", () => {
  assert.equal(isCheckDue(0, "off", 10_000), false);
  assert.equal(isCheckDue(null, "unknown", 10_000), false);
});

test("没有上次记录时任意启用频次立即到期", () => {
  assert.equal(isCheckDue(null, "daily", 1_000), true);
  assert.equal(isCheckDue("", "weekly", 1_000), true);
});

test("窗口内未到期，达到窗口即到期", () => {
  const now = 10 * DAY;
  assert.equal(isCheckDue(now - DAY + 1_000, "daily", now), false);
  assert.equal(isCheckDue(now - DAY, "daily", now), true);
  assert.equal(isCheckDue(now - WEEK, "weekly", now), true);
  assert.equal(isCheckDue(now - WEEK + 1_000, "weekly", now), false);
});

test("日频与周频窗口独立", () => {
  const now = 3 * DAY;
  assert.equal(isCheckDue(now - 2 * DAY, "weekly", now), false);
  assert.equal(isCheckDue(now - 2 * DAY, "daily", now), true);
});
