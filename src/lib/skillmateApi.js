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

export const skillmateApi = Object.freeze({
  inventory: Object.freeze({
    loadDashboard: () => Promise.all([
      invoke("get_all_assistants"),
      invoke("get_all_tags"),
      invoke("get_scenarios"),
      invoke("get_git_backup"),
    ]).then(([assistants, tags, scenarios, git]) => ({ assistants, tags, scenarios, git })),
    readSkill: (path) => Promise.all([
      invoke("get_skill_readme", { path }),
      invoke("inspect_skill_validation", { path }),
    ]).then(([content, validation]) => ({ content, validation })),
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
