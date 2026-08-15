import { useCallback, useEffect, useRef, useState } from "react";

async function loadAppUpdateApis() {
  const [{ getVersion }, { check }, { relaunch }] = await Promise.all([
    import("@tauri-apps/api/app"),
    import("@tauri-apps/plugin-updater"),
    import("@tauri-apps/plugin-process"),
  ]);
  return { getVersion, check, relaunch };
}

export function useAppUpdateFlow({ showToast }) {
  const updateRef = useRef(null);
  const autoCheckRef = useRef(false);
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
        showToast("当前已是最新版本", "success");
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
      showToast(`发现新版本 ${update.version}`, "success");
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
      showToast(`检查应用更新失败: ${message}`, "error");
      return null;
    }
  }, [showToast]);

  // 启动自动检查:静默模式,失败不打扰,发现新版本时提示一次。
  // 不复用 checkAppUpdate,避免把"已是最新"的成功 toast 和错误 toast 打到启动流程里。
  const runStartupUpdateCheck = useCallback(async () => {
    if (autoCheckRef.current) {
      return;
    }
    autoCheckRef.current = true;
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
      showToast(`发现新版本 ${update.version},可在设置中更新`, "success");
    } catch {
      // 启动静默检查失败不打扰用户;设置页手动检查会展示完整错误。
    }
  }, [showToast]);

  useEffect(() => {
    const timer = setTimeout(() => {
      runStartupUpdateCheck();
    }, 3000);
    return () => clearTimeout(timer);
  }, [runStartupUpdateCheck]);

  const installAppUpdate = useCallback(async () => {
    let update = updateRef.current;
    if (!update) {
      update = await checkAppUpdate();
    }
    if (!update) {
      return;
    }
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
      showToast("更新已安装，正在重启应用", "success");
    } catch (e) {
      const message = String(e);
      setAppUpdateState((current) => ({
        ...current,
        status: "error",
        progress: null,
        error: message,
      }));
      showToast(`安装应用更新失败: ${message}`, "error");
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
        error: `自动重启失败: ${message}`,
      }));
      showToast(`自动重启失败，请手动重启: ${message}`, "error");
    }
  }, [checkAppUpdate, showToast]);

  const restartApp = useCallback(async () => {
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
        error: `重启失败: ${e}`,
      }));
      showToast(`重启失败: ${e}`, "error");
    }
  }, [showToast]);

  return {
    appUpdateState,
    checkAppUpdate,
    installAppUpdate,
    restartApp,
  };
}
