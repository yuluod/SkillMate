import test from "node:test";
import assert from "node:assert/strict";

import {
  buildPlanInvocation,
  createSingleFlightPlanExecutor,
  readPlanToken,
} from "./plannedAction.mjs";

test("计划型 IPC 统一使用 camelCase planToken", () => {
  for (const [command, args] of [
    ["install_skill", { package: "owner/repo" }],
    ["import_library", { path: "~/library.json", mode: "merge" }],
    ["apply_skillmate_manifest", { path: "~/skillmate.toml" }],
    ["apply_skill_profile", { profileId: "profile-1" }],
    ["import_scenario_manifest", { path: "~/scenarios.json", mode: "replace" }],
  ]) {
    assert.deepEqual(buildPlanInvocation(command, args, " token "), {
      command,
      args: { ...args, planToken: "token" },
    });
  }
  assert.equal(readPlanToken({ plan_token: " token " }), "token");
  assert.throws(() => buildPlanInvocation("install_skill", {}, ""), /计划缺失/);
});

test("计划型 IPC 连续触发时只启动一次", async () => {
  let resolveInvocation;
  let invocationCount = 0;
  const pending = new Promise((resolve) => { resolveInvocation = resolve; });
  const executor = createSingleFlightPlanExecutor(async () => {
    invocationCount += 1;
    return pending;
  });

  const first = executor.run("install", "install_skill", { package: "owner/repo" }, "token");
  const second = executor.run("install", "install_skill", { package: "owner/repo" }, "token");
  await Promise.resolve();

  assert.equal(first.started, true);
  assert.equal(second.started, false);
  assert.equal(first.promise, second.promise);
  assert.equal(invocationCount, 1);
  resolveInvocation({ success: true });
  await first.promise;

  const third = executor.run("install", "install_skill", { package: "owner/repo" }, "token");
  assert.equal(third.started, true);
  resolveInvocation({ success: true });
  await third.promise;
});

test("计划令牌缺失时不会创建未处理的 rejected Promise", () => {
  const executor = createSingleFlightPlanExecutor(async () => ({ success: true }));
  const execution = executor.run("install", "install_skill", {}, "");

  assert.equal(execution.started, false);
  assert.equal(execution.promise, null);
  assert.match(execution.error.message, /计划缺失/);
});
