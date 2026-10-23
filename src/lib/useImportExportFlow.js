import { useCallback, useEffect, useRef, useState } from "react";
import { buildImportPreviewToken, isImportPreviewCurrent } from "./skillmate.mjs";
import { createSingleFlightPlanExecutor } from "./plannedAction.mjs";
import { invokeSkillMateCommand, skillmateApi, skillmateCommands } from "./skillmateApi.js";
import { useI18n } from "./i18n.jsx";
import { toUserErrorMessage } from "./errorMessage.mjs";

// 后端结果只在是非空字符串时直接展示，否则回退到本地化文案。
function resultMessage(result, fallback) {
  return typeof result === "string" && result.trim() ? result : fallback;
}

export function useImportExportFlow({ showToast, loadData }) {
  const { t, language } = useI18n();
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
  const mountedRef = useRef(false);

  const loadSkillProfiles = useCallback(async () => {
    try {
      const result = await skillmateApi.profiles.get();
      if (!mountedRef.current) return;
      setSkillProfiles(result);
    } catch (e) {
      if (!mountedRef.current) return;
      showToast(t("profile.toast.loadFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    }
  }, [showToast, t]);

  useEffect(() => {
    mountedRef.current = true;
    loadSkillProfiles();
    return () => {
      mountedRef.current = false;
    };
  }, [loadSkillProfiles]);

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

  async function exportLibraryFile() {
    if (!exportPath.trim()) {
      showToast(t("data.toast.enterExportPath"), "error");
      return;
    }
    try {
      const result = await skillmateApi.library.export(exportPath);
      showToast(language === "en" ? t("data.toast.exported") : resultMessage(result, t("data.toast.exported")), "success");
    } catch (e) {
      showToast(t("data.toast.exportFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    }
  }

  async function previewImportLibraryFile() {
    if (!importPath.trim()) {
      showToast(t("data.toast.enterImportPath"), "error");
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
      showToast(t("data.toast.previewReady"), "success");
    } catch (e) {
      setImportPreview(null);
      setImportPreviewToken(null);
      showToast(t("data.toast.previewFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    } finally {
      setPreviewingImport(false);
    }
  }

  async function importLibraryFile() {
    if (!importPath.trim()) {
      showToast(t("data.toast.enterImportPath"), "error");
      return;
    }
    if (!importPreview) {
      showToast(t("data.toast.previewFirst"), "warn");
      return;
    }
    if (!importPreviewCurrent) {
      showToast(t("data.toast.previewChanged"), "warn");
      return;
    }
    if (!importPreviewToken?.planToken) {
      showToast(t("data.toast.planMissing"), "warn");
      return;
    }
    const execution = planExecutorRef.current.run(
      "library-import",
      skillmateCommands.importLibrary,
      { path: importPath, mode: importMode },
      importPreviewToken.planToken,
    );
    if (!execution.started) {
      if (execution.error) {
        showToast(t("data.toast.importFailed", { message: t("common.planMissing") }), "error");
      }
      return;
    }
    setApplyingImport(true);
    try {
      const result = await execution.promise;
      showToast(language === "en" ? t("data.toast.imported") : resultMessage(result, t("data.toast.imported")), "success");
      setImportPreview(null);
      setImportPreviewToken(null);
      await loadData();
    } catch (e) {
      showToast(t("data.toast.importFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    } finally {
      setApplyingImport(false);
    }
  }

  async function exportSkillMateManifestFile() {
    if (!skillMateManifestPath.trim()) {
      showToast(t("manifest.toast.enterPath"), "error");
      return;
    }
    try {
      const result = await skillmateApi.manifests.exportSkillMate(skillMateManifestPath);
      showToast(language === "en" ? t("manifest.toast.exported") : resultMessage(result, t("manifest.toast.exported")), "success");
    } catch (e) {
      showToast(t("manifest.toast.exportFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    }
  }

  async function exportProjectSkillMateManifestFile() {
    if (!projectManifestRoot.trim()) {
      showToast(t("manifest.toast.enterProject"), "error");
      return;
    }
    try {
      const path = await skillmateApi.manifests.exportProject(projectManifestRoot.trim());
      updateSkillMateManifestPath(String(path));
      showToast(t("manifest.toast.projectExported", { path: String(path) }), "success");
    } catch (e) {
      showToast(t("manifest.toast.projectExportFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    }
  }

  async function previewSkillMateManifestFile() {
    if (!skillMateManifestPath.trim()) {
      showToast(t("manifest.toast.enterPath"), "error");
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
      showToast(t("manifest.toast.previewReady"), result.can_apply ? "success" : "warn");
    } catch (e) {
      setSkillMateManifestPreview(null);
      setSkillMateManifestPreviewToken(null);
      showToast(t("manifest.toast.previewFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    } finally {
      setPreviewingSkillMateManifest(false);
    }
  }

  async function applySkillMateManifestFile() {
    if (!skillMateManifestPreview) {
      showToast(t("manifest.toast.previewFirst"), "warn");
      return;
    }
    if (!skillMateManifestPreviewCurrent) {
      showToast(t("manifest.toast.previewChanged"), "warn");
      return;
    }
    if (!skillMateManifestPreviewToken?.planToken) {
      showToast(t("manifest.toast.planMissing"), "warn");
      return;
    }
    if (!skillMateManifestPreview.can_apply) {
      showToast(t("manifest.toast.blocked"), "error");
      return;
    }
    const execution = planExecutorRef.current.run(
      "skillmate-manifest",
      skillmateCommands.applySkillMateManifest,
      { path: skillMateManifestPath },
      skillMateManifestPreviewToken.planToken,
    );
    if (!execution.started) {
      if (execution.error) {
        showToast(t("manifest.toast.applyFailed", { message: t("common.planMissing") }), "error");
      }
      return;
    }
    setApplyingSkillMateManifest(true);
    try {
      const result = await execution.promise;
      showToast(language === "en" ? t("manifest.toast.applied") : resultMessage(result, t("manifest.toast.applied")), "success");
      setSkillMateManifestPreview(null);
      setSkillMateManifestPreviewToken(null);
      await loadData();
    } catch (e) {
      showToast(t("manifest.toast.applyFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    } finally {
      setApplyingSkillMateManifest(false);
    }
  }

  async function saveCurrentSkillProfile() {
    if (!skillProfileName.trim()) {
      showToast(t("profile.toast.enterName"), "error");
      return;
    }
    try {
      const result = await skillmateApi.profiles.saveCurrent(skillProfileName, skillProfileDescription);
      setSkillProfiles(result);
      setSkillProfileName("");
      setSkillProfileDescription("");
      setSkillProfilePreview(null);
      showToast(t("profile.toast.saved"), "success");
    } catch (e) {
      showToast(t("profile.toast.saveFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    }
  }

  async function previewSkillProfile(profileId) {
    setPreviewingSkillProfile(true);
    try {
      const result = await skillmateApi.profiles.preview(profileId);
      setSkillProfilePreview(result);
      showToast(t("profile.toast.previewReady"), result.manifest_preview?.can_apply ? "success" : "warn");
    } catch (e) {
      setSkillProfilePreview(null);
      showToast(t("profile.toast.previewFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    } finally {
      setPreviewingSkillProfile(false);
    }
  }

  async function applySkillProfile(profileId) {
    if (skillProfilePreview?.profile?.id !== profileId || !skillProfilePreview?.plan_token) {
      showToast(t("profile.toast.planMissing"), "warn");
      return;
    }
    if (!skillProfilePreview.manifest_preview?.can_apply || skillProfilePreview.profile_issues?.length) {
      showToast(t("profile.toast.blocked"), "error");
      return;
    }
    const execution = planExecutorRef.current.run(
      `profile-${profileId}`,
      skillmateCommands.applySkillProfile,
      { profileId },
      skillProfilePreview.plan_token,
    );
    if (!execution.started) {
      if (execution.error) {
        showToast(t("profile.toast.applyFailed", { message: t("common.planMissing") }), "error");
      }
      return;
    }
    setApplyingSkillProfile(true);
    try {
      const result = await execution.promise;
      showToast(language === "en" ? t("profile.toast.applied") : resultMessage(result, t("profile.toast.applied")), "success");
      setSkillProfilePreview(null);
      await loadSkillProfiles();
      await loadData();
    } catch (e) {
      showToast(t("profile.toast.applyFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    } finally {
      setApplyingSkillProfile(false);
    }
  }

  async function rollbackSkillProfile() {
    setApplyingSkillProfile(true);
    try {
      const result = await skillmateApi.profiles.rollback();
      showToast(language === "en" ? t("profile.toast.rolledBack") : resultMessage(result, t("profile.toast.rolledBack")), "success");
      setSkillProfilePreview(null);
      await loadSkillProfiles();
      await loadData();
    } catch (e) {
      showToast(t("profile.toast.rollbackFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    } finally {
      setApplyingSkillProfile(false);
    }
  }

  async function exportScenarioManifestFile() {
    if (!scenarioManifestPath.trim()) {
      showToast(t("scenarioManifest.toast.enterPath"), "error");
      return;
    }
    try {
      const result = await skillmateApi.scenarios.exportManifest(scenarioManifestPath);
      showToast(language === "en" ? t("scenarioManifest.toast.exported") : resultMessage(result, t("scenarioManifest.toast.exported")), "success");
    } catch (e) {
      showToast(t("scenarioManifest.toast.exportFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    }
  }

  async function previewImportScenarioManifestFile() {
    if (!scenarioManifestPath.trim()) {
      showToast(t("scenarioManifest.toast.enterPath"), "error");
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
      showToast(t("scenarioManifest.toast.previewReady"), "success");
    } catch (e) {
      setScenarioManifestPreview(null);
      setScenarioManifestPreviewToken(null);
      showToast(t("scenarioManifest.toast.previewFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    } finally {
      setPreviewingScenarioManifest(false);
    }
  }

  async function importScenarioManifestFile() {
    if (!scenarioManifestPath.trim()) {
      showToast(t("scenarioManifest.toast.enterPath"), "error");
      return;
    }
    if (!scenarioManifestPreview) {
      showToast(t("scenarioManifest.toast.previewFirst"), "warn");
      return;
    }
    if (!scenarioManifestPreviewCurrent) {
      showToast(t("scenarioManifest.toast.previewChanged"), "warn");
      return;
    }
    if (!scenarioManifestPreviewToken?.planToken) {
      showToast(t("scenarioManifest.toast.planMissing"), "warn");
      return;
    }
    const execution = planExecutorRef.current.run(
      "scenario-manifest",
      skillmateCommands.importScenarioManifest,
      { path: scenarioManifestPath, mode: scenarioManifestMode },
      scenarioManifestPreviewToken.planToken,
    );
    if (!execution.started) {
      if (execution.error) {
        showToast(t("scenarioManifest.toast.importFailed", { message: t("common.planMissing") }), "error");
      }
      return;
    }
    setApplyingScenarioManifest(true);
    try {
      const result = await execution.promise;
      showToast(language === "en" ? t("scenarioManifest.toast.imported") : resultMessage(result, t("scenarioManifest.toast.imported")), "success");
      setScenarioManifestPreview(null);
      setScenarioManifestPreviewToken(null);
      await loadData();
    } catch (e) {
      showToast(t("scenarioManifest.toast.importFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
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
