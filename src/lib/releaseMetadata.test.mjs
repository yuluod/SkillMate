import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { buildLatestMetadata, REQUIRED_UPDATER_PLATFORMS } from "../../scripts/release/generate-latest.mjs";
import { validateReleaseVersions } from "../../scripts/release/validate-release.mjs";

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

  assert.equal(packageJson.version, "0.0.7");
  assert.equal(tauriConfig.version, packageJson.version);
  assert.equal(cargoVersion, packageJson.version);
});

test("发布版本校验必须拒绝 tag 与三处版本不一致", () => {
  assert.equal(
    validateReleaseVersions({
      tagName: "v0.0.4",
      packageVersion: "0.0.4",
      tauriVersion: "0.0.4",
      rustVersion: "0.0.4",
    }),
    "0.0.4"
  );
  assert.throws(
    () => validateReleaseVersions({
      tagName: "v0.0.5",
      packageVersion: "0.0.4",
      tauriVersion: "0.0.5",
      rustVersion: "0.0.5",
    }),
    /packageVersion/
  );
  assert.throws(
    () => validateReleaseVersions({
      tagName: "v0.0.4-beta.1",
      packageVersion: "0.0.4-beta.1",
      tauriVersion: "0.0.4-beta.1",
      rustVersion: "0.0.4-beta.1",
    }),
    /v<semver>/
  );
});

test("latest.json 生成器必须输出完整平台并拒绝缺失签名", () => {
  const version = "0.0.4";
  const assetNames = [
    `SkillMate_${version}_aarch64.app.tar.gz`,
    `SkillMate_${version}_x64-setup.exe`,
    `SkillMate_${version}_amd64.deb`,
    `SkillMate-${version}-1.x86_64.rpm`,
  ];
  const assets = assetNames.flatMap((name, index) => [
    { id: index * 2 + 1, name },
    { id: index * 2 + 2, name: `${name}.sig` },
  ]);
  const signatures = Object.fromEntries(assetNames.map((name) => [name, `signature-${name}`]));
  const metadata = buildLatestMetadata({
    release: { draft: true, body: "更新日志", assets },
    repository: "yuluod/SkillMate",
    tagName: `v${version}`,
    signatures,
    now: new Date("2026-07-12T00:00:00.000Z"),
  });

  assert.equal(metadata.version, version);
  assert.equal(metadata.notes, "更新日志");
  assert.deepEqual(Object.keys(metadata.platforms).sort(), [...REQUIRED_UPDATER_PLATFORMS].sort());
  assert.throws(
    () => buildLatestMetadata({
      release: { draft: true, assets },
      repository: "yuluod/SkillMate",
      tagName: `v${version}`,
      signatures: {},
    }),
    /签名为空/
  );
});

test("Tauri bundle identifier 不应使用 .app 结尾", () => {
  const tauriConfig = readJson("src-tauri/tauri.conf.json");

  assert.equal(tauriConfig.identifier, "io.github.yuluod.skillmate");
  assert.ok(!tauriConfig.identifier.endsWith(".app"));
});

test("Tauri updater 配置必须生成签名更新包", () => {
  const tauriConfig = readJson("src-tauri/tauri.conf.json");
  const capabilities = readJson("src-tauri/capabilities/default.json");
  const cargoToml = readText("src-tauri/Cargo.toml");
  const security = tauriConfig.app.security;

  assert.equal(tauriConfig.bundle.createUpdaterArtifacts, true);
  assert.equal(tauriConfig.bundle.targets, "all");
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
  assert.doesNotMatch(cargoToml, /features\s*=\s*\[[^\]]*"devtools"/);
  assert.doesNotMatch(cargoToml, /tauri-plugin-(?:shell|fs|dialog)/);
});

