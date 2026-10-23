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

  // 行组件（UpdateRow）用 React.memo 依赖这些回调的引用稳定性，内部链条全部包 useCallback。
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

  const checkAllUpdates = useCallback(async () => {
    if (batch.current) return;
    const paths = [...new Set(updatable.map(skill => skill.path))]
      .filter(path => !active.current.has(path));
    if (!paths.length) return;
    const operation = begin(paths);
    batch.current = operation;
    try {
      const results = await skillmateApi.updates.checkAll(paths);
      if (!isCurrent(operation)) return;
      const byPath = new Map(results.map(result => [result.path, result]));
      const accepted = paths.map(path => accept(operation, path, byPath.get(path)));
      const count = accepted.filter(isIncompleteUpdateCheck).length;
      showToast(t(count ? "updates.toast.batchIncomplete" : "updates.toast.batchDone", { count }), count ? "warn" : "success");
    } catch (error) {
      if (!isCurrent(operation)) return;
      paths.forEach(path => accept(operation, path, failedResult(error)));
      showToast(t("updates.toast.batchFailed", { message: toUserErrorMessage(error, t("error.safeRetry")) }), "error");
    } finally {
      finish(operation, paths);
      if (batch.current === operation) batch.current = null;
    }
  }, [accept, begin, failedResult, finish, isCurrent, showToast, t, updatable]);

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

  return { updateState, resetUpdateState, getSyncInfo, checkAllUpdates, checkUpdate, updateSkill };
}
