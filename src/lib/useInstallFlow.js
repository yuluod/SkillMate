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
  const [projectPath, setProjectPath] = useState("");
  const [projectTargetPreview, setProjectTargetPreview] = useState([]);
  const [previewingProjectTargets, setPreviewingProjectTargets] = useState(false);
  const [installPreviewToken, setInstallPreviewToken] = useState(null);
  const [installDetailsOpen, setInstallDetailsOpen] = useState(false);
  const [installAdvancedOpen, setInstallAdvancedOpen] = useState(false);
  const planExecutorRef = useRef(null);
  if (!planExecutorRef.current) {
    planExecutorRef.current = createSingleFlightPlanExecutor(invokeSkillMateCommand);
  }

  const setPkg = useCallback((value) => {
    setSourceInput((current) => ({ ...current, package: value }));
  }, []);

  const preparePackage = useCallback((value) => {
    setSourceInput({ kind: "git", package: value, manual: false });
    setInstallDetection(null);
    setInstallStructurePreview(null);
    setInstallPreviewToken(null);
    setInstallDetailsOpen(false);
    setInstallAdvancedOpen(false);
  }, []);

  const setSrc = useCallback((value) => {
    setSourceInput((current) => ({ ...current, kind: value, manual: true }));
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
        const result = await skillmateApi.install.previewProjectTargets(projectPath);
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
        const result = await skillmateApi.install.preview({
          packageValue: pkg.trim(),
          source: src,
          assistantName: installAssistant,
          installMode,
          projectPath,
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
            assistantName: installAssistant,
            installMode,
            projectPath,
          }), planToken: "" });
        }
      }
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [installAssistant, installMode, installOpen, pkg, projectPath, src]);

  const installDetectionView = useMemo(
    () => buildInstallDetectionView(installDetection, t),
    [installDetection, t]
  );
  const installPreviewView = useMemo(
    () => buildInstallPreviewView(installStructurePreview, t),
    [installStructurePreview, t]
  );
  const cmd = useMemo(
    () => buildInstallCommandPreview({ source: src, assistantName: installAssistant, installMode, projectPath, t }),
    [installAssistant, installMode, projectPath, src, t]
  );
  const installPreviewCurrent = useMemo(
    () => Boolean(installPreviewToken?.planToken) && isInstallPreviewCurrent({
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
      t,
    }),
    [installPreviewCurrent, installStructurePreview, pkg, previewingInstall, t]
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
    if (!pkg) { showToast(t("install.toast.enterSource"), "error"); return; }
    if (!installAssistant) { showToast(t("install.toast.chooseAssistant"), "error"); return; }
    if (installMode === "symlink" && !projectPath.trim()) { showToast(t("install.toast.enterProject"), "error"); return; }
    setPreviewingInstall(true);
    try {
      const token = buildInstallPreviewToken({
        packageValue: pkg,
        source: src,
        assistantName: installAssistant,
        installMode,
        projectPath,
      });
      const result = await skillmateApi.install.preview({
        packageValue: pkg.trim(),
        source: src,
        assistantName: installAssistant,
        installMode,
        projectPath,
      });
      setInstallStructurePreview(result);
      setInstallPreviewToken({ ...token, planToken: result.plan_token || "" });
      showToast(t(result.can_apply ? "install.toast.previewDone" : "install.toast.previewNeedsAttention"), result.can_apply ? "success" : "error");
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
        assistantName: installAssistant,
        installMode,
        projectPath,
      }), planToken: "" });
      showToast(t("install.toast.previewFailed", { message: String(e) }), "error");
    } finally {
      setPreviewingInstall(false);
    }
  }, [installAssistant, installMode, pkg, projectPath, showToast, src, t]);

  const install = useCallback(async () => {
    if (!pkg) { showToast(t("install.toast.enterPackage"), "error"); return; }
    if (!installAssistant) { showToast(t("install.toast.chooseAssistant"), "error"); return; }
    if (installMode === "symlink" && !projectPath.trim()) { showToast(t("install.toast.enterProject"), "error"); return; }
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
        assistantName: installAssistant,
        installMode,
        projectPath,
      },
      installPreviewToken.planToken,
    );
    if (!execution.started) return;
    setLoading(true);
    try {
      const r = await execution.promise;
      if (r.success) {
        const structureSummary = buildInstallStructureSummary(r);
        showToast(structureSummary && language !== "en" ? t("install.toast.successWith", { summary: structureSummary }) : t("install.toast.success"), "success");
        setInstallOpen(false);
        setSourceInput((current) => ({ ...current, package: "", manual: false }));
        setInstallDetection(null);
        setInstallStructurePreview(null);
        setInstallPreviewToken(null);
        setInstallDetailsOpen(false);
        setInstallAdvancedOpen(false);
        setInstallMode("copy");
        setProjectPath("");
        await loadData();
      } else {
        showToast(t("install.toast.failed", { message: String(r.message) }), "error");
      }
    } catch (e) {
      showToast(t("install.toast.failed", { message: String(e) }), "error");
    } finally {
      setLoading(false);
    }
  }, [installAssistant, installMode, installPreviewCurrent, installPreviewToken, installStructurePreview, language, loadData, pkg, projectPath, setInstallOpen, setLoading, showToast, src, t]);

  const runInstallPrimaryAction = useCallback(() => {
    if (installPrimaryAction.action === "install") {
      install();
    } else {
      previewInstall();
    }
  }, [install, installPrimaryAction.action, previewInstall]);

  return {
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
