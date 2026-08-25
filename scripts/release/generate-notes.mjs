import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function extractChangelogSection(changelog, version) {
  const normalizedVersion = String(version || "").replace(/^v/, "").trim();
  if (!/^\d+\.\d+\.\d+$/.test(normalizedVersion)) {
    throw new Error("更新日志版本必须使用 <semver> 格式");
  }
  const heading = new RegExp(`^## \\[?${escapeRegExp(normalizedVersion)}\\]?(?:\\s+-\\s+[^\\n]+)?\\s*$`, "m");
  const match = changelog.match(heading);
  if (!match || match.index === undefined) {
    throw new Error(`CHANGELOG.md 缺少 ${normalizedVersion} 版本章节`);
  }
  const contentStart = match.index + match[0].length;
  const remaining = changelog.slice(contentStart);
  const boundaries = [remaining.search(/^##\s+/m), remaining.search(/^\[[^\]]+\]:\s+/m)]
    .filter((index) => index >= 0);
  const contentEnd = boundaries.length > 0 ? Math.min(...boundaries) : remaining.length;
  const content = remaining.slice(0, contentEnd).trim();
  if (!content) {
    throw new Error(`CHANGELOG.md 的 ${normalizedVersion} 版本章节为空`);
  }
  return content;
}

export function buildReleaseBody(changelog, tagName) {
  const notes = extractChangelogSection(changelog, tagName);
  return `## 更新内容\n\n${notes}\n\n## 安装包\n\n请从下方 Assets 下载对应系统安装包。\n\n- macOS Apple Silicon: \`_aarch64.dmg\`\n- Windows: \`_setup.exe\`\n- Linux: \`.deb\` 或 \`.rpm\`\n\n当前 macOS 发布包面向 Apple Silicon；Intel Mac 暂不作为 v0.x 发布目标。\n`;
}

function runCli() {
  const changelogPath = resolve(process.env.CHANGELOG_PATH || "CHANGELOG.md");
  const body = buildReleaseBody(readFileSync(changelogPath, "utf8"), process.env.TAG_NAME);
  process.stdout.write(body);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  runCli();
}
