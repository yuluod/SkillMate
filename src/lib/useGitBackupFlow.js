import { useCallback, useMemo, useState } from "react";
import { buildGitBackupState } from "./skillmate.mjs";
import { skillmateApi } from "./skillmateApi.js";

function toDraft(config) {
  return {
    repoPath: config?.repoPath ?? config?.repo_path ?? "",
    remoteUrl: config?.remoteUrl ?? config?.remote_url ?? "",
    branch: config?.branch || "main",
  };
}

export function useGitBackupFlow({ saved, showToast, loadData }) {
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
      showToast("请输入备份仓库路径", "error");
      return;
    }
    if (saving || syncing) return;
    setSaving(true);
    try {
      await skillmateApi.backup.setup(state.payload);
      showToast("Git 备份已保存", "success");
      await loadData();
    } catch (e) {
      showToast(`保存失败: ${e}`, "error");
    } finally {
      setSaving(false);
    }
  }, [loadData, saving, showToast, state.payload, syncing]);

  const sync = useCallback(async () => {
    if (state.dirty) {
      showToast("Git 备份设置尚未保存，请先保存后再同步", "warn");
      return;
    }
    if (!state.configured) {
      showToast("请先配置并保存 Git 备份仓库", "warn");
      return;
    }
    if (saving || syncing) return;
    setSyncing(true);
    try {
      const result = await skillmateApi.backup.sync(`SkillMate sync ${new Date().toISOString()}`);
      showToast(String(result), "success");
      await loadData();
    } catch (e) {
      showToast(`同步失败: ${e}`, "error");
    } finally {
      setSyncing(false);
    }
  }, [loadData, saving, showToast, state.configured, state.dirty, syncing]);

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
