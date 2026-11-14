import { useCallback, useEffect, useRef, useState } from "react";
import { skillmateApi } from "./skillmateApi.js";
import { useI18n } from "./i18n.jsx";
import { toUserErrorMessage } from "./errorMessage.mjs";
import { adaptUpdateResult, getUpdateInfo, isIncompleteUpdateCheck, updateCheckNotice } from "./updateState.js";

export function useUpdateFlow({ updatable, showToast, loadData }) {
  const { t, language } = useI18n();
  const [updateState, setUpdateState] = useState({});
  const generation = useRef(0);
  const active = useRef(new Map());
  const batch = useRef(null);

  useEffect(() => () => { generation.current += 1; }, []);

  const resetUpdateState = useCallback(() => {
    generation.current += 1;
    setUpdateState({});
    // IPC cannot be cancelled: locks stay until outstanding operations settle.
  }, []);

  const getSyncInfo = useCallback((skill) =>
    getUpdateInfo(skill, updateState[skill.path], t("updates.state.unknown")), [t, updateState]);

  // 技能行与详情栏依赖这些回调的引用稳定性，内部链条全部包 useCallback。
  const isCurrent = useCallback(
    (operation) => operation.generation === generation.current, []);
  const change = useCallback((operation, paths, transform) => {
    if (!isCurrent(operation)) return;
    setUpdateState(previous => {
      if (!isCurrent(operation)) return previous;
      const next = { ...previous };
      paths.forEach(path => { next[path] = transform(previous[path] || {}, path); });
      return next;
    });
  }, [isCurrent]);
  const begin = useCallback((paths, updating = false) => {
    const operation = { generation: generation.current };
    paths.forEach(path => active.current.set(path, operation));
    change(operation, paths, state => ({ ...state, checking: !updating, updating }));
    return operation;
  }, [change]);
  const finish = useCallback((operation, paths) => {
    paths.forEach(path => {
      if (active.current.get(path) === operation) active.current.delete(path);
    });
    change(operation, paths, state => ({ ...state, checking: false, updating: false }));
  }, [change]);
  const failedResult = useCallback(error => ({ syncState: "failed",
    message: t("updates.toast.checkFailed", { message: toUserErrorMessage(error, t("error.safeRetry")) }) }), [t]);
  const accept = useCallback((operation, path, result) => {
    change(operation, [path], state => adaptUpdateResult(state, result, t("updates.message.noResult")));
    return adaptUpdateResult({}, result, t("updates.message.noResult"));
  }, [change, t]);
  const notify = useCallback(result => {
    const notice = updateCheckNotice(result);
    showToast(language === "en" ? t(notice.key) : (result.message || t(notice.key)), notice.tone);
  }, [showToast, t, language]);
  // Apply's follow-up check shares its lock instead of calling the public handler.
  const probe = useCallback(async (operation, path) => {
    change(operation, [path], state => ({ ...state, checking: true }));
    let result;
    try { result = await skillmateApi.updates.checkOne(path); }
    catch (error) { result = failedResult(error); }
    if (!isCurrent(operation)) return;
    notify(accept(operation, path, result));
  }, [accept, change, failedResult, isCurrent, notify]);

  // 手动检查与定时检查共用同一批量引擎：silent 变体跳过 toast 提示。
  // 静默失败（IPC 整体失败或单条结果失败）不写入任何结果——上次成功
  // 发现的更新信息保留到下一次成功检查；返回值表示检查是否真正启动，
  // 供定时调度只在启动后记录检查时间戳。requestedPaths 允许定时任务
  // 指定完整可检查库存，不受列表页当前筛选影响。
  const runBatchCheck = useCallback(async ({ silent, paths: requestedPaths } = {}) => {
    if (batch.current) return false;
    const requested = requestedPaths ?? updatable.map(skill => skill.path);
    const paths = [...new Set(requested)]
      .filter(path => !active.current.has(path));
    if (!paths.length) return false;
    const operation = begin(paths);
    batch.current = operation;
    try {
      const results = await skillmateApi.updates.checkAll(paths);
      if (!isCurrent(operation)) return true;
      const byPath = new Map(results.map(result => [result.path, result]));
      if (silent) {
        // 单条失败/缺失结果不覆盖上次成功发现的更新信息
        paths.forEach(path => {
          const result = byPath.get(path);
          if (result && !isIncompleteUpdateCheck(result)) accept(operation, path, result);
        });
      } else {
        const accepted = paths.map(path => accept(operation, path, byPath.get(path)));
        const count = accepted.filter(isIncompleteUpdateCheck).length;
        showToast(t(count ? "updates.toast.batchIncomplete" : "updates.toast.batchDone", { count }), count ? "warn" : "success");
      }
    } catch (error) {
      if (!isCurrent(operation)) return true;
      // 静默失败保留上次成功结果，不把任何条目改写为失败
      if (!silent) {
        paths.forEach(path => accept(operation, path, failedResult(error)));
        showToast(t("updates.toast.batchFailed", { message: toUserErrorMessage(error, t("error.safeRetry")) }), "error");
      }
    } finally {
      finish(operation, paths);
      if (batch.current === operation) batch.current = null;
    }
    return true;
  }, [accept, begin, failedResult, finish, isCurrent, showToast, t, updatable]);

  // onClick 直接绑定时会传入事件对象，仅接受显式路径数组，其余回退到 updatable。
  const checkAllUpdates = useCallback((paths) => runBatchCheck({ silent: false, paths: Array.isArray(paths) ? paths : undefined }), [runBatchCheck]);
  const checkAllSilent = useCallback((paths) => runBatchCheck({ silent: true, paths: Array.isArray(paths) ? paths : undefined }), [runBatchCheck]);

  const checkUpdate = useCallback(async path => {
    if (active.current.has(path)) return;
    const operation = begin([path]);
    try { await probe(operation, path); }
    finally { finish(operation, [path]); }
  }, [begin, finish, probe]);

  const updateSkill = useCallback(async (path, planToken) => {
    if (active.current.has(path)) return false;
    const operation = begin([path], true);
    try {
      const result = await skillmateApi.updates.applyOne(path, planToken);
      // Superseded after success: undefined (not false) so the modal ignores instead of retrying.
      if (!isCurrent(operation)) return undefined;
      showToast(language === "en"
        ? t("updates.toast.updated")
        : (typeof result === "string" && result.trim() ? result : t("updates.toast.updated")), "success");
      await probe(operation, path);
      if (!isCurrent(operation)) return;
      await loadData({ resetUpdates: false });
      return true;
    } catch (error) {
      // A reset supersedes the operation: undefined tells UpdatePreviewModal to ignore it instead of asking for a retry.
      if (!isCurrent(operation)) return undefined;
      showToast(t("updates.toast.updateFailed", { message: toUserErrorMessage(error, t("error.safeRetry")) }), "error");
      return false;
    } finally { finish(operation, [path]); }
  }, [begin, finish, language, loadData, probe, showToast, t]);

  return { updateState, resetUpdateState, getSyncInfo, checkAllUpdates, checkAllSilent, checkUpdate, updateSkill };
}
