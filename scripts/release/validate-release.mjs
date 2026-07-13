import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

function readJson(path) {
  return JSON.parse(readFileSync(resolve(path), "utf8"));
}

function cargoVersion(content) {
  const packageBlock = content.match(/\[package\]([\s\S]*?)(?:\n\[|$)/);
  const version = packageBlock?.[1]?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) {
    throw new Error("Cargo.toml [package] 缺少 version");
  }
  return version;
}

export function validateReleaseVersions({ tagName, packageVersion, tauriVersion, rustVersion }) {
  const match = String(tagName || "").match(/^v(\d+\.\d+\.\d+)$/);
  if (!match) {
    throw new Error("发布 tag 必须使用 v<semver> 格式");
  }
  const tagVersion = match[1];
  const versions = { packageVersion, tauriVersion, rustVersion };
  for (const [source, version] of Object.entries(versions)) {
    if (version !== tagVersion) {
      throw new Error(`${source}=${version} 与 tag=${tagVersion} 不一致`);
    }
  }
  return tagVersion;
}

function runCli() {
  const packageJson = readJson("package.json");
  const tauriConfig = readJson("src-tauri/tauri.conf.json");
  const rustVersion = cargoVersion(readFileSync("src-tauri/Cargo.toml", "utf8"));
  const version = validateReleaseVersions({
    tagName: process.env.TAG_NAME,
    packageVersion: packageJson.version,
    tauriVersion: tauriConfig.version,
    rustVersion,
  });
  process.stdout.write(`${version}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  runCli();
}
