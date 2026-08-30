import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "./i18n.jsx";
import { toUserErrorMessage } from "./errorMessage.mjs";

const AUTO_CHECK_STORAGE_KEY = "skillmate-auto-check-updates";

function initialAutoCheckEnabled() {
  if (typeof window === "undefined") return true;
  return window.localStorage.getItem(AUTO_CHECK_STORAGE_KEY) !== "false";
}

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
  const checkOperationRef = useRef(null);
  const [autoCheckEnabled, setAutoCheckEnabledState] = useState(initialAutoCheckEnabled);
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
          setAppUpdateState((current) => ({ ...current, error: toUserErrorMessage(e, t("error.safeRetry")) }));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  const runAppUpdateCheck = useCallback((interactive) => {
    const currentOperation = checkOperationRef.current;
    if (currentOperation) {
      if (interactive && !currentOperation.interactive) {
        currentOperation.interactive = true;
        setAppUpdateState((current) => ({
          ...current,
          status: "checking",
          progress: null,
          error: "",
        }));
      }
      return currentOperation.promise;
    }

    const operation = { interactive, promise: null };
    operationRef.current = interactive ? "check" : "startup-check";
    if (interactive) {
      setAppUpdateState((current) => ({
        ...current,
        status: "checking",
        progress: null,
        error: "",
      }));
    }

    operation.promise = (async () => {
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
          if (operation.interactive) {
            showToast(t("appUpdate.toast.current"), "success");
          }
          return null;
        }
        setAppUpdateState({
          status: "available",
          currentVersion: currentVersion || update.currentVersion || "",
          update: {
            currentVersion: update.currentVersion,
            version: update.version,
            date: update.date || "",
            body: update.body || "",
          },
          progress: null,
          error: "",
          lastCheckedAt: Date.now(),
        });
        showToast(
          t(operation.interactive ? "appUpdate.toast.available" : "appUpdate.toast.startupAvailable", { version: update.version }),
          "success",
        );
        return update;
      } catch (e) {
        if (operation.interactive) {
          const message = toUserErrorMessage(e, t("error.safeRetry"));
          updateRef.current = null;
          setAppUpdateState((current) => ({
            ...current,
            status: "error",
            progress: null,
            error: message,
            lastCheckedAt: Date.now(),
          }));
          showToast(t("appUpdate.toast.checkFailed", { message }), "error");
        }
        return null;
      } finally {
        if (checkOperationRef.current === operation) {
          checkOperationRef.current = null;
          operationRef.current = null;
        }
      }
    })();
    checkOperationRef.current = operation;
    return operation.promise;
  }, [showToast, t]);

  const checkAppUpdate = useCallback(async () => {
    if (operationRef.current && !checkOperationRef.current) {
      return null;
    }
    return runAppUpdateCheck(true);
  }, [runAppUpdateCheck]);

  const setAutoCheckEnabled = useCallback((enabled) => {
    const next = Boolean(enabled);
    setAutoCheckEnabledState(next);
    window.localStorage.setItem(AUTO_CHECK_STORAGE_KEY, String(next));
  }, []);

  // 启动自动检查:静默模式,失败不打扰,发现新版本时提示一次。
  // 不复用 checkAppUpdate,避免把"已是最新"的成功 toast 和错误 toast 打到启动流程里。
  const runStartupUpdateCheck = useCallback(async () => {
    if (autoCheckRef.current || operationRef.current) {
      return;
    }
    autoCheckRef.current = true;
    await runAppUpdateCheck(false);
  }, [runAppUpdateCheck]);

  useEffect(() => {
    if (!autoCheckEnabled) {
      return undefined;
    }
    const timer = setTimeout(() => {
      runStartupUpdateCheck();
    }, 3000);
    return () => clearTimeout(timer);
  }, [autoCheckEnabled, runStartupUpdateCheck]);

  const installAppUpdate = useCallback(async () => {
    if (operationRef.current && !checkOperationRef.current) {
      return;
    }
    let update;
    if (checkOperationRef.current || !updateRef.current) {
      update = await checkAppUpdate();
    } else {
      update = updateRef.current;
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
      const message = toUserErrorMessage(e, t("error.safeRetry"));
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
      const message = toUserErrorMessage(e, t("error.safeRetry"));
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
        error: t("appUpdate.toast.restartFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }),
      }));
      showToast(t("appUpdate.toast.restartFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    } finally {
      if (operationRef.current === "restart") {
        operationRef.current = null;
      }
    }
  }, [showToast, t]);

  return {
    appUpdateState,
    autoCheckEnabled,
    setAutoCheckEnabled,
    checkAppUpdate,
    installAppUpdate,
    restartApp,
  };
}
