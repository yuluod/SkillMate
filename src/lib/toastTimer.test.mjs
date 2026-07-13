import test from "node:test";
import assert from "node:assert/strict";

import { createResettableTimer } from "./toastTimer.mjs";

function fakeScheduler() {
  let nextId = 1;
  const tasks = new Map();
  return {
    tasks,
    schedule(callback) {
      const id = nextId;
      nextId += 1;
      tasks.set(id, callback);
      return id;
    },
    cancel(id) { tasks.delete(id); },
  };
}

test("Toast 计时器只执行最后一次回调", () => {
  const scheduler = fakeScheduler();
  const timer = createResettableTimer(scheduler);
  const calls = [];

  timer.start(3000, () => calls.push("first"));
  timer.start(3000, () => calls.push("second"));

  assert.equal(scheduler.tasks.size, 1);
  [...scheduler.tasks.values()][0]();
  assert.deepEqual(calls, ["second"]);
});

test("Toast 计时器销毁后不会再执行回调", () => {
  const scheduler = fakeScheduler();
  const timer = createResettableTimer(scheduler);
  timer.start(3000, () => assert.fail("销毁后不应执行"));

  timer.dispose();

  assert.equal(scheduler.tasks.size, 0);
});
