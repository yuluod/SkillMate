import { useEffect, useRef, useState } from "react";
import { buildImportPreviewToken, isImportPreviewCurrent } from "./skillmate.mjs";
import { createSingleFlightPlanExecutor } from "./plannedAction.mjs";
import { invokeSkillMateCommand, skillmateApi, skillmateCommands } from "./skillmateApi.js";

export function useImportExportFlow({ showToast, loadData }) {
  const [exportPath, setExportPath] = useState("~/skillmate-export.json");
  const [importPath, setImportPath] = useState("~/skillmate-export.json");
  const [importMode, setImportMode] = useState("merge");
  const [importPreview, setImportPreview] = useState(null);
  const [importPreviewToken, setImportPreviewToken] = useState(null);
  const [previewingImport, setPreviewingImport] = useState(false);
  const [applyingImport, setApplyingImport] = useState(false);
  const [scenarioManifestPath, setScenarioManifestPath] = useState("");
  const [scenarioManifestMode, setScenarioManifestMode] = useState("merge");
  const [scenarioManifestPreview, setScenarioManifestPreview] = useState(null);
  const [scenarioManifestPreviewToken, setScenarioManifestPreviewToken] = useState(null);
  const [previewingScenarioManifest, setPreviewingScenarioManifest] = useState(false);
  const [applyingScenarioManifest, setApplyingScenarioManifest] = useState(false);
  const [skillMateManifestPath, setSkillMateManifestPath] = useState("~/skillmate.toml");
  const [projectManifestRoot, setProjectManifestRoot] = useState("");
  const [skillMateManifestPreview, setSkillMateManifestPreview] = useState(null);
  const [skillMateManifestPreviewToken, setSkillMateManifestPreviewToken] = useState(null);
  const [previewingSkillMateManifest, setPreviewingSkillMateManifest] = useState(false);
  const [applyingSkillMateManifest, setApplyingSkillMateManifest] = useState(false);
  const [skillProfiles, setSkillProfiles] = useState({ version: 1, active_profile_id: null, profiles: [] });
  const [skillProfileName, setSkillProfileName] = useState("");
  const [skillProfileDescription, setSkillProfileDescription] = useState("");
  const [skillProfilePreview, setSkillProfilePreview] = useState(null);
  const [previewingSkillProfile, setPreviewingSkillProfile] = useState(false);
  const [applyingSkillProfile, setApplyingSkillProfile] = useState(false);
  const planExecutorRef = useRef(null);
  if (!planExecutorRef.current) {
    planExecutorRef.current = createSingleFlightPlanExecutor(invokeSkillMateCommand);
  }

  useEffect(() => {
    loadSkillProfiles();
  }, []);

  const importPreviewCurrent = Boolean(importPreviewToken?.planToken) && isImportPreviewCurrent({
    previewToken: importPreviewToken,
    path: importPath,
    mode: importMode,
  });
  const scenarioManifestPreviewCurrent = Boolean(scenarioManifestPreviewToken?.planToken) && isImportPreviewCurrent({
    previewToken: scenarioManifestPreviewToken,
    path: scenarioManifestPath,
    mode: scenarioManifestMode,
  });
  const skillMateManifestPreviewCurrent = Boolean(skillMateManifestPreviewToken?.planToken) && isImportPreviewCurrent({
    previewToken: skillMateManifestPreviewToken,
    path: skillMateManifestPath,
    mode: "apply",
  });

  function updateImportPath(value) {
    setImportPath(value);
    setImportPreview(null);
    setImportPreviewToken(null);
  }

  function updateImportMode(value) {
    setImportMode(value);
    setImportPreview(null);
    setImportPreviewToken(null);
  }

  function updateScenarioManifestPath(value) {
    setScenarioManifestPath(value);
    setScenarioManifestPreview(null);
    setScenarioManifestPreviewToken(null);
  }

  function updateScenarioManifestMode(value) {
    setScenarioManifestMode(value);
    setScenarioManifestPreview(null);
    setScenarioManifestPreviewToken(null);
  }

  function updateSkillMateManifestPath(value) {
    setSkillMateManifestPath(value);
    setSkillMateManifestPreview(null);
    setSkillMateManifestPreviewToken(null);
  }

  async function loadSkillProfiles() {
    try {
      const result = await skillmateApi.profiles.get();
      setSkillProfiles(result);
    } catch (e) {
      showToast(`加载 Profile 失败: ${e}`, "error");
    }
  }

  async function exportLibraryFile() {
    if (!exportPath.trim()) {
      showToast("请输入导出文件路径", "error");
      return;
    }
    try {
      const result = await skillmateApi.library.export(exportPath);
      showToast(String(result), "success");
    } catch (e) {
      showToast(`导出失败: ${e}`, "error");
    }
  }

  async function previewImportLibraryFile() {
    if (!importPath.trim()) {
      showToast("请输入导入文件路径", "error");
      return;
    }
    setPreviewingImport(true);
    try {
      const result = await skillmateApi.library.previewImport(importPath, importMode);
      setImportPreview(result);
      setImportPreviewToken({
        ...buildImportPreviewToken({ path: importPath, mode: importMode }),
        planToken: result.plan_token || "",
      });
      showToast("已生成导入预览", "success");
    } catch (e) {
      setImportPreview(null);
      setImportPreviewToken(null);
      showToast(`预览失败: ${e}`, "error");
    } finally {
      setPreviewingImport(false);
    }
  }

  async function importLibraryFile() {
    if (!importPath.trim()) {
      showToast("请输入导入文件路径", "error");
      return;
    }
    if (!importPreview) {
      showToast("请先预览导入内容", "warn");
      return;
    }
    if (!importPreviewCurrent) {
      showToast("导入参数已变化，请重新预览", "warn");
      return;
    }
    if (!importPreviewToken?.planToken) {
      showToast("导入计划缺失，请重新预览", "warn");
      return;
    }
    const execution = planExecutorRef.current.run(
      "library-import",
      skillmateCommands.importLibrary,
      { path: importPath, mode: importMode },
      importPreviewToken.planToken,
    );
    if (!execution.started) return;
    setApplyingImport(true);
    try {
      const result = await execution.promise;
      showToast(String(result), "success");
      setImportPreview(null);
      setImportPreviewToken(null);
      await loadData();
    } catch (e) {
      showToast(`导入失败: ${e}`, "error");
    } finally {
      setApplyingImport(false);
    }
  }

  async function exportSkillMateManifestFile() {
    if (!skillMateManifestPath.trim()) {
      showToast("请输入 skillmate.toml 路径", "error");
      return;
    }
    try {
      const result = await skillmateApi.manifests.exportSkillMate(skillMateManifestPath);
      showToast(String(result), "success");
    } catch (e) {
      showToast(`导出失败: ${e}`, "error");
    }
  }

  async function exportProjectSkillMateManifestFile() {
    if (!projectManifestRoot.trim()) {
      showToast("请输入项目路径", "error");
      return;
    }
    try {
      const path = await skillmateApi.manifests.exportProject(projectManifestRoot.trim());
      updateSkillMateManifestPath(String(path));
      showToast(`项目锁定清单已导出到 ${path}`, "success");
    } catch (e) {
      showToast(`导出项目锁定清单失败: ${e}`, "error");
    }
  }

  async function previewSkillMateManifestFile() {
    if (!skillMateManifestPath.trim()) {
      showToast("请输入 skillmate.toml 路径", "error");
      return;
    }
    setPreviewingSkillMateManifest(true);
    try {
      const result = await skillmateApi.manifests.previewSkillMate(skillMateManifestPath);
      setSkillMateManifestPreview(result);
      setSkillMateManifestPreviewToken({
        ...buildImportPreviewToken({ path: skillMateManifestPath, mode: "apply" }),
        planToken: result.plan_token || "",
      });
      showToast("已生成 SkillMate manifest 预览", result.can_apply ? "success" : "warn");
    } catch (e) {
      setSkillMateManifestPreview(null);
      setSkillMateManifestPreviewToken(null);
      showToast(`预览失败: ${e}`, "error");
    } finally {
      setPreviewingSkillMateManifest(false);
    }
  }

  async function applySkillMateManifestFile() {
    if (!skillMateManifestPreview) {
      showToast("请先预览 skillmate.toml", "warn");
      return;
    }
    if (!skillMateManifestPreviewCurrent) {
      showToast("manifest 路径已变化，请重新预览", "warn");
      return;
    }
    if (!skillMateManifestPreviewToken?.planToken) {
      showToast("manifest 计划缺失，请重新预览", "warn");
      return;
    }
    if (!skillMateManifestPreview.can_apply) {
      showToast("manifest 存在冲突或格式问题，无法应用", "error");
      return;
    }
    const execution = planExecutorRef.current.run(
      "skillmate-manifest",
      skillmateCommands.applySkillMateManifest,
      { path: skillMateManifestPath },
      skillMateManifestPreviewToken.planToken,
    );
    if (!execution.started) return;
    setApplyingSkillMateManifest(true);
    try {
      const result = await execution.promise;
      showToast(String(result), "success");
      setSkillMateManifestPreview(null);
      setSkillMateManifestPreviewToken(null);
      await loadData();
    } catch (e) {
      showToast(`应用失败: ${e}`, "error");
    } finally {
      setApplyingSkillMateManifest(false);
    }
  }

  async function saveCurrentSkillProfile() {
    if (!skillProfileName.trim()) {
      showToast("请输入 Profile 名称", "error");
      return;
    }
    try {
      const result = await skillmateApi.profiles.saveCurrent(skillProfileName, skillProfileDescription);
      setSkillProfiles(result);
      setSkillProfileName("");
      setSkillProfileDescription("");
      setSkillProfilePreview(null);
      showToast("已保存当前 Skill 组合", "success");
    } catch (e) {
      showToast(`保存 Profile 失败: ${e}`, "error");
    }
  }

  async function previewSkillProfile(profileId) {
    setPreviewingSkillProfile(true);
    try {
      const result = await skillmateApi.profiles.preview(profileId);
      setSkillProfilePreview(result);
      showToast("已生成 Profile 预览", result.manifest_preview?.can_apply ? "success" : "warn");
    } catch (e) {
      setSkillProfilePreview(null);
      showToast(`预览 Profile 失败: ${e}`, "error");
    } finally {
      setPreviewingSkillProfile(false);
    }
  }

  async function applySkillProfile(profileId) {
    if (skillProfilePreview?.profile?.id !== profileId || !skillProfilePreview?.plan_token) {
      showToast("Profile 计划缺失或已切换，请重新预览", "warn");
      return;
    }
    if (!skillProfilePreview.manifest_preview?.can_apply || skillProfilePreview.profile_issues?.length) {
      showToast("Profile 存在冲突或格式问题，无法应用", "error");
      return;
    }
    const execution = planExecutorRef.current.run(
      `profile-${profileId}`,
      skillmateCommands.applySkillProfile,
      { profileId },
      skillProfilePreview.plan_token,
    );
    if (!execution.started) return;
    setApplyingSkillProfile(true);
    try {
      const result = await execution.promise;
      showToast(String(result), "success");
      setSkillProfilePreview(null);
      await loadSkillProfiles();
      await loadData();
    } catch (e) {
      showToast(`应用 Profile 失败: ${e}`, "error");
    } finally {
      setApplyingSkillProfile(false);
    }
  }

  async function rollbackSkillProfile() {
    setApplyingSkillProfile(true);
    try {
      const result = await skillmateApi.profiles.rollback();
      showToast(String(result), "success");
      setSkillProfilePreview(null);
      await loadSkillProfiles();
      await loadData();
    } catch (e) {
      showToast(`回滚 Profile 失败: ${e}`, "error");
    } finally {
      setApplyingSkillProfile(false);
    }
  }

  async function exportScenarioManifestFile() {
    if (!scenarioManifestPath.trim()) {
      showToast("请输入场景 manifest 文件路径", "error");
      return;
    }
    try {
      const result = await skillmateApi.scenarios.exportManifest(scenarioManifestPath);
      showToast(String(result), "success");
    } catch (e) {
      showToast(`导出失败: ${e}`, "error");
    }
  }

  async function previewImportScenarioManifestFile() {
    if (!scenarioManifestPath.trim()) {
      showToast("请输入场景 manifest 文件路径", "error");
      return;
    }
    setPreviewingScenarioManifest(true);
    try {
      const result = await skillmateApi.scenarios.previewManifest(scenarioManifestPath, scenarioManifestMode);
      setScenarioManifestPreview(result);
      setScenarioManifestPreviewToken({
        ...buildImportPreviewToken({
          path: scenarioManifestPath,
          mode: scenarioManifestMode,
        }),
        planToken: result.plan_token || "",
      });
      showToast("已生成场景导入预览", "success");
    } catch (e) {
      setScenarioManifestPreview(null);
      setScenarioManifestPreviewToken(null);
      showToast(`预览失败: ${e}`, "error");
    } finally {
      setPreviewingScenarioManifest(false);
    }
  }

  async function importScenarioManifestFile() {
    if (!scenarioManifestPath.trim()) {
      showToast("请输入场景 manifest 文件路径", "error");
      return;
    }
    if (!scenarioManifestPreview) {
      showToast("请先预览场景导入内容", "warn");
      return;
    }
    if (!scenarioManifestPreviewCurrent) {
      showToast("场景导入参数已变化，请重新预览", "warn");
      return;
    }
    if (!scenarioManifestPreviewToken?.planToken) {
      showToast("场景导入计划缺失，请重新预览", "warn");
      return;
    }
    const execution = planExecutorRef.current.run(
      "scenario-manifest",
      skillmateCommands.importScenarioManifest,
      { path: scenarioManifestPath, mode: scenarioManifestMode },
      scenarioManifestPreviewToken.planToken,
    );
    if (!execution.started) return;
    setApplyingScenarioManifest(true);
    try {
      const result = await execution.promise;
      showToast(String(result), "success");
      setScenarioManifestPreview(null);
      setScenarioManifestPreviewToken(null);
      await loadData();
    } catch (e) {
      showToast(`导入失败: ${e}`, "error");
    } finally {
      setApplyingScenarioManifest(false);
    }
  }

  return {
    library: {
      exportPath,
      setExportPath,
      importPath,
      importMode,
      importPreview,
      previewingImport,
      applyingImport,
      importPreviewCurrent,
      updateImportPath,
      updateImportMode,
      exportLibraryFile,
      previewImportLibraryFile,
      importLibraryFile,
    },
    scenarios: {
      path: scenarioManifestPath,
      mode: scenarioManifestMode,
      preview: scenarioManifestPreview,
      previewing: previewingScenarioManifest,
      applying: applyingScenarioManifest,
      previewCurrent: scenarioManifestPreviewCurrent,
      updatePath: updateScenarioManifestPath,
      updateMode: updateScenarioManifestMode,
      exportFile: exportScenarioManifestFile,
      previewFile: previewImportScenarioManifestFile,
      importFile: importScenarioManifestFile,
    },
    manifest: {
      path: skillMateManifestPath,
      projectRoot: projectManifestRoot,
      preview: skillMateManifestPreview,
      previewing: previewingSkillMateManifest,
      applying: applyingSkillMateManifest,
      previewCurrent: skillMateManifestPreviewCurrent,
      updatePath: updateSkillMateManifestPath,
      setProjectRoot: setProjectManifestRoot,
      exportFile: exportSkillMateManifestFile,
      exportProjectFile: exportProjectSkillMateManifestFile,
      previewFile: previewSkillMateManifestFile,
      applyFile: applySkillMateManifestFile,
    },
    profiles: {
      store: skillProfiles,
      name: skillProfileName,
      description: skillProfileDescription,
      preview: skillProfilePreview,
      previewing: previewingSkillProfile,
      applying: applyingSkillProfile,
      setName: setSkillProfileName,
      setDescription: setSkillProfileDescription,
      reload: loadSkillProfiles,
      save: saveCurrentSkillProfile,
      previewOne: previewSkillProfile,
      applyOne: applySkillProfile,
      rollback: rollbackSkillProfile,
    },
  };
}
