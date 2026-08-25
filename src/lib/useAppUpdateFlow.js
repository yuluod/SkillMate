import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "./i18n.jsx";

async function loadAppUpdateApis() {
  const [{ getVersion }, { check }, { relaunch }] = await Promise.all([
    import("@tauri-apps/api/app"),
    import("@tauri-apps/plugin-updater"),
    import("@tauri-apps/plugin-process"),
  ]);
  return { getVersion, check, relaunch };
}

export function useAppUpdateFlow({ showToast }) {
  const { t } = useI18n();
  const updateRef = useRef(null);
  const autoCheckRef = useRef(false);
  const operationRef = useRef(null);
  const [appUpdateState, setAppUpdateState] = useState({
    status: "idle",
    currentVersion: "",
    update: null,
    progress: null,
    error: "",
    lastCheckedAt: null,
  });

  useEffect(() => {
    let cancelled = false;
    loadAppUpdateApis()
      .then(({ getVersion }) => getVersion())
      .then((version) => {
        if (!cancelled) {
          setAppUpdateState((current) => ({ ...current, currentVersion: version || "" }));
        }
      })
      .catch((e) => {
        if (!cancelled) {
          setAppUpdateState((current) => ({ ...current, error: String(e) }));
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const checkAppUpdate = useCallback(async () => {
    if (operationRef.current) {
      return null;
    }
    operationRef.current = "check";
    setAppUpdateState((current) => ({
      ...current,
      status: "checking",
      progress: null,
      error: "",
    }));
    try {
      const { getVersion, check } = await loadAppUpdateApis();
      const currentVersion = await getVersion();
      const update = await check();
      updateRef.current = update;
      if (!update) {
        setAppUpdateState({
          status: "current",
          currentVersion,
          update: null,
          progress: null,
          error: "",
          lastCheckedAt: Date.now(),
        });
        showToast(t("appUpdate.toast.current"), "success");
        return null;
      }
      const plainUpdate = {
        currentVersion: update.currentVersion,
        version: update.version,
        date: update.date || "",
        body: update.body || "",
      };
      setAppUpdateState({
        status: "available",
        currentVersion: currentVersion || update.currentVersion || "",
        update: plainUpdate,
        progress: null,
        error: "",
        lastCheckedAt: Date.now(),
      });
      showToast(t("appUpdate.toast.available", { version: update.version }), "success");
      return update;
    } catch (e) {
      const message = String(e);
      updateRef.current = null;
      setAppUpdateState((current) => ({
        ...current,
        status: "error",
        progress: null,
        error: message,
        lastCheckedAt: Date.now(),
      }));
      showToast(t("appUpdate.toast.checkFailed", { message }), "error");
      return null;
    } finally {
      if (operationRef.current === "check") {
        operationRef.current = null;
      }
    }
  }, [showToast, t]);

  // 启动自动检查:静默模式,失败不打扰,发现新版本时提示一次。
  // 不复用 checkAppUpdate,避免把"已是最新"的成功 toast 和错误 toast 打到启动流程里。
  const runStartupUpdateCheck = useCallback(async () => {
    if (autoCheckRef.current || operationRef.current) {
      return;
    }
    autoCheckRef.current = true;
    operationRef.current = "startup-check";
    try {
      const { check } = await loadAppUpdateApis();
      const update = await check();
      updateRef.current = update;
      if (!update) {
        setAppUpdateState((current) => ({
          ...current,
          status: "current",
          update: null,
          lastCheckedAt: Date.now(),
        }));
        return;
      }
      setAppUpdateState((current) => ({
        ...current,
        status: "available",
        update: {
          currentVersion: update.currentVersion,
          version: update.version,
          date: update.date || "",
          body: update.body || "",
        },
        lastCheckedAt: Date.now(),
      }));
      showToast(t("appUpdate.toast.startupAvailable", { version: update.version }), "success");
    } catch {
      // 启动静默检查失败不打扰用户;设置页手动检查会展示完整错误。
    } finally {
      if (operationRef.current === "startup-check") {
        operationRef.current = null;
      }
    }
  }, [showToast, t]);

  useEffect(() => {
    const timer = setTimeout(() => {
      runStartupUpdateCheck();
    }, 3000);
    return () => clearTimeout(timer);
  }, [runStartupUpdateCheck]);

  const installAppUpdate = useCallback(async () => {
    if (operationRef.current) {
      return;
    }
    let update = updateRef.current;
    if (!update) {
      update = await checkAppUpdate();
    }
    if (!update) {
      return;
    }
    if (operationRef.current) {
      return;
    }
    operationRef.current = "install";
    setAppUpdateState((current) => ({
      ...current,
      status: "downloading",
      progress: { downloaded: 0, contentLength: 0 },
      error: "",
    }));
    let downloaded = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          downloaded = 0;
          setAppUpdateState((current) => ({
            ...current,
            status: "downloading",
            progress: {
              downloaded,
              contentLength: event.data.contentLength || 0,
            },
          }));
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength || 0;
          setAppUpdateState((current) => ({
            ...current,
            status: "downloading",
            progress: {
              downloaded,
              contentLength: current.progress?.contentLength || 0,
            },
          }));
        } else if (event.event === "Finished") {
          setAppUpdateState((current) => ({
            ...current,
            status: "installing",
          }));
        }
      });
      setAppUpdateState((current) => ({
        ...current,
        status: "restarting",
        progress: null,
        error: "",
      }));
      showToast(t("appUpdate.toast.restarting"), "success");
    } catch (e) {
      const message = String(e);
      setAppUpdateState((current) => ({
        ...current,
        status: "error",
        progress: null,
        error: message,
      }));
      showToast(t("appUpdate.toast.installFailed", { message }), "error");
      if (operationRef.current === "install") {
        operationRef.current = null;
      }
      return;
    }
    try {
      const { relaunch } = await loadAppUpdateApis();
      await relaunch();
    } catch (e) {
      const message = String(e);
      setAppUpdateState((current) => ({
        ...current,
        status: "ready_to_restart",
        progress: null,
        error: t("appUpdate.toast.autoRestartFailed", { message }),
      }));
      showToast(t("appUpdate.toast.restartManually", { message }), "error");
    } finally {
      if (operationRef.current === "install") {
        operationRef.current = null;
      }
    }
  }, [checkAppUpdate, showToast, t]);

  const restartApp = useCallback(async () => {
    if (operationRef.current) {
      return;
    }
    operationRef.current = "restart";
    try {
      setAppUpdateState((current) => ({
        ...current,
        status: "restarting",
        progress: null,
        error: "",
      }));
      const { relaunch } = await loadAppUpdateApis();
      await relaunch();
    } catch (e) {
      setAppUpdateState((current) => ({
        ...current,
        status: "ready_to_restart",
        progress: null,
        error: t("appUpdate.toast.restartFailed", { message: String(e) }),
      }));
      showToast(t("appUpdate.toast.restartFailed", { message: String(e) }), "error");
    } finally {
      if (operationRef.current === "restart") {
        operationRef.current = null;
      }
    }
  }, [showToast, t]);

  return {
    appUpdateState,
    checkAppUpdate,
    installAppUpdate,
    restartApp,
  };
}
