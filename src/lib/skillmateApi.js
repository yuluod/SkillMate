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
  const [assistants, librarySkills] = await Promise.all([
    invoke("get_all_assistants"),
    invoke("get_library_skills"),
  ]);
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
    librarySkills,
    tags: optionalValue("tags", "标签", tagsResult, []),
    scenarios: optionalValue("scenarios", "组合", scenariosResult, []),
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
    trashSkill: (path) => invoke("trash_skill", { path }),
    restoreTrash: (token) => invoke("restore_trashed_skill", { token }),
    purgeTrash: (token) => invoke("purge_trashed_skill", { token }),
    unlinkSkill: (path) => invoke("unlink_symlink_skill", { path }),
    openFolder: (path) => invoke("open_folder", { path }),
    inspectProject: (projectPath) => invoke("inspect_project", { projectPath }),
  }),
  adoption: Object.freeze({
    preview: ({ path, assistantName, projectPath }) => invoke("preview_adopt_skill", { path, assistantName, projectPath }),
    apply: ({ path, assistantName, projectPath, planToken }) => invoke("adopt_skill", { path, assistantName, projectPath, planToken }),
  }),
  market: Object.freeze({
    search: (source, query) => invoke("search_market", { source, query }),
    openSource: (url) => invoke("open_external_url", { url }),
  }),
  drift: Object.freeze({
    preview: (sourcePath, targetPaths) => invoke("preview_sync_skill_copies", { sourcePath, targetPaths }),
    apply: (sourcePath, targetPaths, planToken) => invoke("sync_skill_copies", { sourcePath, targetPaths, planToken }),
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
    preview: ({ packageValue, source, assistantName, installMode, projectPath, selectedSkillPaths, preferredSkillId }) => invoke("preview_install_skill", {
      package: packageValue,
      source,
      assistantName,
      installMode,
      projectPath,
      selectedSkillPaths,
      preferredSkillId,
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
    settings: () => invoke("get_library_settings"),
    setRoot: (path) => invoke("set_library_root", { path }),
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
