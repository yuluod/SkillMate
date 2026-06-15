import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  SUPPORTED_INSTALL_SOURCES,
  buildImportPreviewToken,
  buildInstallCommandPreview,
  buildInstallDetectionView,
  buildInstallPreviewToken,
  buildInstallPreviewView,
  buildInstallPrimaryAction,
  buildInstallStructureSummary,
  isInstallPreviewCurrent,
  isImportPreviewCurrent,
  shouldShowInstallAdvancedOptions,
  shouldShowProjectLinkOption,
} from "./skillmate.mjs";

export function useInstallFlow({ installOpen, assistants, setInstallOpen, showToast, loadData, setLoading }) {
  const [src, setSrc] = useState("git");
  const [pkg, setPkg] = useState("");
  const [installDetection, setInstallDetection] = useState(null);
  const [installStructurePreview, setInstallStructurePreview] = useState(null);
  const [previewingInstall, setPreviewingInstall] = useState(false);
  const [installAssistant, setInstallAssistant] = useState("");
  const [installMode, setInstallMode] = useState("copy");
  const [projectPath, setProjectPath] = useState("");
  const [projectTargetPreview, setProjectTargetPreview] = useState([]);
  const [previewingProjectTargets, setPreviewingProjectTargets] = useState(false);
  const [installPreviewToken, setInstallPreviewToken] = useState(null);
  const [installDetailsOpen, setInstallDetailsOpen] = useState(false);
  const [installAdvancedOpen, setInstallAdvancedOpen] = useState(false);

  useEffect(() => {
    setInstallAssistant((current) => (
      assistants.some((assistant) => assistant.name === current)
        ? current
        : (assistants[0]?.name || "")
    ));
  }, [assistants]);

  useEffect(() => {
    if (!installOpen) {
      setInstallDetailsOpen(false);
      setInstallAdvancedOpen(false);
    }
  }, [installOpen]);

  useEffect(() => {
    setInstallStructurePreview(null);
    setInstallPreviewToken(null);
  }, [installAssistant, installMode, pkg, projectPath, src]);

  useEffect(() => {
    if (installMode === "symlink" && src !== "local") {
      setInstallMode("copy");
    }
  }, [installMode, src]);

  useEffect(() => {
    if (!installOpen || installMode !== "symlink" || !projectPath.trim()) {
      setProjectTargetPreview([]);
      return undefined;
    }
    let cancelled = false;
    const timer = setTimeout(async () => {
      setPreviewingProjectTargets(true);
      try {
        const result = await invoke("preview_project_skill_targets", { projectPath });
        if (!cancelled) setProjectTargetPreview(result);
      } catch {
        if (!cancelled) setProjectTargetPreview([]);
      } finally {
        if (!cancelled) setPreviewingProjectTargets(false);
      }
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [installMode, installOpen, projectPath]);

  useEffect(() => {
    if (!installOpen || !pkg.trim()) {
      setInstallDetection(null);
      return undefined;
    }
    let cancelled = false;
    const timer = setTimeout(async () => {
      try {
        const result = await invoke("detect_install_source", { input: pkg.trim() });
        if (cancelled) return;
        setInstallDetection(result);
        if (SUPPORTED_INSTALL_SOURCES.includes(result.normalized_source) && result.normalized_source !== src) {
          setSrc(result.normalized_source);
          setInstallStructurePreview(null);
        }
      } catch (e) {
        if (!cancelled) {
          setInstallDetection({
            detector: "rules",
            source_kind: "unknown",
            normalized_source: "",
            original_input: pkg.trim(),
            confidence: "low",
            warnings: ["unrecognized_input", String(e)],
            needs_model: true,
          });
        }
      }
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [installOpen, pkg, src]);

  useEffect(() => {
    if (
      !installOpen
      || src !== "local"
      || !pkg.trim()
      || !installAssistant
      || (installMode === "symlink" && !projectPath.trim())
    ) {
      setInstallStructurePreview(null);
      setInstallPreviewToken(null);
      return undefined;
    }
    let cancelled = false;
    const timer = setTimeout(async () => {
      try {
        const token = buildInstallPreviewToken({
          packageValue: pkg,
          source: src,
          assistantName: installAssistant,
          installMode,
          projectPath,
        });
        const result = await invoke("preview_install_skill", {
          package: pkg.trim(),
          source: src,
          assistantName: installAssistant,
          installMode,
          projectPath,
        });
        if (!cancelled) {
          setInstallStructurePreview(result);
          setInstallPreviewToken(token);
        }
      } catch (e) {
        if (!cancelled) {
          setInstallStructurePreview({
            can_apply: false,
            structure_status: "nonstandard",
            structure_features: [],
            structure_warnings: ["structure_preview_failed", String(e)],
            manifest_title: null,
            manifest_description: null,
          });
          setInstallPreviewToken(buildInstallPreviewToken({
            packageValue: pkg,
            source: src,
            assistantName: installAssistant,
            installMode,
            projectPath,
          }));
        }
      }
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [installAssistant, installMode, installOpen, pkg, projectPath, src]);

  const installDetectionView = useMemo(
    () => buildInstallDetectionView(installDetection),
    [installDetection]
  );
  const installPreviewView = useMemo(
    () => buildInstallPreviewView(installStructurePreview),
    [installStructurePreview]
  );
  const cmd = useMemo(
    () => buildInstallCommandPreview({ source: src, assistantName: installAssistant, installMode, projectPath }),
    [installAssistant, installMode, projectPath, src]
  );
  const installPreviewCurrent = useMemo(
    () => isInstallPreviewCurrent({
      previewToken: installPreviewToken,
      packageValue: pkg,
      source: src,
      assistantName: installAssistant,
      installMode,
      projectPath,
    }),
    [installAssistant, installMode, installPreviewToken, pkg, projectPath, src]
  );
  const installPrimaryAction = useMemo(
    () => buildInstallPrimaryAction({
      packageValue: pkg,
      preview: installStructurePreview,
      previewCurrent: installPreviewCurrent,
      previewingInstall,
      loading: false,
    }),
    [installPreviewCurrent, installStructurePreview, pkg, previewingInstall]
  );
  const showProjectLinkOption = useMemo(
    () => shouldShowProjectLinkOption({ source: src, detection: installDetection }),
    [installDetection, src]
  );
  const showInstallAdvancedOptions = useMemo(
    () => shouldShowInstallAdvancedOptions({ advancedOpen: installAdvancedOpen, detection: installDetection }),
    [installAdvancedOpen, installDetection]
  );

  const previewInstall = useCallback(async () => {
    if (!pkg) { showToast("请输入仓库地址", "error"); return; }
    if (!installAssistant) { showToast("请选择目标助手", "error"); return; }
    if (installMode === "symlink" && !projectPath.trim()) { showToast("请输入项目路径", "error"); return; }
    setPreviewingInstall(true);
    try {
      const token = buildInstallPreviewToken({
        packageValue: pkg,
        source: src,
        assistantName: installAssistant,
        installMode,
        projectPath,
      });
      const result = await invoke("preview_install_skill", {
        package: pkg.trim(),
        source: src,
        assistantName: installAssistant,
        installMode,
        projectPath,
      });
      setInstallStructurePreview(result);
      setInstallPreviewToken(token);
      showToast(result.can_apply ? "安装预览完成" : `预览完成：${result.message}`, result.can_apply ? "success" : "error");
    } catch (e) {
      setInstallStructurePreview({
        can_install: false,
        can_apply: false,
        message: String(e),
        structure_status: "nonstandard",
        structure_features: [],
        structure_warnings: ["structure_preview_failed"],
        manifest_title: null,
        manifest_description: null,
      });
      setInstallPreviewToken(buildInstallPreviewToken({
        packageValue: pkg,
        source: src,
        assistantName: installAssistant,
        installMode,
        projectPath,
      }));
      showToast(`预览失败: ${e}`, "error");
    } finally {
      setPreviewingInstall(false);
    }
  }, [installAssistant, installMode, pkg, projectPath, showToast, src]);

  const install = useCallback(async () => {
    if (!pkg) { showToast("请输入包名", "error"); return; }
    if (!installAssistant) { showToast("请选择目标助手", "error"); return; }
    if (installMode === "symlink" && !projectPath.trim()) { showToast("请输入项目路径", "error"); return; }
    if (!installStructurePreview) {
      showToast("请先预览安装计划", "warn");
      return;
    }
    if (!installPreviewCurrent) {
      showToast("安装计划已过期，请重新检查结构", "warn");
      return;
    }
    if (!(installStructurePreview.can_apply ?? installStructurePreview.can_install)) {
      showToast(`当前预览不可安装: ${installStructurePreview.message || "存在冲突"}`, "error");
      return;
    }
    setLoading(true);
    try {
      const r = await invoke("install_skill", {
        package: pkg.trim(),
        source: src,
        assistantName: installAssistant,
        installMode,
        projectPath,
      });
      if (r.success) {
        const structureSummary = buildInstallStructureSummary(r);
        showToast(structureSummary ? `安装成功，${structureSummary}` : "安装成功", "success");
        setInstallOpen(false);
        setPkg("");
        setInstallDetection(null);
        setInstallStructurePreview(null);
        setInstallPreviewToken(null);
        setInstallDetailsOpen(false);
        setInstallAdvancedOpen(false);
        setInstallMode("copy");
        setProjectPath("");
        await loadData();
      } else {
        showToast(`安装失败: ${r.message}`, "error");
      }
    } catch (e) {
      showToast(`安装失败: ${e}`, "error");
    } finally {
      setLoading(false);
    }
  }, [installAssistant, installMode, installPreviewCurrent, installStructurePreview, loadData, pkg, projectPath, setInstallOpen, setLoading, showToast, src]);

  const runInstallPrimaryAction = useCallback(() => {
    if (installPrimaryAction.action === "install") {
      install();
    } else {
      previewInstall();
    }
  }, [install, installPrimaryAction.action, previewInstall]);

  return {
    src,
    setSrc,
    pkg,
    setPkg,
    installDetectionView,
    installStructurePreview,
    installPreviewView,
    previewingInstall,
    installAssistant,
    setInstallAssistant,
    installMode,
    setInstallMode,
    projectPath,
    setProjectPath,
    projectTargetPreview,
    previewingProjectTargets,
    installPreviewCurrent,
    installPrimaryAction,
    runInstallPrimaryAction,
    showProjectLinkOption,
    installDetailsOpen,
    setInstallDetailsOpen,
    installAdvancedOpen,
    setInstallAdvancedOpen,
    showInstallAdvancedOptions,
    cmd,
    previewInstall,
    install,
  };
}

export function useImportExportFlow({ showToast, loadData }) {
  const [exportPath, setExportPath] = useState("~/skillmate-export.json");
  const [importPath, setImportPath] = useState("~/skillmate-export.json");
  const [importMode, setImportMode] = useState("merge");
  const [importPreview, setImportPreview] = useState(null);
  const [importPreviewToken, setImportPreviewToken] = useState(null);
  const [previewingImport, setPreviewingImport] = useState(false);
  const [scenarioManifestPath, setScenarioManifestPath] = useState("");
  const [scenarioManifestMode, setScenarioManifestMode] = useState("merge");
  const [scenarioManifestPreview, setScenarioManifestPreview] = useState(null);
  const [scenarioManifestPreviewToken, setScenarioManifestPreviewToken] = useState(null);
  const [previewingScenarioManifest, setPreviewingScenarioManifest] = useState(false);
  const [skillMateManifestPath, setSkillMateManifestPath] = useState("~/skillmate.toml");
  const [skillMateManifestPreview, setSkillMateManifestPreview] = useState(null);
  const [skillMateManifestPreviewToken, setSkillMateManifestPreviewToken] = useState(null);
  const [previewingSkillMateManifest, setPreviewingSkillMateManifest] = useState(false);
  const [skillProfiles, setSkillProfiles] = useState({ version: 1, active_profile_id: null, profiles: [] });
  const [skillProfileName, setSkillProfileName] = useState("");
  const [skillProfileDescription, setSkillProfileDescription] = useState("");
  const [skillProfilePreview, setSkillProfilePreview] = useState(null);
  const [previewingSkillProfile, setPreviewingSkillProfile] = useState(false);
  const [applyingSkillProfile, setApplyingSkillProfile] = useState(false);

  useEffect(() => {
    loadSkillProfiles();
  }, []);

  const importPreviewCurrent = isImportPreviewCurrent({
    previewToken: importPreviewToken,
    path: importPath,
    mode: importMode,
  });
  const scenarioManifestPreviewCurrent = isImportPreviewCurrent({
    previewToken: scenarioManifestPreviewToken,
    path: scenarioManifestPath,
    mode: scenarioManifestMode,
  });
  const skillMateManifestPreviewCurrent = isImportPreviewCurrent({
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
      const result = await invoke("get_skill_profiles");
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
      const result = await invoke("export_library", { path: exportPath });
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
      const result = await invoke("preview_import_library", { path: importPath, mode: importMode });
      setImportPreview(result);
      setImportPreviewToken(buildImportPreviewToken({ path: importPath, mode: importMode }));
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
    try {
      const result = await invoke("import_library", { path: importPath, mode: importMode });
      showToast(String(result), "success");
      setImportPreview(null);
      setImportPreviewToken(null);
      await loadData();
    } catch (e) {
      showToast(`导入失败: ${e}`, "error");
    }
  }

  async function exportSkillMateManifestFile() {
    if (!skillMateManifestPath.trim()) {
      showToast("请输入 skillmate.toml 路径", "error");
      return;
    }
    try {
      const result = await invoke("export_skillmate_manifest", { path: skillMateManifestPath });
      showToast(String(result), "success");
    } catch (e) {
      showToast(`导出失败: ${e}`, "error");
    }
  }

  async function previewSkillMateManifestFile() {
    if (!skillMateManifestPath.trim()) {
      showToast("请输入 skillmate.toml 路径", "error");
      return;
    }
    setPreviewingSkillMateManifest(true);
    try {
      const result = await invoke("preview_apply_skillmate_manifest", { path: skillMateManifestPath });
      setSkillMateManifestPreview(result);
      setSkillMateManifestPreviewToken(buildImportPreviewToken({ path: skillMateManifestPath, mode: "apply" }));
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
    try {
      const result = await invoke("apply_skillmate_manifest", { path: skillMateManifestPath });
      showToast(String(result), "success");
      setSkillMateManifestPreview(null);
      setSkillMateManifestPreviewToken(null);
      await loadData();
    } catch (e) {
      showToast(`应用失败: ${e}`, "error");
    }
  }

  async function saveCurrentSkillProfile() {
    if (!skillProfileName.trim()) {
      showToast("请输入 Profile 名称", "error");
      return;
    }
    try {
      const result = await invoke("save_current_skill_profile", {
        name: skillProfileName,
        description: skillProfileDescription,
      });
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
      const result = await invoke("preview_apply_skill_profile", { profileId });
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
    setApplyingSkillProfile(true);
    try {
      const result = await invoke("apply_skill_profile", { profileId });
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
      const result = await invoke("rollback_skill_profile");
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
      const result = await invoke("export_scenario_manifest", { path: scenarioManifestPath });
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
      const result = await invoke("preview_import_scenario_manifest", {
        path: scenarioManifestPath,
        mode: scenarioManifestMode,
      });
      setScenarioManifestPreview(result);
      setScenarioManifestPreviewToken(buildImportPreviewToken({
        path: scenarioManifestPath,
        mode: scenarioManifestMode,
      }));
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
    try {
      const result = await invoke("import_scenario_manifest", {
        path: scenarioManifestPath,
        mode: scenarioManifestMode,
      });
      showToast(String(result), "success");
      setScenarioManifestPreview(null);
      setScenarioManifestPreviewToken(null);
      await loadData();
    } catch (e) {
      showToast(`导入失败: ${e}`, "error");
    }
  }

  return {
    exportPath,
    setExportPath,
    importPath,
    importMode,
    importPreview,
    previewingImport,
    importPreviewCurrent,
    scenarioManifestPath,
    scenarioManifestMode,
    scenarioManifestPreview,
    previewingScenarioManifest,
    scenarioManifestPreviewCurrent,
    skillMateManifestPath,
    skillMateManifestPreview,
    previewingSkillMateManifest,
    skillMateManifestPreviewCurrent,
    skillProfiles,
    skillProfileName,
    skillProfileDescription,
    skillProfilePreview,
    previewingSkillProfile,
    applyingSkillProfile,
    updateImportPath,
    updateImportMode,
    updateScenarioManifestPath,
    updateScenarioManifestMode,
    updateSkillMateManifestPath,
    setSkillProfileName,
    setSkillProfileDescription,
    exportLibraryFile,
    previewImportLibraryFile,
    importLibraryFile,
    exportSkillMateManifestFile,
    previewSkillMateManifestFile,
    applySkillMateManifestFile,
    loadSkillProfiles,
    saveCurrentSkillProfile,
    previewSkillProfile,
    applySkillProfile,
    rollbackSkillProfile,
    exportScenarioManifestFile,
    previewImportScenarioManifestFile,
    importScenarioManifestFile,
  };
}

export function useUpdateFlow({ updateable, showToast, loadData }) {
  const [updateState, setUpdateState] = useState({});

  const resetUpdateState = useCallback(() => setUpdateState({}), []);

  const getSyncInfo = useCallback((skill) => {
    const state = updateState[skill.path] || {};
    return {
      originKind: state.originKind || skill.origin_kind,
      originLocator: state.originLocator || skill.origin_locator,
      resolvedLocator: state.resolvedLocator || skill.resolved_locator,
      trackingRef: state.trackingRef || skill.tracking_ref,
      installedRef: state.installedRef || skill.installed_ref,
      latestRef: state.latestRef || skill.latest_ref,
      syncState: state.syncState || skill.sync_state,
      message: state.message || skill.sync_message || "待检查",
      lagCount: state.lagCount ?? skill.lag_count ?? 0,
      lastProbeAt: state.lastProbeAt ?? skill.last_probe_at,
      lastSyncAt: state.lastSyncAt ?? skill.last_sync_at,
      managedByApp: state.managedByApp ?? skill.managed_by_app,
      canSync: state.canSync ?? skill.can_sync ?? false,
      checking: Boolean(state.checking),
      updating: Boolean(state.updating)
    };
  }, [updateState]);

  const checkAllUpdates = useCallback(async () => {
    if (updateable.length === 0) return;
    const CONCURRENCY = 4;
    let idx = 0;
    const initial = {};
    updateable.forEach(s => { initial[s.path] = { ...(updateState[s.path] || {}), checking: true }; });
    setUpdateState(prev => ({ ...prev, ...initial }));

    let cancelled = false;
    async function worker() {
      while (idx < updateable.length && !cancelled) {
        const skill = updateable[idx++];
        try {
          const result = await invoke("check_update", { path: skill.path });
          if (!cancelled) setUpdateState(prev => ({ ...prev, [skill.path]: { checking: false, updating: false, ...result } }));
        } catch (error) {
          if (!cancelled) setUpdateState(prev => ({ ...prev, [skill.path]: { checking: false, updating: false, hasUpdate: false, lagCount: 0, message: `检查失败: ${error}`, syncState: "failed" } }));
        }
      }
    }
    const workers = Array.from({ length: Math.min(CONCURRENCY, updateable.length) }, () => worker());
    await Promise.all(workers);
    showToast("全部检查完成", "success");
  }, [showToast, updateState, updateable]);

  const checkUpdate = useCallback(async (path) => {
    try {
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), checking: true } }));
      const r = await invoke("check_update", { path, force: true });
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), checking: false, updating: false, ...r } }));
      const hasUpdate = typeof r.hasUpdate === "boolean" ? r.hasUpdate : r.syncState === "behind";
      showToast(r.message || (hasUpdate ? "有更新" : "已是最新"), hasUpdate ? "warn" : "success");
    } catch (e) {
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), checking: false } }));
      showToast(`检查失败: ${e}`, "error");
    }
  }, [showToast]);

  const updateSkill = useCallback(async (path) => {
    try {
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), updating: true } }));
      const result = await invoke("update_from_upstream", { path });
      showToast(String(result || "更新成功"), "success");
      await checkUpdate(path);
      await loadData();
    } catch (e) {
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), updating: false } }));
      showToast(`更新失败: ${e}`, "error");
    }
  }, [checkUpdate, loadData, showToast]);

  return {
    updateState,
    resetUpdateState,
    getSyncInfo,
    checkAllUpdates,
    checkUpdate,
    updateSkill,
  };
}
