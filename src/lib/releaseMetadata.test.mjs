import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

function readJson(relativePath) {
  return JSON.parse(readFileSync(resolve(repoRoot, relativePath), "utf8"));
}

function readText(relativePath) {
  return readFileSync(resolve(repoRoot, relativePath), "utf8");
}

function readCargoPackageVersion() {
  const cargoToml = readText("src-tauri/Cargo.toml");
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

  assert.equal(packageJson.version, "0.0.4");
  assert.equal(tauriConfig.version, packageJson.version);
  assert.equal(cargoVersion, packageJson.version);
});

test("Tauri bundle identifier 不应使用 .app 结尾", () => {
  const tauriConfig = readJson("src-tauri/tauri.conf.json");

  assert.equal(tauriConfig.identifier, "io.github.yuluod.skillmate");
  assert.ok(!tauriConfig.identifier.endsWith(".app"));
});

test("Tauri updater 配置必须生成签名更新包", () => {
  const tauriConfig = readJson("src-tauri/tauri.conf.json");
  const capabilities = readJson("src-tauri/capabilities/default.json");
  const security = tauriConfig.app.security;

  assert.equal(tauriConfig.bundle.createUpdaterArtifacts, true);
  assert.doesNotMatch(security.csp, /unsafe-eval/);
  assert.doesNotMatch(security.csp, /localhost:1420/);
  assert.match(security.devCsp, /unsafe-eval/);
  assert.match(security.devCsp, /localhost:1420/);
  assert.match(
    tauriConfig.plugins.updater.pubkey,
    /^dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6/
  );
  assert.deepEqual(tauriConfig.plugins.updater.endpoints, [
    "https://github.com/yuluod/SkillMate/releases/latest/download/latest.json",
  ]);
  assert.equal(tauriConfig.plugins.updater.windows.installMode, "passive");
  assert.ok(capabilities.permissions.includes("updater:default"));
  assert.ok(capabilities.permissions.includes("process:default"));
});

test("Release workflow 必须发布 updater metadata", () => {
  const workflow = readText(".github/workflows/release.yml");

  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY/);
  assert.match(workflow, /缺少 updater 签名密钥/);
  assert.match(workflow, /macOS Apple Silicon/);
  assert.match(workflow, /Intel Mac 暂不作为 v0\.x 发布目标/);
  assert.match(workflow, /args: "--bundles app,dmg"/);
  assert.equal((workflow.match(/includeUpdaterJson: true/g) || []).length, 0);
  assert.equal((workflow.match(/updaterJsonPreferNsis: true/g) || []).length, 0);
  assert.match(workflow, /Generate updater metadata/);
  assert.match(workflow, /SkillMate_aarch64\.app\.tar\.gz/);
  assert.match(workflow, /SkillMate_\$\{version\}_x64-setup\.exe/);
  assert.match(workflow, /SkillMate_\$\{version\}_amd64\.deb/);
  assert.match(workflow, /SkillMate-\$\{version\}-1\.x86_64\.rpm/);
  assert.match(workflow, /\.toString\("utf8"\)\.trim\(\)/);
  assert.doesNotMatch(workflow, /\.toString\("base64"\)/);
  assert.match(workflow, /updater 签名为空/);
  for (const platform of [
    "darwin-aarch64",
    "darwin-aarch64-app",
    "windows-x86_64",
    "windows-x86_64-nsis",
    "linux-x86_64",
    "linux-x86_64-deb",
    "linux-x86_64-rpm",
  ]) {
    assert.match(workflow, new RegExp(`"${platform}"`));
  }
});
