import { useCallback, useState } from "react";
import { skillmateApi } from "./skillmateApi.js";
import { useI18n } from "./i18n.jsx";
import { toUserErrorMessage } from "./errorMessage.mjs";

export function useUpdateFlow({ updatable, showToast, loadData }) {
  const { t, language } = useI18n();
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
      message: state.message || skill.sync_message || t("updates.state.unknown"),
      lagCount: state.lagCount ?? skill.lag_count ?? 0,
      lastProbeAt: state.lastProbeAt ?? skill.last_probe_at,
      lastSyncAt: state.lastSyncAt ?? skill.last_sync_at,
      managedByApp: state.managedByApp ?? skill.managed_by_app,
      canCheck: state.canCheck ?? skill.can_check ?? false,
      canSync: state.canSync ?? skill.can_sync ?? false,
      checking: Boolean(state.checking),
      updating: Boolean(state.updating)
    };
  }, [t, updateState]);

  const checkAllUpdates = useCallback(async () => {
    if (updatable.length === 0) return;
    const initial = {};
    updatable.forEach(s => { initial[s.path] = { ...(updateState[s.path] || {}), checking: true }; });
    setUpdateState(prev => ({ ...prev, ...initial }));
    try {
      const results = await skillmateApi.updates.checkAll(updatable.map((skill) => skill.path));
      const byPath = new Map(results.map((result) => [result.path, result]));
      setUpdateState((previous) => {
        const next = { ...previous };
        updatable.forEach((skill) => {
          const result = byPath.get(skill.path);
          next[skill.path] = result
            ? { checking: false, updating: false, ...result }
            : {
                ...(previous[skill.path] || {}),
                checking: false,
                updating: false,
                hasUpdate: false,
                lagCount: 0,
                message: t("updates.message.noResult"),
                syncState: "failed",
              };
        });
        return next;
      });
      const failed = results.filter((result) => result.syncState === "failed").length;
      showToast(
        failed > 0 ? t("updates.toast.batchPartial", { count: failed }) : t("updates.toast.batchDone"),
        failed > 0 ? "warn" : "success"
      );
    } catch (error) {
      setUpdateState((previous) => {
        const next = { ...previous };
        updatable.forEach((skill) => {
          next[skill.path] = {
            ...(previous[skill.path] || {}),
            checking: false,
            updating: false,
            hasUpdate: false,
            lagCount: 0,
            message: t("updates.toast.checkFailed", { message: toUserErrorMessage(error, t("error.safeRetry")) }),
            syncState: "failed",
          };
        });
        return next;
      });
      showToast(t("updates.toast.batchFailed", { message: toUserErrorMessage(error, t("error.safeRetry")) }), "error");
    }
  }, [showToast, t, updatable, updateState]);

  const checkUpdate = useCallback(async (path) => {
    try {
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), checking: true } }));
      const r = await skillmateApi.updates.checkOne(path);
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), checking: false, updating: false, ...r } }));
      const hasUpdate = typeof r.hasUpdate === "boolean" ? r.hasUpdate : r.syncState === "behind";
      const fallback = t(hasUpdate ? "updates.toast.available" : "updates.toast.current");
      showToast(language === "en" ? fallback : (r.message || fallback), hasUpdate ? "warn" : "success");
    } catch (e) {
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), checking: false } }));
      showToast(t("updates.toast.checkFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    }
  }, [language, showToast, t]);

  const updateSkill = useCallback(async (path) => {
    try {
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), updating: true } }));
      const result = await skillmateApi.updates.applyOne(path);
      showToast(language === "en" ? t("updates.toast.updated") : String(result || t("updates.toast.updated")), "success");
      await checkUpdate(path);
      await loadData();
    } catch (e) {
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), updating: false } }));
      showToast(t("updates.toast.updateFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    }
  }, [checkUpdate, language, loadData, showToast, t]);

  return {
    updateState,
    resetUpdateState,
    getSyncInfo,
    checkAllUpdates,
    checkUpdate,
    updateSkill,
  };
}
