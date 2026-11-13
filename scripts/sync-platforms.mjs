import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const AGENTS_DOC = "https://raw.githubusercontent.com/vercel-labs/skills/main/docs/agents.md";
const APP_CORE = resolve(import.meta.dirname, "../src-tauri/src/app_core.rs");

export function normalizePath(value) {
  return String(value || "")
    .trim()
    .replace(/`/g, "")
    .replace(/^\.\//, "")
    .replace(/\/+$/, "")
    .replace(/^~\//, "");
}

export function parseAgentsDoc(markdown) {
  const rows = [];
  for (const line of String(markdown).split("\n")) {
    const cells = line.split("|").map((cell) => cell.trim());
    if (cells.length < 6 || !cells[1] || cells[1] === "Agent" || /^-+$/.test(cells[2])) continue;
    const project = normalizePath(cells[3]);
    const global = normalizePath(cells[4]);
    if (!project || !global || project.startsWith("N/A") || global.startsWith("N/A")) continue;
    rows.push({ name: cells[1], project, global });
  }
  return rows;
}

export function parseAssistantDefinitions(source) {
  const blocks = String(source).split("AssistantDefinition {").slice(1);
  return blocks
    .map((block) => {
      const read = (pattern) => block.match(pattern)?.[1] || "";
      const discovery = (block.match(/project_discovery_paths:\s*&\[([^\]]*)\]/)?.[1] || "")
        .split(",")
        .map((item) => normalizePath(item.replace(/"/g, "")))
        .filter(Boolean);
      return {
        name: read(/name:\s*"([^"]+)"/),
        project: normalizePath(read(/project_install_path:\s*Some\("([^"]+)"\)/)),
        projectDiscovery: discovery,
        global: normalizePath(read(/global_install_path:\s*"([^"]+)"/)),
      };
    })
    .filter((item) => item.name && item.project && item.global);
}

export function findMissingPlatforms(documented, defined) {
  // 全局目录是平台的稳定身份；项目目录各工具常有别名，不作为是否已支持的判据。
  const known = new Set(defined.map((item) => item.global));
  return documented.filter((item) => !known.has(item.global));
}

export function findProjectPathDiffs(documented, defined) {
  const byGlobal = new Map(defined.map((item) => [item.global, item]));
  const diffs = [];
  for (const item of documented) {
    const match = byGlobal.get(item.global);
    if (match && !match.projectDiscovery.includes(item.project)) {
      diffs.push({
        name: match.name,
        skillmate: match.projectDiscovery.join(", "),
        upstream: item.project,
      });
    }
  }
  return diffs;
}

async function main() {
  const response = await fetch(AGENTS_DOC);
  if (!response.ok) throw new Error(`无法读取 ${AGENTS_DOC}: ${response.status}`);
  const documented = parseAgentsDoc(await response.text());
  const defined = parseAssistantDefinitions(readFileSync(APP_CORE, "utf8"));
  const missing = findMissingPlatforms(documented, defined);
  const diffs = findProjectPathDiffs(documented, defined);

  console.log(`已定义 ${defined.length} 个平台，上游文档列出 ${documented.length} 个项目目录。`);
  if (diffs.length > 0) {
    console.log(`\n项目目录与上游不同（${diffs.length}）：`);
    for (const item of diffs) {
      console.log(`- ${item.name}: SkillMate ${item.skillmate} / 上游 ${item.upstream}`);
    }
  }
  if (missing.length === 0) {
    console.log("没有发现缺失的平台。");
    return;
  }
  console.log(`\n上游存在但 SkillMate 未定义（${missing.length}）：`);
  for (const item of missing) {
    console.log(`- ${item.name}: 项目 ${item.project} / 全局 ${item.global}`);
  }
  console.log("\n按 src-tauri/src/app_core.rs 的 AssistantDefinition 补充，并同步路径矩阵测试与 README。");
}

if (process.argv[1] && resolve(process.argv[1]) === import.meta.filename) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
