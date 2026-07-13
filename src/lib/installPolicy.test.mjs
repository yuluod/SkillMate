import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_INSTALL_POLICY,
  normalizeInstallPolicy,
  splitPolicyEntries,
} from "./installPolicy.mjs";

test("安装策略输入按换行和逗号去重", () => {
  assert.deepEqual(
    splitPolicyEntries("github.com, gitlab.com\ngithub.com\n"),
    ["github.com", "gitlab.com"]
  );
});

test("安装策略适配缺失字段时保持安全默认值", () => {
  assert.deepEqual(normalizeInstallPolicy(null), DEFAULT_INSTALL_POLICY);
  assert.deepEqual(
    normalizeInstallPolicy({ mode: "trusted-only", block_risky_content: 1 }),
    {
      mode: "trusted-only",
      block_risky_content: true,
      trusted_git_hosts: [],
      trusted_local_roots: [],
    }
  );
});
