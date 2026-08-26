import test from "node:test";
import assert from "node:assert/strict";

import { toUserErrorMessage } from "./errorMessage.mjs";

test("保留可理解的后端错误并移除通用异常前缀", () => {
  assert.equal(toUserErrorMessage(new Error("目录不可读"), "请重试"), "目录不可读");
});

test("隐藏实现细节并保留操作上下文", () => {
  assert.equal(
    toUserErrorMessage("加载安装策略失败：TypeError: Cannot read properties of undefined (reading 'invoke')", "请重试", "；"),
    "加载安装策略失败；请重试",
  );
  assert.equal(toUserErrorMessage("TypeError: invoke is unavailable", "请重试"), "请重试");
});
