import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  SUPPORTED_INSTALL_SOURCES,
  buildInstallCommandPreview,
  buildInstallDetectionView,
  buildInstallPreviewToken,
  buildInstallPreviewView,
  buildInstallPrimaryAction,
  buildInstallStructureSummary,
  isInstallPreviewCurrent,
  shouldShowInstallAdvancedOptions,
  shouldShowProjectLinkOption,
} from "./skillmate.mjs";
import { createSingleFlightPlanExecutor } from "./plannedAction.mjs";
import { invokeSkillMateCommand, skillmateApi, skillmateCommands } from "./skillmateApi.js";
import { useI18n } from "./i18n.jsx";

export function useInstallFlow({ installOpen, assistants, setInstallOpen, showToast, loadData, setLoading }) {
  const { t, language } = useI18n();
  const [sourceInput, setSourceInput] = useState({ kind: "git", package: "", manual: false });
  const { kind: src, package: pkg } = sourceInput;
  const [installDetection, setInstallDetection] = useState(null);
  const [installStructurePreview, setInstallStructurePreview] = useState(null);
  const [previewingInstall, setPreviewingInstall] = useState(false);
  const [installAssistant, setInstallAssistant] = useState("");
  const [installMode, setInstallMode] = useState("copy");
  const [workflow, setWorkflow] = useState("add");
  const [projectPath, setProjectPath] = useState("");
  const [projectTargetPreview, setProjectTargetPreview] = useState([]);
  const [previewingProjectTargets, setPreviewingProjectTargets] = useState(false);
  const [installPreviewToken, setInstallPreviewToken] = useState(null);
  const [installDetailsOpen, setInstallDetailsOpen] = useState(false);
  const [installAdvancedOpen, setInstallAdvancedOpen] = useState(false);
  const [preferredSkillId, setPreferredSkillId] = useState("");
  const [selectedSkillPaths, setSelectedSkillPaths] = useState([]);
  const planExecutorRef = useRef(null);
  if (!planExecutorRef.current) {
    planExecutorRef.current = createSingleFlightPlanExecutor(invokeSkillMateCommand);
  }

  const setPkg = useCallback((value) => {
    setSourceInput((current) => ({ ...current, package: value }));
    setPreferredSkillId("");
    setSelectedSkillPaths([]);
  }, []);

  const preparePackage = useCallback((value, preferredId = "", sourceKind = "git", nextWorkflow = "add") => {
    setSourceInput({ kind: sourceKind, package: value, manual: false });
    setWorkflow(nextWorkflow === "enable" ? "enable" : "add");
    setPreferredSkillId(String(preferredId || "").trim());
    setSelectedSkillPaths([]);
    setInstallDetection(null);
    setInstallStructurePreview(null);
    setInstallPreviewToken(null);
    setInstallDetailsOpen(false);
    setInstallAdvancedOpen(false);
  }, []);

  const startAdd = useCallback(() => {
    preparePackage("", "", "git", "add");
    setInstallMode("copy");
    setProjectPath("");
  }, [preparePackage]);

  const operationMode = workflow === "add" ? "library" : installMode;
  const operationAssistant = workflow === "add" ? "" : installAssistant;

  const setSrc = useCallback((value) => {
    setSourceInput((current) => ({ ...current, kind: value, manual: true }));
    setPreferredSkillId("");
    setSelectedSkillPaths([]);
  }, []);

  useEffect(() => {
    setInstallAssistant((current) => (
      assistants.some((assistant) => assistant.name === current)
        ? current
        : (assistants[0]?.name || "")
    ));
  }, [assistants]);

  useEffect(() => {
    if (!installOpen) {
      setSourceInput((current) => (
        current.manual ? { ...current, manual: false } : current
      ));
      setInstallDetailsOpen(false);
      setInstallAdvancedOpen(false);
      setInstallStructurePreview(null);
      setInstallPreviewToken(null);
      setPreferredSkillId("");
      setSelectedSkillPaths([]);
    }
  }, [installOpen]);

  useEffect(() => {
    setInstallStructurePreview(null);
    setInstallPreviewToken(null);
  }, [operationAssistant, operationMode, pkg, projectPath, src]);

  useEffect(() => {
    if (!installOpen || workflow !== "enable" || installMode !== "symlink" || !projectPath.trim()) {
      setProjectTargetPreview([]);
      return undefined;
    }
    let cancelled = false;
    const timer = setTimeout(async () => {
      setPreviewingProjectTargets(true);
      try {
        const result = await skillmateApi.install.previewProjectTargets(projectPath);
        if (!cancelled) {
          setProjectTargetPreview(result.filter((target) => target.assistant === installAssistant));
        }
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
  }, [installAssistant, installMode, installOpen, projectPath, workflow]);

  useEffect(() => {
    if (!installOpen || !pkg.trim()) {
      setInstallDetection(null);
      return undefined;
    }
    let cancelled = false;
    const timer = setTimeout(async () => {
      try {
        const result = await skillmateApi.install.detectSource(pkg.trim());
        if (cancelled) return;
        setInstallDetection(result);
        if (SUPPORTED_INSTALL_SOURCES.includes(result.normalized_source)) {
          setSourceInput((current) => (
            current.manual || current.kind === result.normalized_source
              ? current
              : { ...current, kind: result.normalized_source }
          ));
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
  }, [installOpen, pkg]);

  useEffect(() => {
    if (
      !installOpen
      || src !== "local"
      || !pkg.trim()
      || (workflow === "enable" && !installAssistant)
      || (operationMode === "symlink" && !projectPath.trim())
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
          assistantName: operationAssistant,
          installMode: operationMode,
          projectPath,
          selectedSkillPaths: [],
          preferredSkillId,
        });
        const result = await skillmateApi.install.preview({
          packageValue: pkg.trim(),
          source: src,
          assistantName: operationAssistant,
          installMode: operationMode,
          projectPath,
          selectedSkillPaths: undefined,
          preferredSkillId,
        });
        if (!cancelled) {
          setInstallStructurePreview(result);
          setInstallPreviewToken({ ...token, planToken: result.plan_token || "" });
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
          setInstallPreviewToken({ ...buildInstallPreviewToken({
            packageValue: pkg,
            source: src,
            assistantName: operationAssistant,
            installMode: operationMode,
            projectPath,
            selectedSkillPaths: [],
            preferredSkillId,
          }), planToken: "" });
        }
      }
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [installOpen, operationAssistant, operationMode, pkg, preferredSkillId, projectPath, src, workflow]);

  const installDetectionView = useMemo(
    () => buildInstallDetectionView(installDetection, t),
    [installDetection, t]
  );
  const installPreviewView = useMemo(
    () => buildInstallPreviewView(installStructurePreview, t),
    [installStructurePreview, t]
  );
  const cmd = useMemo(
    () => buildInstallCommandPreview({ source: src, assistantName: operationAssistant, installMode: operationMode, projectPath, t }),
    [operationAssistant, operationMode, projectPath, src, t]
  );
  const installPreviewCurrent = useMemo(
    () => Boolean(installPreviewToken?.planToken) && isInstallPreviewCurrent({
        previewToken: installPreviewToken,
        packageValue: pkg,
        source: src,
        assistantName: operationAssistant,
        installMode: operationMode,
        projectPath,
        selectedSkillPaths,
        preferredSkillId,
      }),
    [installPreviewToken, operationAssistant, operationMode, pkg, preferredSkillId, projectPath, selectedSkillPaths, src]
  );
  const installPrimaryAction = useMemo(
    () => buildInstallPrimaryAction({
      packageValue: pkg,
      source: src,
      preview: installStructurePreview,
      previewCurrent: installPreviewCurrent,
      previewingInstall,
      loading: false,
      selectionRequired: Boolean(installStructurePreview?.selection_required),
      selectedSkillCount: selectedSkillPaths.length,
      workflow,
      t,
    }),
    [installPreviewCurrent, installStructurePreview, pkg, previewingInstall, selectedSkillPaths.length, src, t, workflow]
  );
  const showProjectLinkOption = useMemo(
    () => workflow === "enable" && shouldShowProjectLinkOption({
      supportsProjectSkills: assistants.find((assistant) => assistant.name === installAssistant)?.supports_project_skills,
    }),
    [assistants, installAssistant, workflow]
  );

  useEffect(() => {
    if (!showProjectLinkOption && installMode === "symlink") {
      setInstallMode("copy");
    }
  }, [installMode, showProjectLinkOption]);
  const showInstallAdvancedOptions = useMemo(
    () => shouldShowInstallAdvancedOptions({ advancedOpen: installAdvancedOpen, detection: installDetection }),
    [installAdvancedOpen, installDetection]
  );

  const previewInstall = useCallback(async () => {
    if (!pkg) { showToast(t("install.toast.enterSource"), "error"); return; }
    if (workflow === "enable" && !installAssistant) { showToast(t("install.toast.chooseAssistant"), "error"); return; }
    if (operationMode === "symlink" && !projectPath.trim()) { showToast(t("install.toast.enterProject"), "error"); return; }
    setPreviewingInstall(true);
    try {
      const token = buildInstallPreviewToken({
        packageValue: pkg,
        source: src,
        assistantName: operationAssistant,
        installMode: operationMode,
        projectPath,
        selectedSkillPaths,
        preferredSkillId,
      });
      const result = await skillmateApi.install.preview({
        packageValue: pkg.trim(),
        source: src,
        assistantName: operationAssistant,
        installMode: operationMode,
        projectPath,
        selectedSkillPaths: selectedSkillPaths.length > 0 ? selectedSkillPaths : undefined,
        preferredSkillId: preferredSkillId || undefined,
      });
      const resolvedSelectedPaths = workflow === "add"
        ? (result.package_detection?.detected_skills || []).map((skill) => skill.relative_path)
        : [];
      setSelectedSkillPaths(resolvedSelectedPaths);
      setInstallStructurePreview(result);
      setInstallPreviewToken({
        ...token,
        selectedSkillPaths: [...new Set(resolvedSelectedPaths)].sort(),
        planToken: result.plan_token || "",
      });
      const previewDoneKey = workflow === "add" ? "install.toast.addPreviewDone" : "install.toast.enablePreviewDone";
      showToast(t(result.can_apply ? previewDoneKey : "install.toast.previewNeedsAttention"), result.can_apply ? "success" : "error");
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
      setInstallPreviewToken({ ...buildInstallPreviewToken({
        packageValue: pkg,
        source: src,
        assistantName: operationAssistant,
        installMode: operationMode,
        projectPath,
        selectedSkillPaths,
        preferredSkillId,
      }), planToken: "" });
      showToast(t("install.toast.previewFailed", { message: String(e) }), "error");
    } finally {
      setPreviewingInstall(false);
    }
  }, [operationAssistant, operationMode, pkg, preferredSkillId, projectPath, selectedSkillPaths, showToast, src, t, workflow]);

  const install = useCallback(async () => {
    if (!pkg) { showToast(t("install.toast.enterPackage"), "error"); return; }
    if (workflow === "enable" && !installAssistant) { showToast(t("install.toast.chooseAssistant"), "error"); return; }
    if (operationMode === "symlink" && !projectPath.trim()) { showToast(t("install.toast.enterProject"), "error"); return; }
    if (!installStructurePreview) {
      showToast(t("install.toast.previewFirst"), "warn");
      return;
    }
    if (!installPreviewCurrent) {
      showToast(t("install.toast.expired"), "warn");
      return;
    }
    if (!(installStructurePreview.can_apply ?? installStructurePreview.can_install)) {
      showToast(t("install.toast.cannotInstall", { message: language === "en" ? t("install.toast.conflict") : (installStructurePreview.message || t("install.toast.conflict")) }), "error");
      return;
    }
    if (!installPreviewToken?.planToken) {
      showToast(t("install.toast.missingPlan"), "warn");
      return;
    }
    const execution = planExecutorRef.current.run(
      "install",
      skillmateCommands.installSkill,
      {
        package: pkg.trim(),
        source: src,
        assistantName: operationAssistant,
        installMode: operationMode,
        projectPath,
        selectedSkillPaths: selectedSkillPaths.length > 0 ? selectedSkillPaths : undefined,
        preferredSkillId,
      },
      installPreviewToken.planToken,
    );
    if (!execution.started) return;
    setLoading(true);
    try {
      const r = await execution.promise;
      if (r.success) {
        const structureSummary = buildInstallStructureSummary(r);
        const successKey = workflow === "add" ? "install.toast.added" : "install.toast.enabled";
        const successWithKey = workflow === "add" ? "install.toast.addedWith" : "install.toast.enabledWith";
        showToast(structureSummary && language !== "en" ? t(successWithKey, { summary: structureSummary }) : t(successKey), "success");
        setInstallOpen(false);
        setSourceInput((current) => ({ ...current, package: "", manual: false }));
        setInstallDetection(null);
        setInstallStructurePreview(null);
        setInstallPreviewToken(null);
        setInstallDetailsOpen(false);
        setInstallAdvancedOpen(false);
        setInstallMode("copy");
        setProjectPath("");
        setPreferredSkillId("");
        setSelectedSkillPaths([]);
        await loadData();
      } else {
        showToast(t("install.toast.failed", { message: String(r.message) }), "error");
      }
    } catch (e) {
      showToast(t("install.toast.failed", { message: String(e) }), "error");
    } finally {
      setLoading(false);
    }
  }, [installAssistant, installPreviewCurrent, installPreviewToken, installStructurePreview, language, loadData, operationAssistant, operationMode, pkg, preferredSkillId, projectPath, selectedSkillPaths, setInstallOpen, setLoading, showToast, src, t, workflow]);

  const toggleSelectedSkillPath = useCallback((path) => {
    setSelectedSkillPaths((current) => (
      current.includes(path)
        ? current.filter((value) => value !== path)
        : [...current, path].sort()
    ));
  }, []);

  const runInstallPrimaryAction = useCallback(() => {
    if (installPrimaryAction.action === "install") {
      install();
    } else {
      previewInstall();
    }
  }, [install, installPrimaryAction.action, previewInstall]);

  return {
    workflow,
    startAdd,
    source: {
      kind: src,
      setKind: setSrc,
      package: pkg,
      setPackage: setPkg,
      prepare: preparePackage,
      detectionView: installDetectionView,
    },
    target: {
      assistant: installAssistant,
      setAssistant: setInstallAssistant,
      mode: installMode,
      setMode: setInstallMode,
      projectPath,
      setProjectPath,
      projectPreview: projectTargetPreview,
      previewingProject: previewingProjectTargets,
      showProjectLinkOption,
    },
    preview: {
      structure: installStructurePreview,
      view: installPreviewView,
      previewing: previewingInstall,
      current: installPreviewCurrent,
      primaryAction: installPrimaryAction,
      runPrimaryAction: runInstallPrimaryAction,
    },
    selection: {
      availableSkills: installPreviewView?.availableSkills || [],
      selectedPaths: selectedSkillPaths,
      required: Boolean(installStructurePreview?.selection_required),
      toggle: toggleSelectedSkillPath,
    },
    disclosure: {
      detailsOpen: installDetailsOpen,
      setDetailsOpen: setInstallDetailsOpen,
      advancedOpen: installAdvancedOpen,
      setAdvancedOpen: setInstallAdvancedOpen,
      showAdvancedOptions: showInstallAdvancedOptions,
    },
    commandPreview: cmd,
  };
}
