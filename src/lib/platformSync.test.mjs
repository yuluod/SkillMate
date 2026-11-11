import test from "node:test";
import assert from "node:assert/strict";

import {
  findMissingPlatforms,
  findProjectPathDiffs,
  normalizePath,
  parseAgentsDoc,
  parseAssistantDefinitions,
} from "../../scripts/sync-platforms.mjs";

test("平台路径统一去掉前缀、尾斜杠和家目录", () => {
  assert.equal(normalizePath("./.claude/skills/"), ".claude/skills");
  assert.equal(normalizePath("~/.agents/skills"), ".agents/skills");
  assert.equal(normalizePath("N/A (project-only)"), "N/A (project-only)");
});

test("解析上游 agents 文档中的项目与全局目录", () => {
  const rows = parseAgentsDoc(`
| Agent | \`--agent\` | Project Path | Global Path |
|-------|-----------|--------------|-------------|
| Claude Code | \`claude-code\` | \`.claude/skills/\` | \`~/.claude/skills/\` |
| Eve | \`eve\` | \`agent/skills/\` | N/A (project-only) |
`);
  assert.deepEqual(rows, [
    { name: "Claude Code", project: ".claude/skills", global: ".claude/skills" },
  ]);
});

test("解析 Rust 平台定义并与上游比较", () => {
  const defined = parseAssistantDefinitions(`
    AssistantDefinition {
        name: "Claude Code",
        global_install_path: "~/.claude/skills",
        project_install_path: Some(".claude/skills"),
    },
    AssistantDefinition {
        name: "Cursor",
        global_install_path: "~/.cursor/skills",
        project_install_path: Some(".cursor/skills"),
        project_discovery_paths: &[".agents/skills", ".cursor/skills"],
    },
  `);
  assert.deepEqual(defined.map((item) => item.name), ["Claude Code", "Cursor"]);
  assert.deepEqual(defined[1].projectDiscovery, [".agents/skills", ".cursor/skills"]);
  const missing = findMissingPlatforms([
    { name: "Claude Code", project: ".agents/skills", global: ".claude/skills" },
    { name: "Windsurf", project: ".windsurf/skills", global: ".codeium/windsurf/skills" },
  ], defined);
  assert.deepEqual(missing.map((item) => item.name), ["Windsurf"]);
  const diffs = findProjectPathDiffs([
    { name: "Cursor", project: ".agents/skills", global: ".cursor/skills" },
  ], defined);
  // .agents/skills 已在 Cursor 的项目发现目录中，不应再报差异。
  assert.deepEqual(diffs, []);
  const uncovered = findProjectPathDiffs([
    { name: "Cursor", project: ".elsewhere/skills", global: ".cursor/skills" },
  ], defined);
  assert.deepEqual(uncovered, [{
    name: "Cursor",
    skillmate: ".agents/skills, .cursor/skills",
    upstream: ".elsewhere/skills",
  }]);
});
