import { useCallback, useState } from "react";
import { skillmateApi } from "./skillmateApi.js";

export function useUpdateFlow({ updatable, showToast, loadData }) {
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
                message: "检查失败: 后端未返回结果",
                syncState: "failed",
              };
        });
        return next;
      });
      const failed = results.filter((result) => result.syncState === "failed").length;
      showToast(
        failed > 0 ? `检查完成，${failed} 个 Skill 失败` : "全部检查完成",
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
            message: `检查失败: ${error}`,
            syncState: "failed",
          };
        });
        return next;
      });
      showToast(`批量检查失败: ${error}`, "error");
    }
  }, [showToast, updatable, updateState]);

  const checkUpdate = useCallback(async (path) => {
    try {
      setUpdateState(prev => ({ ...prev, [path]: { ...(prev[path] || {}), checking: true } }));
      const r = await skillmateApi.updates.checkOne(path);
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
      const result = await skillmateApi.updates.applyOne(path);
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
