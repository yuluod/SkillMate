import { useCallback, useEffect, useMemo, useState } from "react";
import { skillmateApi } from "./skillmateApi.js";
import { toUserErrorMessage } from "./errorMessage.mjs";
import { useI18n } from "./i18n.jsx";

export function useLibrarySettingsFlow({ showToast, loadData }) {
  const { t } = useI18n();
  const [path, setPath] = useState("");
  const [savedPath, setSavedPath] = useState("");
  const [configurable, setConfigurable] = useState(true);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const result = await skillmateApi.library.settings();
      setPath(result.path);
      setSavedPath(result.path);
      setConfigurable(result.configurable !== false);
      setError("");
    } catch (reason) {
      setError(toUserErrorMessage(reason, t("error.safeRetry")));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => { void load(); }, [load]);

  const save = useCallback(async () => {
    if (saving || !path.trim()) return;
    setSaving(true);
    try {
      const result = await skillmateApi.library.setRoot(path.trim());
      setPath(result.path);
      setSavedPath(result.path);
      setConfigurable(result.configurable !== false);
      setError("");
      showToast(t("librarySettings.saved"), "success");
      await loadData({ resetUpdates: false });
    } catch (reason) {
      const message = toUserErrorMessage(reason, t("error.safeRetry"));
      setError(message);
      showToast(message, "error");
    } finally {
      setSaving(false);
    }
  }, [loadData, path, saving, showToast, t]);

  return {
    path,
    setPath,
    configurable,
    loading,
    saving,
    dirty: useMemo(() => path.trim() !== savedPath, [path, savedPath]),
    error,
    save,
    reload: load,
  };
}
