import test from "node:test";
import assert from "node:assert/strict";
import { formatTranslation } from "./i18nCore.mjs";
import en from "../locales/en.js";
import zhCN from "../locales/zh-CN.js";

test("i18n 在键缺失时回退中文并替换变量", () => {
  assert.equal(
    formatTranslation({}, { greeting: "你好，{name}" }, "greeting", { name: "SkillMate" }),
    "你好，SkillMate",
  );
});

test("英文词典应当覆盖所有中文界面文案键", () => {
  assert.deepEqual(Object.keys(zhCN).filter((key) => !(key in en)), []);
});
