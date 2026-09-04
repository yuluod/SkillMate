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
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { invokeSkillMateCommand, skillmateApi, skillmateCommands } from "./skillmateApi.js";
import { useI18n } from "./i18n.jsx";

function mergeInstallPreviews(previews) {
  const [first] = previews;
  if (!first) return null;
  const unique = (items, keyOf) => [...new Map(items.map((item) => [keyOf(item), item])).values()];
  return {
    ...first,
    can_install: previews.every((preview) => preview.can_install),
    can_apply: previews.every((preview) => preview.can_apply),
    target_actions: unique(
      previews.flatMap((preview) => preview.target_actions || []),
      (action) => `${action.action}\0${action.source}\0${action.target}`,
    ),
    conflicts: unique(
      previews.flatMap((preview) => preview.conflicts || []),
      (conflict) => `${conflict.target}\0${conflict.reason}`,
    ),
  };
}

export function useInstallFlow({ installOpen, assistants, setInstallOpen, showToast, loadData, setLoading }) {
  const { t, language } = useI18n();
  const [sourceInput, setSourceInput] = useState({ kind: "git", package: "", manual: false });
  const { kind: src, package: pkg } = sourceInput;
  const [installDetection, setInstallDetection] = useState(null);
  const [installStructurePreview, setInstallStructurePreview] = useState(null);
  const [previewingInstall, setPreviewingInstall] = useState(false);
  const [installAssistants, setInstallAssistants] = useState([]);
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

  const installAssistant = installAssistants[0] || "";
  const operationMode = workflow === "add" ? "library" : installMode;
  const operationAssistant = workflow === "add" ? "" : [...installAssistants].sort().join("、");

  const setSrc = useCallback((value) => {
    setSourceInput((current) => ({ ...current, kind: value, manual: true }));
    setPreferredSkillId("");
    setSelectedSkillPaths([]);
  }, []);

  const setInstallAssistant = useCallback((value) => {
    setInstallAssistants(value ? [value] : []);
  }, []);

  const toggleInstallAssistant = useCallback((name) => {
    setInstallAssistants((current) => current.includes(name)
      ? current.filter((value) => value !== name)
      : [...current, name]);
  }, []);

  useEffect(() => {
    setInstallAssistants((current) => {
      const available = new Set(assistants.map((assistant) => assistant.name));
      const valid = current.filter((name) => available.has(name));
      const next = valid.length > 0 ? valid : (assistants[0]?.name ? [assistants[0].name] : []);
      return next.length === current.length && next.every((name, index) => name === current[index])
        ? current
        : next;
    });
  }, [assistants]);

  useEffect(() => {
    if (workflow !== "enable" || installMode !== "symlink") return;
    const supported = new Set(assistants.filter((assistant) => assistant.supports_project_skills).map((assistant) => assistant.name));
    setInstallAssistants((current) => {
      const valid = current.filter((name) => supported.has(name));
      if (valid.length > 0) {
        return valid.length === current.length ? current : valid;
      }
      const fallback = assistants.find((assistant) => assistant.supports_project_skills)?.name;
      const next = fallback ? [fallback] : [];
      return next.length === current.length && next.every((name, index) => name === current[index])
        ? current
        : next;
    });
  }, [assistants, installMode, workflow]);

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
          setProjectTargetPreview(result.filter((target) => installAssistants.includes(target.assistant)));
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
  }, [installAssistants, installMode, installOpen, projectPath, workflow]);

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
      || workflow !== "add"
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
  const installPrimaryAction = useMemo(() => {
    const action = buildInstallPrimaryAction({
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
    });
    return workflow === "enable" && installAssistants.length === 0
      ? { ...action, disabled: true }
      : action;
  }, [installAssistants.length, installPreviewCurrent, installStructurePreview, pkg, previewingInstall, selectedSkillPaths.length, src, t, workflow]);
  const showProjectLinkOption = useMemo(
    () => workflow === "enable" && shouldShowProjectLinkOption({
      supportsProjectSkills: assistants.some((assistant) => assistant.supports_project_skills),
    }),
    [assistants, workflow]
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

  const pickProjectDirectory = useCallback(async () => {
    try {
      const selected = await openDialog({
        directory: true,
        multiple: false,
        title: t("install.projectPickerTitle"),
      });
      if (typeof selected === "string") setProjectPath(selected);
    } catch (error) {
      showToast(t("install.toast.projectPickerFailed", { message: String(error) }), "error");
    }
  }, [showToast, t]);

  const previewInstall = useCallback(async () => {
    if (!pkg) { showToast(t("install.toast.enterSource"), "error"); return; }
    if (workflow === "enable" && installAssistants.length === 0) { showToast(t("install.toast.chooseAssistant"), "error"); return; }
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
      const targetAssistants = workflow === "enable" ? installAssistants : [""];
      const plans = await Promise.all(targetAssistants.map(async (assistantName) => {
        const preview = await skillmateApi.install.preview({
          packageValue: pkg.trim(),
          source: src,
          assistantName,
          installMode: operationMode,
          projectPath,
          selectedSkillPaths: selectedSkillPaths.length > 0 ? selectedSkillPaths : undefined,
          preferredSkillId: preferredSkillId || undefined,
        });
        return { assistantName, preview, planToken: preview.plan_token || "" };
      }));
      const result = mergeInstallPreviews(plans.map((plan) => plan.preview));
      const resolvedSelectedPaths = workflow === "add"
        ? (result.package_detection?.detected_skills || []).map((skill) => skill.relative_path)
        : [];
      setSelectedSkillPaths(resolvedSelectedPaths);
      setInstallStructurePreview(result);
      setInstallPreviewToken({
        ...token,
        selectedSkillPaths: [...new Set(resolvedSelectedPaths)].sort(),
        planToken: plans[0]?.planToken || "",
        plans,
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
  }, [installAssistants, operationAssistant, operationMode, pkg, preferredSkillId, projectPath, selectedSkillPaths, showToast, src, t, workflow]);

  const install = useCallback(async () => {
    if (!pkg) { showToast(t("install.toast.enterPackage"), "error"); return; }
    if (workflow === "enable" && installAssistants.length === 0) { showToast(t("install.toast.chooseAssistant"), "error"); return; }
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
    setLoading(true);
    try {
      const plans = installPreviewToken.plans || [{ assistantName: operationAssistant, planToken: installPreviewToken.planToken }];
      const results = [];
      for (const plan of plans) {
        const execution = planExecutorRef.current.run(
          `install:${plan.assistantName || "library"}`,
          skillmateCommands.installSkill,
          {
            package: pkg.trim(),
            source: src,
            assistantName: plan.assistantName,
            installMode: operationMode,
            projectPath,
            selectedSkillPaths: selectedSkillPaths.length > 0 ? selectedSkillPaths : undefined,
            preferredSkillId,
          },
          plan.planToken,
        );
        if (!execution.started) return;
        results.push({ assistantName: plan.assistantName, result: await execution.promise });
      }
      const failed = results.filter(({ result }) => !result.success);
      if (failed.length === 0) {
        const structureSummary = buildInstallStructureSummary(results[0]?.result);
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
        setInstallStructurePreview(null);
        setInstallPreviewToken(null);
        await loadData();
        showToast(t("install.toast.enablePartial", {
          success: results.length - failed.length,
          failed: failed.length,
          message: failed.map(({ assistantName, result }) => `${assistantName}: ${result.message}`).join(t("common.messageSeparator")),
        }), "error");
      }
    } catch (e) {
      showToast(t("install.toast.failed", { message: String(e) }), "error");
    } finally {
      setLoading(false);
    }
  }, [installAssistants.length, installPreviewCurrent, installPreviewToken, installStructurePreview, language, loadData, operationAssistant, operationMode, pkg, preferredSkillId, projectPath, selectedSkillPaths, setInstallOpen, setLoading, showToast, src, t, workflow]);

  const toggleSelectedSkillPath = useCallback((path) => {
    setSelectedSkillPaths((current) => (
      current.includes(path)
        ? current.filter((value) => value !== path)
        : [...current, path].sort()
    ));
  }, []);

  const runInstallPrimaryAction = useCallback(() => {
    if (installPrimaryAction.action === "install") {
      return install();
    }
    return previewInstall();
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
      assistants: installAssistants,
      toggleAssistant: toggleInstallAssistant,
      mode: installMode,
      setMode: setInstallMode,
      projectPath,
      setProjectPath,
      pickProjectDirectory,
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
