import { invoke } from "@tauri-apps/api/core";

export const skillmateCommands = Object.freeze({
  importLibrary: "import_library",
  applySkillMateManifest: "apply_skillmate_manifest",
  applySkillProfile: "apply_skill_profile",
  importScenarioManifest: "import_scenario_manifest",
  installSkill: "install_skill",
});

export function invokeSkillMateCommand(command, args) {
  return invoke(command, args);
}

async function loadDashboard() {
  const assistants = await invoke("get_all_assistants");
  const [tagsResult, scenariosResult, gitResult] = await Promise.allSettled([
    invoke("get_all_tags"),
    invoke("get_scenarios"),
    invoke("get_git_backup"),
  ]);
  const diagnostics = [];
  const optionalValue = (section, label, result, fallback) => {
    if (result.status === "fulfilled") return result.value;
    diagnostics.push({ section, label, message: String(result.reason) });
    return fallback;
  };

  return {
    assistants,
    tags: optionalValue("tags", "标签", tagsResult, []),
    scenarios: optionalValue("scenarios", "场景", scenariosResult, []),
    git: optionalValue("git", "Git 备份", gitResult, {
      enabled: false,
      repo_path: "",
      remote_url: "",
      branch: "main",
      last_sync: "",
    }),
    diagnostics,
  };
}

async function readSkill(path) {
  const [contentResult, validationResult] = await Promise.allSettled([
    invoke("get_skill_readme", { path }),
    invoke("inspect_skill_validation", { path }),
  ]);
  if (contentResult.status === "rejected") throw contentResult.reason;

  const diagnostics = validationResult.status === "rejected"
    ? [{ section: "validation", label: "结构验证", message: String(validationResult.reason) }]
    : [];
  return {
    content: contentResult.value,
    validation: validationResult.status === "fulfilled" ? validationResult.value : null,
    diagnostics,
  };
}

export const skillmateApi = Object.freeze({
  inventory: Object.freeze({
    loadDashboard,
    readSkill,
    deleteSkill: (path) => invoke("delete_skill", { path }),
    unlinkSkill: (path) => invoke("unlink_symlink_skill", { path }),
    openFolder: (path) => invoke("open_folder", { path }),
  }),
  tags: Object.freeze({
    add: (name, color) => invoke("add_tag", { name, color }),
    updateSkill: (skillPath, tags) => invoke("update_skill_tags", { skillPath, tags }),
  }),
  scenarios: Object.freeze({
    create: ({ name, description, skillIds }) => invoke("create_scenario", { name, description, skillIds }),
    delete: (scenarioId) => invoke("delete_scenario", { scenarioId }),
    exportManifest: (path) => invoke("export_scenario_manifest", { path }),
    previewManifest: (path, mode) => invoke("preview_import_scenario_manifest", { path, mode }),
  }),
  backup: Object.freeze({
    setup: (payload) => invoke("setup_git_backup", payload),
    sync: (message) => invoke("sync_to_git", { message }),
  }),
  install: Object.freeze({
    detectSource: (input) => invoke("detect_install_source", { input }),
    previewProjectTargets: (projectPath) => invoke("preview_project_skill_targets", { projectPath }),
    preview: ({ packageValue, source, assistantName, installMode, projectPath }) => invoke("preview_install_skill", {
      package: packageValue,
      source,
      assistantName,
      installMode,
      projectPath,
    }),
  }),
  updates: Object.freeze({
    checkAll: (paths) => invoke("check_updates", { paths }),
    checkOne: (path) => invoke("check_update", { path, force: true }),
    applyOne: (path) => invoke("update_from_upstream", { path }),
  }),
  policy: Object.freeze({
    get: () => invoke("get_install_policy"),
    set: (config) => invoke("set_install_policy", { config }),
  }),
  library: Object.freeze({
    export: (path) => invoke("export_library", { path }),
    previewImport: (path, mode) => invoke("preview_import_library", { path, mode }),
  }),
  manifests: Object.freeze({
    exportSkillMate: (path) => invoke("export_skillmate_manifest", { path }),
    exportProject: (projectPath) => invoke("export_project_skillmate_manifest", { projectPath }),
    previewSkillMate: (path) => invoke("preview_apply_skillmate_manifest", { path }),
  }),
  profiles: Object.freeze({
    get: () => invoke("get_skill_profiles"),
    saveCurrent: (name, description) => invoke("save_current_skill_profile", { name, description }),
    preview: (profileId) => invoke("preview_apply_skill_profile", { profileId }),
    rollback: () => invoke("rollback_skill_profile"),
  }),
});
