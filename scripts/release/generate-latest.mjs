import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

export const REQUIRED_UPDATER_PLATFORMS = [
  "darwin-aarch64",
  "darwin-aarch64-app",
  "windows-x86_64",
  "windows-x86_64-nsis",
  "linux-x86_64",
  "linux-x86_64-deb",
  "linux-x86_64-rpm",
];

function requiredAsset(assets, name) {
  if (!assets.has(name)) {
    throw new Error(`缺少 release asset: ${name}`);
  }
  return assets.get(name);
}

export function buildLatestMetadata({ release, repository, tagName, signatures, now = new Date() }) {
  if (!release?.draft) {
    throw new Error("release 已提前公开，拒绝生成 updater metadata");
  }
  const version = String(tagName || "").replace(/^v/, "");
  if (!version) {
    throw new Error("发布 tag 不能为空");
  }
  const assets = new Map((release.assets || []).map((asset) => [asset.name, asset]));
  function entry(assetName) {
    requiredAsset(assets, assetName);
    requiredAsset(assets, `${assetName}.sig`);
    const signature = String(signatures?.[assetName] || "").trim();
    if (!signature) {
      throw new Error(`updater 签名为空: ${assetName}.sig`);
    }
    return {
      signature,
      url: `https://github.com/${repository}/releases/download/${tagName}/${encodeURIComponent(assetName)}`,
    };
  }

  const macApp = entry(`SkillMate_${version}_aarch64.app.tar.gz`);
  const windowsNsis = entry(`SkillMate_${version}_x64-setup.exe`);
  const linuxDeb = entry(`SkillMate_${version}_amd64.deb`);
  const linuxRpm = entry(`SkillMate-${version}-1.x86_64.rpm`);
  const platforms = {
    "darwin-aarch64": macApp,
    "darwin-aarch64-app": macApp,
    "windows-x86_64": windowsNsis,
    "windows-x86_64-nsis": windowsNsis,
    "linux-x86_64": linuxDeb,
    "linux-x86_64-deb": linuxDeb,
    "linux-x86_64-rpm": linuxRpm,
  };
  const missing = REQUIRED_UPDATER_PLATFORMS.filter((platform) => !platforms[platform]);
  if (missing.length > 0) {
    throw new Error(`latest.json 缺少平台: ${missing.join(", ")}`);
  }
  return {
    version,
    notes: release.body || "",
    pub_date: now.toISOString(),
    platforms,
  };
}

function runCli() {
  const releasePath = resolve(process.env.RELEASE_JSON || "release.json");
  const signatureDirectory = resolve(process.env.SIGNATURE_DIR || "release-signatures");
  const outputPath = resolve(process.env.OUTPUT_JSON || "latest.json");
  const release = JSON.parse(readFileSync(releasePath, "utf8"));
  const version = String(process.env.TAG_NAME || "").replace(/^v/, "");
  const assetNames = [
    `SkillMate_${version}_aarch64.app.tar.gz`,
    `SkillMate_${version}_x64-setup.exe`,
    `SkillMate_${version}_amd64.deb`,
    `SkillMate-${version}-1.x86_64.rpm`,
  ];
  const signatures = Object.fromEntries(assetNames.map((assetName) => [
    assetName,
    readFileSync(resolve(signatureDirectory, `${assetName}.sig`), "utf8"),
  ]));
  const metadata = buildLatestMetadata({
    release,
    repository: process.env.REPOSITORY,
    tagName: process.env.TAG_NAME,
    signatures,
  });
  writeFileSync(outputPath, `${JSON.stringify(metadata, null, 2)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  runCli();
}
