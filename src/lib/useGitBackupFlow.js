import { useCallback, useMemo, useState } from "react";
import { buildGitBackupState } from "./skillmate.mjs";
import { skillmateApi } from "./skillmateApi.js";
import { useI18n } from "./i18n.jsx";

function toDraft(config) {
  return {
    repoPath: config?.repoPath ?? config?.repo_path ?? "",
    remoteUrl: config?.remoteUrl ?? config?.remote_url ?? "",
    branch: config?.branch || "main",
  };
}

export function useGitBackupFlow({ saved, showToast, loadData }) {
  const { t, language } = useI18n();
  const [draft, setDraft] = useState(() => toDraft(saved));
  const [saving, setSaving] = useState(false);
  const [syncing, setSyncing] = useState(false);

  const setRepoPath = useCallback((repoPath) => {
    setDraft((current) => ({ ...current, repoPath }));
  }, []);
  const setRemoteUrl = useCallback((remoteUrl) => {
    setDraft((current) => ({ ...current, remoteUrl }));
  }, []);
  const setBranch = useCallback((branch) => {
    setDraft((current) => ({ ...current, branch }));
  }, []);

  const hydrate = useCallback((config) => {
    setDraft((current) => (
      buildGitBackupState({ draft: current, saved }).dirty ? current : toDraft(config)
    ));
  }, [saved]);

  const state = useMemo(() => buildGitBackupState({
    draft,
    saved,
    saving,
    syncing,
  }), [draft, saved, saving, syncing]);

  const save = useCallback(async () => {
    if (!state.payload.repoPath) {
      showToast(t("backup.toast.enterPath"), "error");
      return;
    }
    if (saving || syncing) return;
    setSaving(true);
    try {
      await skillmateApi.backup.setup(state.payload);
      showToast(t("backup.toast.saved"), "success");
      await loadData();
    } catch (e) {
      showToast(t("backup.toast.saveFailed", { message: String(e) }), "error");
    } finally {
      setSaving(false);
    }
  }, [loadData, saving, showToast, state.payload, syncing, t]);

  const sync = useCallback(async () => {
    if (state.dirty) {
      showToast(t("backup.toast.unsaved"), "warn");
      return;
    }
    if (!state.configured) {
      showToast(t("backup.toast.unconfigured"), "warn");
      return;
    }
    if (saving || syncing) return;
    setSyncing(true);
    try {
      const result = await skillmateApi.backup.sync(`SkillMate sync ${new Date().toISOString()}`);
      showToast(language === "en" ? t("backup.toast.synced") : String(result || t("backup.toast.synced")), "success");
      await loadData();
    } catch (e) {
      showToast(t("backup.toast.syncFailed", { message: String(e) }), "error");
    } finally {
      setSyncing(false);
    }
  }, [language, loadData, saving, showToast, state.configured, state.dirty, syncing, t]);

  return {
    repoPath: draft.repoPath,
    setRepoPath,
    remoteUrl: draft.remoteUrl,
    setRemoteUrl,
    branch: draft.branch,
    setBranch,
    hydrate,
    save,
    sync,
    ...state,
  };
}
