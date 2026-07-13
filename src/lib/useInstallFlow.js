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
  const planExecutorRef = useRef(null);
  if (!planExecutorRef.current) {
    planExecutorRef.current = createSingleFlightPlanExecutor(invokeSkillMateCommand);
  }

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
      const result = await skillmateApi.install.preview({
        packageValue: pkg.trim(),
        source: src,
        assistantName: installAssistant,
        installMode,
        projectPath,
      });
      setInstallStructurePreview(result);
      setInstallPreviewToken({ ...token, planToken: result.plan_token || "" });
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
      setInstallPreviewToken({ ...buildInstallPreviewToken({
        packageValue: pkg,
        source: src,
        assistantName: installAssistant,
        installMode,
        projectPath,
      }), planToken: "" });
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
    if (!installPreviewToken?.planToken) {
      showToast("安装计划缺失，请重新检查结构", "warn");
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
  }, [installAssistant, installMode, installPreviewCurrent, installPreviewToken, installStructurePreview, loadData, pkg, projectPath, setInstallOpen, setLoading, showToast, src]);

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
