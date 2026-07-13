import { useCallback, useMemo, useState } from "react";
import { buildGitBackupState } from "./skillmate.mjs";
import { skillmateApi } from "./skillmateApi.js";

export function useGitBackupFlow({ saved, showToast, loadData }) {
  const [repoPath, setRepoPath] = useState(saved?.repo_path || "");
  const [branch, setBranch] = useState(saved?.branch || "main");
  const [remoteUrl, setRemoteUrl] = useState(saved?.remote_url || "");
  const [saving, setSaving] = useState(false);
  const [syncing, setSyncing] = useState(false);

  const hydrate = useCallback((config) => {
    setRepoPath(config?.repo_path || "");
    setRemoteUrl(config?.remote_url || "");
    setBranch(config?.branch || "main");
  }, []);

  const state = useMemo(() => buildGitBackupState({
    draft: { repoPath, remoteUrl, branch },
    saved,
    saving,
    syncing,
  }), [branch, remoteUrl, repoPath, saved, saving, syncing]);

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
    repoPath,
    setRepoPath,
    remoteUrl,
    setRemoteUrl,
    branch,
    setBranch,
    hydrate,
    save,
    sync,
    ...state,
  };
}
