import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

function readJson(relativePath) {
  return JSON.parse(readFileSync(resolve(repoRoot, relativePath), "utf8"));
}

function readCargoPackageVersion() {
  const cargoToml = readFileSync(resolve(repoRoot, "src-tauri/Cargo.toml"), "utf8");
  const lines = cargoToml.split(/\r?\n/);
  const packageStart = lines.findIndex((line) => line.trim() === "[package]");
  assert.notEqual(packageStart, -1, "Cargo.toml 必须包含 [package] 区块");

  const packageLines = [];
  for (const line of lines.slice(packageStart + 1)) {
    if (line.startsWith("[") && line.endsWith("]")) {
      break;
    }
    packageLines.push(line);
  }

  const version = packageLines.join("\n").match(/^version\s*=\s*"([^"]+)"/m);
  assert.ok(version, "Cargo.toml [package] 必须声明 version");
  return version[1];
}

test("发布元数据版本必须保持一致", () => {
  const packageJson = readJson("package.json");
  const tauriConfig = readJson("src-tauri/tauri.conf.json");
  const cargoVersion = readCargoPackageVersion();

  assert.equal(packageJson.version, "0.0.1");
  assert.equal(tauriConfig.version, packageJson.version);
  assert.equal(cargoVersion, packageJson.version);
});

test("Tauri bundle identifier 不应使用 .app 结尾", () => {
  const tauriConfig = readJson("src-tauri/tauri.conf.json");

  assert.equal(tauriConfig.identifier, "io.github.yuluod.skillmate");
  assert.ok(!tauriConfig.identifier.endsWith(".app"));
});