test("Release workflow 必须发布 updater metadata", () => {
  const workflow = readText(".github/workflows/release.yml");
  const generator = readText("scripts/release/generate-latest.mjs");
  const publishStep = workflow.indexOf("Publish completed release");
  const metadataStep = workflow.indexOf("Generate updater metadata");

  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY/);
  assert.match(workflow, /缺少 updater 签名密钥/);
  assert.match(workflow, /macOS Apple Silicon/);
  assert.match(workflow, /Intel Mac 暂不作为 v0\.x 发布目标/);
  assert.match(workflow, /args: "--target aarch64-apple-darwin --bundles app,dmg"/);
  assert.equal((workflow.match(/includeUpdaterJson: true/g) || []).length, 0);
  assert.equal((workflow.match(/updaterJsonPreferNsis: true/g) || []).length, 0);
  assert.ok(publishStep >= 0, "Release workflow 必须包含发布 release 步骤");
  assert.ok(metadataStep >= 0, "Release workflow 必须包含 updater metadata 步骤");
  assert.ok(
    metadataStep < publishStep,
    "必须先在 draft release 中上传并校验 latest.json，最后再公开 release"
  );
  assert.match(workflow, /Generate updater metadata/);
  assert.match(generator, /release 已提前公开/);
  assert.match(workflow, /draft release 中未找到 latest\.json/);
  assert.match(workflow, /scripts\/release\/validate-release\.mjs/);
  assert.match(workflow, /git merge-base --is-ancestor/);
  assert.match(workflow, /拒绝覆盖正式 Release/);
  assert.match(workflow, /--target aarch64-apple-darwin/);
  assert.match(workflow, /SHA256SUMS/);
  assert.match(workflow, /skillmate-sbom\.spdx\.json/);
  assert.match(workflow, /actions\/attest-build-provenance@[0-9a-f]{40}/);
  assert.match(generator, /encodeURIComponent\(assetName\)/);
  assert.match(generator, /SkillMate_\$\{version\}_aarch64\.app\.tar\.gz/);
  assert.match(generator, /SkillMate_\$\{version\}_x64-setup\.exe/);
  assert.match(generator, /SkillMate_\$\{version\}_amd64\.deb/);
  assert.match(generator, /SkillMate-\$\{version\}-1\.x86_64\.rpm/);
  assert.match(generator, /updater 签名为空/);
  for (const platform of [
    "darwin-aarch64",
    "darwin-aarch64-app",
    "windows-x86_64",
    "windows-x86_64-nsis",
    "linux-x86_64",
    "linux-x86_64-deb",
    "linux-x86_64-rpm",
  ]) {
    assert.match(generator, new RegExp(`"${platform}"`));
  }
});

test("普通 CI 必须覆盖前端、格式、Clippy 和 Rust 测试", () => {
  const workflow = readText(".github/workflows/ci.yml");
  const platformTestStart = workflow.indexOf("  platform-test:");

  assert.match(workflow, /pull_request:/);
  assert.match(workflow, /branches:\s*\n\s*- main/);
  assert.match(workflow, /pnpm frontend:test/);
  assert.match(workflow, /pnpm frontend:build/);
  assert.match(workflow, /\.\/actionlint/);
  assert.match(workflow, /windows-2022/);
  assert.match(workflow, /macos-14/);
  assert.ok(platformTestStart >= 0, "普通 CI 必须包含跨平台测试 job");
  const platformTest = workflow.slice(platformTestStart);
  assert.match(platformTest, /cargo test --manifest-path src-tauri\/Cargo\.toml --locked --no-fail-fast/);
  assert.match(workflow, /cargo fmt --manifest-path src-tauri\/Cargo\.toml -- --check/);
  assert.match(workflow, /cargo clippy --manifest-path src-tauri\/Cargo\.toml --all-targets --locked -- -D warnings/);
  assert.match(workflow, /cargo test --manifest-path src-tauri\/Cargo\.toml --locked --no-fail-fast/);
});

test("工作流 Action 必须固定完整提交 SHA", () => {
  for (const path of [".github/workflows/ci.yml", ".github/workflows/release.yml"]) {
    const workflow = readText(path);
    const uses = [...workflow.matchAll(/^\s*uses:\s*([^\s#]+)/gm)].map((match) => match[1]);
    assert.ok(uses.length > 0, `${path} 必须包含 Action`);
    for (const action of uses) {
      assert.match(action, /^(?:\.\/|[^@]+@[0-9a-f]{40}$)/, `${path} 未固定 Action: ${action}`);
    }
  }
});

test("公开仓库必须提供完整许可证、贡献与安全治理文件", () => {
  const license = readText("LICENSE");
  const packageJson = readJson("package.json");
  const toolchain = readText("rust-toolchain.toml");
  const dependabot = readText(".github/dependabot.yml");

  assert.match(license, /GNU AFFERO GENERAL PUBLIC LICENSE/);
  assert.match(license, /END OF TERMS AND CONDITIONS/);
  assert.ok(license.length > 30_000);
  assert.match(readText("SECURITY.md"), /Report a vulnerability/);
  assert.match(readText("CONTRIBUTING.md"), /cargo clippy/);
  assert.equal(readText(".node-version").trim(), "22.13.0");
  assert.equal(packageJson.engines.node, ">=22.13.0");
  assert.equal(packageJson.devEngines.runtime.version, ">=22.13.0");
  assert.match(toolchain, /channel = "1\.96\.0"/);
  assert.match(dependabot, /package-ecosystem: npm/);
  assert.match(dependabot, /package-ecosystem: cargo/);
  assert.match(dependabot, /package-ecosystem: github-actions/);
});
