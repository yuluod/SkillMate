import React, { useEffect, useMemo, useState, useRef, useCallback } from "react";
import Icon from "./components/Icon.jsx";
import appIcon from "../src-tauri/icons/128x128.png";
import {
  ConfirmModal,
  AdoptionModal,
  DriftSyncModal,
  InstallModal,
  PreviewModal,
  TagEditorModal,
  TagManagerModal,
} from "./components/SkillMateModals.jsx";
import SettingsView from "./components/SettingsView.jsx";
import ScenarioView from "./components/ScenarioView.jsx";
import { AssistantsView, SkillsView, UpdatesView } from "./components/InventoryViews.jsx";
import DashboardView, { MarketDiscovery } from "./components/DashboardView.jsx";
import {
  buildAppUpdateView,
  buildDashboardStats,
  buildDriftGroups,
  buildScenarioSkillInventory,
  buildUniqueSkillInventory,
  filterSkillsByScenario,
  getMarketInstallRequest,
} from "./lib/skillmate.mjs";
import { useI18n } from "./lib/i18n.jsx";
import { useAppUpdateFlow } from "./lib/useAppUpdateFlow.js";
import { useGitBackupFlow } from "./lib/useGitBackupFlow.js";
import { useImportExportFlow } from "./lib/useImportExportFlow.js";
import { useInstallFlow } from "./lib/useInstallFlow.js";
import { useInstallPolicyFlow } from "./lib/useInstallPolicyFlow.js";
import { useLibrarySettingsFlow } from "./lib/useLibrarySettingsFlow.js";
import { useScenarioFlow } from "./lib/useScenarioFlow.js";
import { useSearchFlow } from "./lib/useSearchFlow.js";
import { useUpdateFlow } from "./lib/useUpdateFlow.js";
import { useProjectWorkspace } from "./lib/useProjectWorkspace.js";
import ProjectWorkspaceBar from "./components/ProjectWorkspaceBar.jsx";
import UpdatePreviewModal from "./components/UpdatePreviewModal.jsx";
import { createResettableTimer } from "./lib/toastTimer.mjs";
import { toUserErrorMessage } from "./lib/errorMessage.mjs";
import { skillmateApi } from "./lib/skillmateApi.js";

const EMPTY_DATA = { assistants: [], librarySkills: [], tags: [], scenarios: [], git: { enabled: false, remote_url: "" } };
const THEME_STORAGE_KEY = "skillmate-theme-mode";
const THEME_MODES = ["system", "light", "dark"];
const SKIN_STORAGE_KEY = "skillmate-skin";
const SKINS = ["ledger", "standard", "cardbox"];
const VIEWS = {
  dashboard: { titleKey: "nav.dashboard", icon: "dashboard" },
  skills: { titleKey: "nav.skills", icon: "skills" },
  ai: { titleKey: "nav.assistants", icon: "monitor" },
  scenarios: { titleKey: "nav.scenarios", icon: "scenarios" },
  updates: { titleKey: "nav.updates", icon: "updates" },
  settings: { titleKey: "nav.settings", icon: "settings" },
};
const NAV_VIEW_KEYS = ["dashboard", "skills", "ai", "scenarios", "updates", "settings"];

const SETTINGS_TAB_LABELS = {
  language: "settings.language",
  appearance: "settings.appearance",
  backup: "settings.tabs.backup",
  "app-update": "settings.tabs.appUpdate",
  "install-policy": "settings.tabs.installPolicy",
  library: "settings.tabs.library",
  data: "settings.tabs.data",
  skillset: "settings.tabs.skillset",
};



function getParamOverride(key, allowed, search) {
  if (search === undefined) {
    if (typeof window === "undefined") return null;
    search = window.location.search;
  }
  const value = new URLSearchParams(search).get(key);
  return allowed.includes(value) ? value : null;
}

export function persistPreference(storage, search, overrideKey, storageKey, allowed, value) {
  if (getParamOverride(overrideKey, allowed, search)) return false;
  storage.setItem(storageKey, value);
  return true;
}

function getSavedThemeMode() {
  if (typeof window === "undefined") return "system";
  const override = getParamOverride("theme", THEME_MODES);
  if (override) return override;
  const saved = window.localStorage.getItem(THEME_STORAGE_KEY);
  return THEME_MODES.includes(saved) ? saved : "system";
}

function getSavedSkin() {
  if (typeof window === "undefined") return "standard";
  const override = getParamOverride("skin", SKINS);
  if (override) return override;
  const saved = window.localStorage.getItem(SKIN_STORAGE_KEY);
  return SKINS.includes(saved) ? saved : "standard";
}

function getSystemTheme() {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function getInitialView() {
  if (typeof window === "undefined") return "dashboard";
  const parts = window.location.hash.replace(/^#\/?/, "").split("/");
  return VIEWS[parts[0]] ? parts[0] : "dashboard";
}

function getInitialSettingsTab() {
  if (typeof window === "undefined") return "language";
  const parts = window.location.hash.replace(/^#\/?/, "").split("/");
  return parts[0] === "settings" && SETTINGS_TAB_LABELS[parts[1]] ? parts[1] : "language";
}

function Skeleton() {
  return (
    <div className="skeleton">
      {[1,2,3,4,5,6].map(i => (
        <div className="skeleton-card" key={i}>
          <div className="sk-header"><div className="sk-icon" /><div className="sk-lines"><div className="sk-line w80" /><div className="sk-line w60" /></div></div>
          <div className="sk-line w40" /><div className="sk-path" /><div className="sk-btns"><div className="sk-btn" /><div className="sk-btn" /><div className="sk-btn" /></div>
        </div>
      ))}
    </div>
  );
}

function Loader({ label }) {
  return (
    <div className="loader-overlay">
      <div className="loader-spinner"><div /><div /><div /></div>
      <p>{label}</p>
    </div>
  );
}

const Logo = React.memo(function Logo() {
  return <img className="logo" src={appIcon} alt="" />;
});

function getDir(path) {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(0, i) : path;
}


function getStatePriority(state) {
  switch (state) {
    case "behind": return 0;
    case "failed": return 1;
    case "diverged": return 2;
    case "ahead_local": return 3;
    case "unsupported": return 4;
    case "current": return 5;
    default: return 6;
  }
}


function App() {
  const { t, language } = useI18n();
  const [data, setData] = useState(EMPTY_DATA);
  const [view, setView] = useState(getInitialView);
  const [skillFilterRequest, setSkillFilterRequest] = useState({ filter: "all", nonce: 0 });

  function openSkills(filter = "all") {
    setSkillFilterRequest((current) => ({ filter, nonce: current.nonce + 1 }));
    setView("skills");
  }
  const [tags, setTags] = useState([]);
  const [confirmState, setConfirmState] = useState({ open: false, title: "", message: "", confirmLabel: "", tone: "danger", onConfirm: null });
  const [sort, setSort] = useState("name");
  const [loading, setLoading] = useState(false);
  const [init, setInit] = useState(true);
  const [installOpen, setInstallOpen] = useState(false);
  const [discoveryOpen, setDiscoveryOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [preview, setPreview] = useState({ title: "", content: "", validation: null, diagnostics: [] });
  const [tagEditor, setTagEditor] = useState({ open: false, skills: [], selected: [], mode: "replace" });
  const [tagManagerOpen, setTagManagerOpen] = useState(false);
  const [selectedSkillPaths, setSelectedSkillPaths] = useState([]);
  const [toastState, setToastState] = useState({ show: false, msg: "", type: "", action: "", actionLabel: "", skipLabel: "" });
  const [theme, setTheme] = useState(getSavedThemeMode);
  const [skin, setSkin] = useState(getSavedSkin);
  const [newTagName, setNewTagName] = useState("");
  const [newTagColor, setNewTagColor] = useState("#58a6ff");
  const [settingsTab, setSettingsTab] = useState(getInitialSettingsTab);
  const [loadError, setLoadError] = useState(null);
  const [trashReceipt, setTrashReceipt] = useState(null);
  const [driftGroup, setDriftGroup] = useState(null);
  const [adoptionCandidate, setAdoptionCandidate] = useState(null);
  const [projectInspectionRevision, setProjectInspectionRevision] = useState(0);
  const projectWorkspace = useProjectWorkspace();
  const projectWorkspacePath = projectWorkspace.path;
  const [updatePreviewPath, setUpdatePreviewPath] = useState("");
  const {
    input: searchInput,
    query: search,
    update: handleSearchInput,
    clear: clearSearch,
  } = useSearchFlow();

  const [sysTheme, setSysTheme] = useState(getSystemTheme);
  const searchRef = useRef(null);
  const contentRef = useRef(null);
  const toastTimerRef = useRef(null);
  const trashTimerRef = useRef(null);
  const addTagPendingRef = useRef(false);
  const mountedRef = useRef(false);
  const loadRequestRef = useRef(0);
  // loadData 与 gitBackupFlow 互相依赖（见两处定义），hydrate 经此引用取最新值。
  const gitHydrateRef = useRef(null);
  // loadData 声明早于 useUpdateFlow，resetUpdateState 经此引用打破声明顺序循环。
  const resetUpdateStateRef = useRef(null);
  if (!toastTimerRef.current) {
    toastTimerRef.current = createResettableTimer();
  }
  if (!trashTimerRef.current) {
    trashTimerRef.current = createResettableTimer();
  }

  const resolved = theme === "system" ? sysTheme : theme;

  // 深链：视图与设置页签同步到 location.hash，前进后退可精确回溯
  useEffect(() => {
    const applyHash = () => {
      const parts = window.location.hash.replace(/^#\/?/, "").split("/");
      if (VIEWS[parts[0]]) {
        setView(parts[0]);
        if (parts[0] === "settings" && parts[1] && SETTINGS_TAB_LABELS[parts[1]]) {
          setSettingsTab(parts[1]);
        }
      }
    };
    window.addEventListener("hashchange", applyHash);
    return () => window.removeEventListener("hashchange", applyHash);
  }, []);

  useEffect(() => {
    const next = view === "settings" ? `#/settings/${settingsTab}` : `#/${view}`;
    if (window.location.hash !== next) window.location.hash = next;
  }, [view, settingsTab]);

  // 快捷键：Alt+1~6 切换主导航。
  useEffect(() => {
    const handler = (e) => {
      if (e.altKey && e.key >= "1" && e.key <= "6") {
        e.preventDefault();
        const next = NAV_VIEW_KEYS[parseInt(e.key) - 1];
        if (next === "skills") setSkillFilterRequest((current) => ({ filter: "all", nonce: current.nonce + 1 }));
        setView(next);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  useEffect(() => {
    if (contentRef.current) contentRef.current.scrollTop = 0;
  }, [view]);

  // 初始加载需要在 StrictMode 下保持幂等，并避免卸载后继续写状态。
  useEffect(() => {
    mountedRef.current = true;
    loadData({ resetUpdates: false });
    // 暂存区跨重启可见：启动后恢复未过期的撤销条，不阻塞首屏加载。
    refreshTrashState().catch(() => {});
    return () => {
      mountedRef.current = false;
      loadRequestRef.current += 1;
      toastTimerRef.current?.dispose();
      trashTimerRef.current?.dispose();
    };
  }, []);

  // Custom confirm dialog helper
  const confirmAction = useCallback((title, message, onConfirm, options = {}) => {
    setConfirmState({
      open: true,
      title,
      message,
      confirmLabel: options.confirmLabel || t("common.confirm"),
      tone: options.tone || "danger",
      onConfirm,
    });
  }, [t]);

  useEffect(() => {
    const m = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = () => setSysTheme(m.matches ? "dark" : "light");
    handler();
    m.addEventListener("change", handler);
    return () => m.removeEventListener("change", handler);
  }, []);
  useEffect(() => {
    persistPreference(window.localStorage, window.location.search, "theme", THEME_STORAGE_KEY, THEME_MODES, theme);
  }, [theme]);
  useEffect(() => { document.documentElement.setAttribute("data-theme", resolved); }, [resolved]);
  useEffect(() => {
    persistPreference(window.localStorage, window.location.search, "skin", SKIN_STORAGE_KEY, SKINS, skin);
    document.documentElement.setAttribute("data-skin", skin);
  }, [skin]);
  useEffect(() => {
    const handler = (e) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        if (document.querySelector('[role="dialog"], [role="alertdialog"]')) return;
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);
  const showToast = useCallback((msg, type = "", options = {}) => {
    const safeMessage = type === "error"
      ? toUserErrorMessage(msg, t("error.safeRetry"), t("common.messageSeparator"))
      : msg;
    setToastState({
      show: true,
      msg: safeMessage,
      type,
      action: options.action || "",
      actionLabel: options.actionLabel || "",
      skipLabel: options.skipLabel || "",
    });
    toastTimerRef.current.start(options.duration || 3000, () => {
      if (mountedRef.current) {
        setToastState({ show: false, msg: "", type: "", action: "", actionLabel: "", skipLabel: "" });
      }
    });
  }, [t]);

  const loadData = useCallback(async (options = {}) => {
    const resetUpdates = options?.resetUpdates ?? true;
    const requestId = ++loadRequestRef.current;
    setLoading(true);
    setLoadError(null);
    try {
      const { assistants, librarySkills, tags, scenarios, git, diagnostics = [] } = await skillmateApi.inventory.loadDashboard();
      if (!mountedRef.current || requestId !== loadRequestRef.current) return;
      const failedSections = new Set(diagnostics.map((item) => item.section));
      setData((current) => ({
        assistants,
        librarySkills,
        tags: failedSections.has("tags") ? current.tags : tags,
        scenarios: failedSections.has("scenarios") ? current.scenarios : scenarios,
        git: failedSections.has("git") ? current.git : git,
      }));
      if (!failedSections.has("tags")) {
        setTags((current) => tags.map((tag) => ({
          ...tag,
          selected: current.some((item) => item.id === tag.id && item.selected),
        })));
      }
      const diagnosticMessage = diagnostics
        .map((item) => `${t(`diagnostic.${item.section}`)}: ${item.message}`)
        .join(t("common.messageSeparator"));
      setLoadError(diagnosticMessage ? {
        summary: t("error.partialSummary"),
        impact: t("error.partialImpact"),
        details: diagnosticMessage,
      } : null);
      if (resetUpdates) resetUpdateStateRef.current?.();
      if (!failedSections.has("git")) gitHydrateRef.current?.(git);
      if (diagnosticMessage) showToast(t("toast.partialData"), "warn");
    } catch (e) {
      if (mountedRef.current && requestId === loadRequestRef.current) {
        setLoadError({
          summary: t("error.coreSummary"),
          impact: t("error.coreImpact"),
          details: String(e),
        });
        showToast(t("toast.loadFailed"), "error");
      }
    }
    finally {
      if (mountedRef.current && requestId === loadRequestRef.current) {
        setLoading(false);
        setInit(false);
      }
    }
  }, [showToast, t]);

  const selectedTags = useMemo(
    () => tags.filter(tag => tag.selected).map(tag => tag.id),
    [tags]
  );

  const allSkills = useMemo(() => {
    const deployed = buildUniqueSkillInventory(data.assistants, t);
    const inLibrary = data.librarySkills.map((skill) => ({
      ...skill,
      in_library: true,
      ai: t("skills.library"),
      aiIcon: "skillmate",
      availableIn: [],
    }));
    return [...deployed, ...inLibrary];
  }, [data.assistants, data.librarySkills, t]);
  const driftGroups = useMemo(() => buildDriftGroups(data.assistants, t), [data.assistants, t]);
  const scenarioSkills = useMemo(() => buildScenarioSkillInventory(allSkills), [allSkills]);
  const dashboardStats = useMemo(
    () => buildDashboardStats(data.assistants, data.librarySkills, t),
    [data.assistants, data.librarySkills, t]
  );

  const scenarioFlow = useScenarioFlow({
    scenarios: data.scenarios,
    allSkills: scenarioSkills,
    selectableSkills: scenarioSkills,
    showToast,
    loadData,
    setView,
  });
  const activeScenario = scenarioFlow.active;

  const gitBackupFlow = useGitBackupFlow({ saved: data.git, showToast, loadData });
  gitHydrateRef.current = gitBackupFlow.hydrate;

  const skills = useMemo(() => {
    let list = [...allSkills];
    if (search) list = list.filter(s => s.name.toLowerCase().includes(search.toLowerCase()));
    if (selectedTags.length > 0) list = list.filter(s => selectedTags.some(t => s.tags.includes(t)));
    if (activeScenario) {
      list = filterSkillsByScenario({ skills: list, activeScenarioPaths: activeScenario.skill_ids });
    }
    list.sort((a, b) => sort === "date" ? Number(b.modified||0) - Number(a.modified||0) : a.name.localeCompare(b.name, "zh-CN"));
    return list;
  }, [activeScenario, allSkills, search, selectedTags, sort]);

  const updatable = useMemo(() => {
    let list = [...allSkills];
    if (search) list = list.filter(s => s.name.toLowerCase().includes(search.toLowerCase()));
    if (selectedTags.length > 0) list = list.filter(s => selectedTags.some(t => s.tags.includes(t)));
    if (activeScenario) {
      list = filterSkillsByScenario({ skills: list, activeScenarioPaths: activeScenario.skill_ids });
    }
    list.sort((a, b) => a.name.localeCompare(b.name, "zh-CN"));
    return list;
  }, [activeScenario, allSkills, search, selectedTags]);

  const {
    updateState,
    resetUpdateState,
    getSyncInfo,
    checkAllUpdates,
    checkUpdate,
    updateSkill,
  } = useUpdateFlow({ updatable, showToast, loadData });
  resetUpdateStateRef.current = resetUpdateState;

  const {
    appUpdateState,
    autoCheckEnabled,
    setAutoCheckEnabled,
    checkAppUpdate,
    installAppUpdate,
    restartApp,
  } = useAppUpdateFlow({ showToast });

  useEffect(() => {
    let cancelled = false;
    let unlisten = () => {};

    import("@tauri-apps/api/event")
      .then(({ listen }) => listen("skillmate:tray-action", ({ payload }) => {
        if (payload === "settings") {
          setView("settings");
          return;
        }
        if (payload === "check-update") {
          setView("settings");
          setSettingsTab("app-update");
          void checkAppUpdate();
        }
      }))
      .then((stopListening) => {
        if (cancelled) {
          stopListening();
        } else {
          unlisten = stopListening;
        }
      })
      .catch(() => {
        // 浏览器预览环境没有 Tauri 事件桥，忽略即可。
      });

    return () => {
      cancelled = true;
      unlisten();
    };
  }, [checkAppUpdate]);

  useEffect(() => {
    import("@tauri-apps/api/event")
      .then(({ emit }) => emit("skillmate:tray-language", language))
      .catch(() => {
        // 浏览器预览环境没有 Tauri 事件桥，忽略即可。
      });
  }, [language]);

  const installPolicyFlow = useInstallPolicyFlow({ showToast });
  const librarySettingsFlow = useLibrarySettingsFlow({ showToast, loadData });

  const installFlow = useInstallFlow({
    installOpen,
    assistants: data.assistants,
    setInstallOpen,
    showToast,
    loadData,
    setLoading,
  });
  // installFlow 对象每次渲染都是新引用；行组件要的回调只依赖这几个稳定的底层方法。
  const { prepare: prepareInstallSource } = installFlow.source;
  const { setMode: setInstallTargetMode, setProjectPath: setInstallProjectPath } = installFlow.target;

  const {
    library: libraryFlow,
    scenarios: scenarioManifestFlow,
    manifest: manifestFlow,
    profiles: profileFlow,
  } = useImportExportFlow({ showToast, loadData });

  function toggleTag(id) {
    setTags(prev => prev.map(t => t.id === id ? { ...t, selected: !t.selected } : t));
  }

  const toggleSkillSelection = useCallback((path) => {
    setSelectedSkillPaths((current) => current.includes(path)
      ? current.filter((item) => item !== path)
      : [...current, path]);
  }, []);

  function toggleVisibleSkillSelection(visiblePaths) {
    setSelectedSkillPaths((current) => {
      const allSelected = visiblePaths.length > 0 && visiblePaths.every((path) => current.includes(path));
      return allSelected
        ? current.filter((path) => !visiblePaths.includes(path))
        : [...new Set([...current, ...visiblePaths])];
    });
  }


  async function addTag() {
    if (addTagPendingRef.current) return;
    if (!newTagName.trim()) { showToast(t("tags.enterName"), "error"); return; }
    addTagPendingRef.current = true;
    try {
      const tag = await skillmateApi.tags.add(newTagName.trim(), newTagColor);
      setTags(prev => [...prev, { ...tag, selected: false }]);
      setNewTagName("");
      setNewTagColor("#58a6ff");
      showToast(t("tags.added"), "success");
    } catch (e) { showToast(t("tags.addFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error"); }
    finally { addTagPendingRef.current = false; }
  }

  async function updateTag(tagId, name, color) {
    const nextName = String(name || "").trim();
    if (!nextName) { showToast(t("tags.enterName"), "error"); return; }
    try {
      const updated = await skillmateApi.tags.update(tagId, nextName, String(color));
      setTags((current) => current.map((tag) => tag.id === tagId ? { ...tag, ...updated } : tag));
      showToast(t("tags.tagUpdated"), "success");
    } catch (e) {
      showToast(t("tags.tagUpdateFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
    }
  }

  function requestDeleteTag(tag) {
    confirmAction(
      t("tags.deleteTitle"),
      t("tags.deleteConfirm", { name: tag.name }),
      async () => {
        try {
          await skillmateApi.tags.delete(tag.id);
          setTagEditor((current) => ({
            ...current,
            selected: current.selected.filter((id) => id !== tag.id),
          }));
          await loadData({ resetUpdates: false });
          showToast(t("tags.deleted"), "success");
        } catch (e) {
          showToast(t("tags.deleteFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error");
        }
      },
      { confirmLabel: t("tags.delete") },
    );
  }

  const openTagEditor = useCallback((skillOrSkills) => {
    const batch = Array.isArray(skillOrSkills);
    const editorSkills = batch ? skillOrSkills : [skillOrSkills];
    setTagEditor({
      open: true,
      skills: editorSkills,
      selected: batch ? [] : [...(editorSkills[0]?.tags || [])],
      mode: batch ? "add" : "replace",
    });
  }, []);

  function toggleSkillTag(tagId) {
    setTagEditor((current) => ({
      ...current,
      selected: current.selected.includes(tagId)
        ? current.selected.filter((id) => id !== tagId)
        : [...current.selected, tagId],
    }));
  }

  async function saveSkillTags() {
    if (tagEditor.skills.length === 0) return;
    const results = await Promise.allSettled(tagEditor.skills.map((skill) => {
      const nextTags = tagEditor.mode === "add"
        ? [...new Set([...(skill.tags || []), ...tagEditor.selected])]
        : tagEditor.selected;
      return skillmateApi.tags.updateSkill(skill.path, nextTags);
    }));
    const failedCount = results.filter((result) => result.status === "rejected").length;
    const successCount = results.length - failedCount;
    if (failedCount === results.length) {
      showToast(t("tags.updateFailedSafe"), "error");
      return;
    }
    showToast(failedCount > 0
      ? t("tags.batchPartial", { success: successCount, failed: failedCount })
      : t(tagEditor.mode === "add" ? "tags.addedMany" : "tags.updated"), failedCount > 0 ? "warn" : "success");
    setTagEditor({ open: false, skills: [], selected: [], mode: "replace" });
    setSelectedSkillPaths([]);
    await loadData();
  }

  const statAI = data.assistants.filter(a => a.exists).length;
  const statSkills = allSkills.length;
  const updateBadge = allSkills.reduce((count, skill) => {
    const state = updateState[skill.path]?.syncState || skill.sync_state;
    return count + (state === "behind" ? 1 : 0);
  }, 0);

  const openPreview = useCallback(async (path) => {
    try {
      const { content, validation, diagnostics = [] } = await skillmateApi.inventory.readSkill(path);
      const skill = allSkills.find((item) => item.path === path) || null;
      setPreview({
        title: path.split(/[\\/]/).pop(),
        content: content || t("preview.empty"),
        validation,
        diagnostics,
        skill,
      });
      setPreviewOpen(true);
    } catch (e) { showToast(t("preview.failed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error"); }
  }, [allSkills, showToast, t]);

  // 从后端重建撤销条状态：暂存区列表按过期时间升序，取最早一条展示并调度自动清理，
  // 清理成功后再刷新以浮出下一条；失败时只收起撤销条，剩余条目交由重启维护兜底。
  async function refreshTrashState() {
    let entries;
    try {
      entries = await skillmateApi.inventory.listTrashedSkills();
    } catch {
      // 拉取暂存区失败时保持现状，不改动当前撤销条。
      return;
    }
    if (!mountedRef.current) return;
    const first = Array.isArray(entries) && entries.length > 0 ? entries[0] : null;
    if (!first) {
      setTrashReceipt(null);
      trashTimerRef.current.clear();
      return;
    }
    setTrashReceipt(first);
    // 过期时间缺失或非法时不调度自动清理（NaN 会让 setTimeout 立即触发），交由重启后的维护兜底。
    const expiresAt = Number(first.expiresAt ?? first.expires_at);
    if (!Number.isFinite(expiresAt) || expiresAt <= 0) return;
    trashTimerRef.current.start(Math.max(0, expiresAt - Date.now()), async () => {
      if (mountedRef.current) setTrashReceipt(null);
      try {
        await skillmateApi.inventory.purgeTrash(first.token);
      } catch {
        // 清理失败时不再刷新，避免对同一条目反复重试。
        return;
      }
      if (mountedRef.current) refreshTrashState();
    });
  }

  const remove = useCallback(async (path, name, availableIn = []) => {
    const sharedWarning = availableIn.length > 1
      ? t("trash.shared", { assistants: availableIn.map((assistant) => assistant.name).join(t("common.listSeparator")) })
      : "";
    confirmAction(t("trash.title"), t("trash.message", { name, shared: sharedWarning }), async () => {
      setLoading(true);
      try {
        const receipt = await skillmateApi.inventory.trashSkill(path);
        setTrashReceipt(receipt);
        // 过期时间缺失或非法时不调度自动清理（NaN 会让 setTimeout 立即触发），交由重启维护兜底。
        const expiresAt = Number(receipt.expiresAt ?? receipt.expires_at);
        if (Number.isFinite(expiresAt) && expiresAt > 0) {
          trashTimerRef.current.start(Math.max(0, expiresAt - Date.now()), async () => {
            setTrashReceipt(null);
            try { await skillmateApi.inventory.purgeTrash(receipt.token); } catch { /* 重启维护会清理遗留暂存区。 */ return; }
            // 清理完成后刷新暂存区，浮出跨重启遗留的其他撤销条。
            if (mountedRef.current) refreshTrashState();
          });
        }
        showToast(t("trash.done", { name }), "success");
        await loadData();
      } catch (e) { showToast(t("trash.failed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error"); }
      finally { setLoading(false); }
    }, { confirmLabel: t("trash.action") });
  }, [confirmAction, loadData, showToast, t]);

  async function undoTrash() {
    if (!trashReceipt) return;
    trashTimerRef.current.clear();
    try {
      await skillmateApi.inventory.restoreTrash(trashReceipt.token);
      showToast(t("trash.restored", { name: trashReceipt.name }), "success");
      setTrashReceipt(null);
      await loadData();
      // 恢复后刷新暂存区，让下一条遗留撤销条（如有）继续可见。
      await refreshTrashState();
    } catch (error) {
      showToast(t("trash.restoreFailed", { message: toUserErrorMessage(error, t("error.safeRetry")) }), "error");
    }
  }

  const unlinkSymlink = useCallback(async (path, name) => {
    confirmAction(t("unlink.title"), t("unlink.message", { name }), async () => {
      setLoading(true);
      try {
        const r = await skillmateApi.inventory.unlinkSkill(path);
        showToast(typeof r === "string" && r.trim() ? r : t("unlink.done"), "success");
        await loadData();
      } catch (e) { showToast(t("unlink.failed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error"); }
      finally { setLoading(false); }
    }, { confirmLabel: t("unlink.action"), tone: "primary" });
  }, [confirmAction, loadData, showToast, t]);

  const orderedUpdatable = useMemo(() => [...updatable].sort((a, b) => {
    const aInfo = getSyncInfo(a);
    const bInfo = getSyncInfo(b);
    const priority = getStatePriority(aInfo.syncState) - getStatePriority(bInfo.syncState);
    if (priority !== 0) return priority;
    return a.name.localeCompare(b.name, "zh-CN");
  }), [getSyncInfo, updatable]);

  const appUpdateView = useMemo(
    () => buildAppUpdateView(appUpdateState, language),
    [appUpdateState, language]
  );
  const runAppUpdatePrimaryAction = useCallback(() => {
    if (appUpdateView.primaryAction === "install") {
      return installAppUpdate();
    }
    if (appUpdateView.primaryAction === "restart") {
      return restartApp();
    }
    return checkAppUpdate();
  }, [appUpdateView.primaryAction, checkAppUpdate, installAppUpdate, restartApp]);

  const openDir = useCallback(async (path) => {
    try { await skillmateApi.inventory.openFolder(getDir(path)); } catch (e) { showToast(t("folder.openFailed", { message: toUserErrorMessage(e, t("error.safeRetry")) }), "error"); }
  }, [showToast, t]);

  function installMarketSkill(item) {
    const request = getMarketInstallRequest(item);
    if (!request) {
      showToast(t("market.error", { message: t("common.unknown") }), "error");
      return;
    }
    installFlow.source.prepare(request.source, request.preferredSkillId);
    setInstallOpen(true);
  }

  function startAddSkill() {
    installFlow.startAdd();
    setInstallOpen(true);
  }

  const enableLibrarySkill = useCallback((skill) => {
    const skillsToEnable = Array.isArray(skill) ? skill : [skill];
    prepareInstallSource(skillsToEnable.map((item) => item.symlink_source || item.path), "", "local", "enable");
    setInstallTargetMode(projectWorkspacePath ? "symlink" : "copy");
    setInstallProjectPath(projectWorkspacePath);
    setInstallOpen(true);
  }, [prepareInstallSource, projectWorkspacePath, setInstallOpen, setInstallProjectPath, setInstallTargetMode]);

  async function completeDrift(message) {
    showToast(typeof message === "string" && message.trim() ? message : t("drift.complete"), "success");
    await loadData();
  }

  async function completeAdoption(message) {
    showToast(typeof message === "string" && message.trim() ? message : t("adoption.complete"), "success");
    await loadData();
    setProjectInspectionRevision((revision) => revision + 1);
  }

  return (
    <div className="app">
      {loading && <Loader label={t("common.loading")} />}

      <header className="header">
        <div className="header-left">
          <div className="header-brand">
            <Logo />
            <h1 className="app-name">SkillMate</h1>
          </div>
        </div>

        <div className="header-center">
          {(view === "skills" || view === "updates") && (
            <div className="search-box">
              <Icon name="search" size={16} />
              <input ref={searchRef} type="text" aria-label={view === "updates" ? t("header.searchUpdates") : t("nav.skills")} placeholder={view === "updates" ? t("header.searchUpdates") : t("header.searchSkills", { shortcut: typeof navigator !== "undefined" && navigator.platform?.startsWith("Mac") ? "⌘K" : "Ctrl+K" })} value={searchInput} onChange={e => handleSearchInput(e.target.value)} />
              {search && <button className="search-x" aria-label={t("header.clearSearch")} onClick={clearSearch}><Icon name="x" size={14} /></button>}
            </div>
          )}
        </div>

        <div className="header-right">
          {view === "skills" && (
            <div className="sort-tabs">
              <button className={`sort-tab ${sort === "name" ? "active" : ""}`} onClick={() => setSort("name")}><Icon name="tag" size={14} />{t("sort.name")}</button>
              <button className={`sort-tab ${sort === "date" ? "active" : ""}`} onClick={() => setSort("date")}><Icon name="clock" size={14} />{t("sort.time")}</button>
            </div>
          )}
          <button className="btn btn-ghost" onClick={loadData} title={t("common.refresh")} aria-label={t("common.refresh")}><Icon name="refresh" size={18} className={loading ? "spin" : ""} /></button>
        </div>
      </header>

      <div className="layout">
        <nav className="sidebar" aria-label={t("nav.primary")}>
          <div className="nav-items">
            {NAV_VIEW_KEYS.map((k) => {
              const v = VIEWS[k];
              return (
                <button key={k} aria-current={view === k ? "page" : undefined} className={`nav-item ${k === "settings" ? "nav-settings" : ""} ${view === k ? "active" : ""}`} onClick={() => (k === "skills" ? openSkills("all") : setView(k))}>
                  <Icon name={v.icon} size={18} />
                  <span>{t(v.titleKey)}</span>
                  {k === "skills" && statSkills > 0 && <span className="badge">{statSkills}</span>}
                  {k === "ai" && <span className="badge">{statAI}</span>}
                  {k === "updates" && updateBadge > 0 && <span className="badge warn">{updateBadge}</span>}
                </button>
              );
            })}
          </div>
        </nav>

        <main className={`content ${view === "settings" ? "content-settings" : ""}`} ref={contentRef}>
          {loadError && (
            <div className="load-error-banner" role="alert">
              <div className="load-error-copy">
                <strong>{loadError.summary}</strong>
                <span>{loadError.impact}</span>
                <details>
                  <summary>{t("error.diagnosticDetails")}</summary>
                  <code>{loadError.details}</code>
                </details>
              </div>
              <button className="btn btn-secondary btn-sm" onClick={() => loadData({ resetUpdates: false })}>{t("common.retry")}</button>
            </div>
          )}
          {init ? <Skeleton /> : view === "dashboard" && (
            <DashboardView
              stats={dashboardStats}
              tagCount={tags.length}
              driftGroups={driftGroups}
              onNavigate={(target, filter = "all") => (target === "skills" ? openSkills(filter) : setView(target))}
              onInstall={startAddSkill}
              onOpenDrift={setDriftGroup}
            />
          )}

          {!init && view === "skills" && (
            <SkillsView
              skills={skills}
              allSkills={allSkills}
              allSkillCount={allSkills.length}
              selectedTagCount={selectedTags.length}
              tags={tags}
              onInstall={startAddSkill}
              onToggleDiscovery={() => setDiscoveryOpen((open) => !open)}
              discoveryOpen={discoveryOpen}
              discovery={discoveryOpen ? <MarketDiscovery onInstall={installMarketSkill} installedSkills={allSkills} /> : null}
              filterRequest={skillFilterRequest}
              activeScenario={activeScenario}
              onClearScenario={() => scenarioFlow.setActiveId("")}
              onUpdate={setUpdatePreviewPath}
              getSyncInfo={getSyncInfo}
              selectedTagIds={tags.filter(tag => tag.selected).map(tag => tag.id)}
              onToggleTag={toggleTag}
              onClearTags={() => setTags(current => current.map(tag => ({ ...tag, selected: false })))}
              onManageTags={() => setTagManagerOpen(true)}
              onClearFilters={() => {
                clearSearch();
                setTags(current => current.map(tag => ({ ...tag, selected: false })));
                scenarioFlow.setActiveId("");
              }}
              onEditTags={openTagEditor}
              onOpenDirectory={openDir}
              onPreview={openPreview}
              onEnable={enableLibrarySkill}
              onAdopt={setAdoptionCandidate}
              onUnlink={unlinkSymlink}
              onRemove={remove}
              selectedSkillPaths={selectedSkillPaths}
              onToggleSelection={toggleSkillSelection}
              onToggleVisibleSelection={toggleVisibleSkillSelection}
              onClearSelection={() => setSelectedSkillPaths([])}
            />
          )}

          {view === "ai" && (
            <AssistantsView
              assistants={data.assistants}
              installedCount={statAI}
              onAdopt={setAdoptionCandidate}
              onMaterialize={setAdoptionCandidate}
              projectRevision={projectInspectionRevision}
              workspace={projectWorkspace}
              toolbar={<ProjectWorkspaceBar workspace={projectWorkspace} onOpen={() => setView("ai")} />}
            />
          )}

          {view === "scenarios" && (
            <ScenarioView
              scenarios={data.scenarios}
              skills={scenarioSkills}
              flow={scenarioFlow}
              onEnable={enableLibrarySkill}
            />
          )}

          {view === "updates" && (
            <UpdatesView
              orderedSkills={orderedUpdatable}
              updateState={updateState}
              getSyncInfo={getSyncInfo}
              checkAll={checkAllUpdates}
              checkOne={checkUpdate}
              updateOne={setUpdatePreviewPath}
              activeScenario={activeScenario}
              onClearScenario={() => scenarioFlow.setActiveId("")}
              tags={tags}
              selectedTagCount={selectedTags.length}
              selectedTagIds={tags.filter(tag => tag.selected).map(tag => tag.id)}
              onToggleTag={toggleTag}
              onClearTags={() => setTags(current => current.map(tag => ({ ...tag, selected: false })))}
              onManageTags={() => setTagManagerOpen(true)}
            />
          )}

          {view === "settings" && (
            <SettingsView
              activeTab={settingsTab}
              setActiveTab={setSettingsTab}
              appearance={{ skin, setSkin, theme, setTheme }}
              backup={{ ...gitBackupFlow, lastSync: data.git.last_sync }}
              appUpdate={{
                view: appUpdateView,
                autoCheckEnabled,
                setAutoCheckEnabled,
                runPrimaryAction: runAppUpdatePrimaryAction,
                check: checkAppUpdate,
              }}
              installPolicy={installPolicyFlow}
              librarySettings={librarySettingsFlow}
              data={{
                ...libraryFlow,
                exportLibrary: libraryFlow.exportLibraryFile,
                previewImport: libraryFlow.previewImportLibraryFile,
                importLibrary: libraryFlow.importLibraryFile,
                scenarioManifestPath: scenarioManifestFlow.path,
                scenarioManifestMode: scenarioManifestFlow.mode,
                scenarioManifestPreview: scenarioManifestFlow.preview,
                previewingScenarioManifest: scenarioManifestFlow.previewing,
                applyingScenarioManifest: scenarioManifestFlow.applying,
                scenarioManifestPreviewCurrent: scenarioManifestFlow.previewCurrent,
                updateScenarioManifestPath: scenarioManifestFlow.updatePath,
                updateScenarioManifestMode: scenarioManifestFlow.updateMode,
                exportScenarioManifest: scenarioManifestFlow.exportFile,
                previewScenarioManifest: scenarioManifestFlow.previewFile,
                importScenarioManifest: scenarioManifestFlow.importFile,
              }}
              skillSet={{
                manifestPath: manifestFlow.path,
                projectManifestRoot: manifestFlow.projectRoot,
                manifestPreview: manifestFlow.preview,
                previewingManifest: manifestFlow.previewing,
                applyingManifest: manifestFlow.applying,
                manifestPreviewCurrent: manifestFlow.previewCurrent,
                updateManifestPath: manifestFlow.updatePath,
                setProjectManifestRoot: manifestFlow.setProjectRoot,
                exportManifest: manifestFlow.exportFile,
                exportProjectManifest: manifestFlow.exportProjectFile,
                previewManifest: manifestFlow.previewFile,
                applyManifest: manifestFlow.applyFile,
                profiles: profileFlow.store,
                profileName: profileFlow.name,
                profileDescription: profileFlow.description,
                profilePreview: profileFlow.preview,
                previewingProfile: profileFlow.previewing,
                applyingProfile: profileFlow.applying,
                setProfileName: profileFlow.setName,
                setProfileDescription: profileFlow.setDescription,
                saveProfile: profileFlow.save,
                previewProfile: profileFlow.previewOne,
                applyProfile: profileFlow.applyOne,
                rollbackProfile: profileFlow.rollback,
              }}
            />
          )}
        </main>
      </div>

      {installOpen && (
        <InstallModal
          flow={installFlow}
          assistants={data.assistants}
          loading={loading}
          onClose={() => setInstallOpen(false)}
        />
      )}

      {previewOpen && (
        <PreviewModal
          preview={preview}
          getSyncInfo={getSyncInfo}
          driftGroup={driftGroups.find((group) => group.name === preview.skill?.name)}
          onCheckUpdate={checkUpdate}
          onOpenDrift={(group) => {
            setPreviewOpen(false);
            setDriftGroup(group);
          }}
          onClose={() => setPreviewOpen(false)}
        />
      )}

      {tagEditor.open && (
        <TagEditorModal
          tagEditor={tagEditor}
          tags={tags}
          toggleSkillTag={toggleSkillTag}
          saveSkillTags={saveSkillTags}
          onClose={() => setTagEditor({ open: false, skills: [], selected: [], mode: "replace" })}
        />
      )}

      {tagManagerOpen && (
        <TagManagerModal
          tags={tags}
          name={newTagName}
          color={newTagColor}
          setName={setNewTagName}
          setColor={setNewTagColor}
          onAdd={addTag}
          onUpdate={updateTag}
          onDelete={requestDeleteTag}
          onClose={() => setTagManagerOpen(false)}
        />
      )}

      <div className={`toast ${toastState.show ? "show" : ""} ${toastState.type}`} role="status" aria-live="polite" aria-atomic="true">
        {toastState.show && (
          <>
            <span className="toast-message">{toastState.msg}</span>
            {toastState.action === "install-update" && (
              <div className="toast-actions">
                <button
                  className="btn btn-primary btn-sm toast-action"
                  onClick={() => {
                    toastTimerRef.current.clear();
                    setToastState({ show: false, msg: "", type: "", action: "", actionLabel: "", skipLabel: "" });
                    void installAppUpdate();
                  }}
                >
                  <Icon name="download" size={14} />{toastState.actionLabel}
                </button>
                <button
                  className="btn btn-ghost btn-sm toast-action"
                  onClick={() => {
                    toastTimerRef.current.clear();
                    setToastState({ show: false, msg: "", type: "", action: "", actionLabel: "", skipLabel: "" });
                  }}
                >
                  {toastState.skipLabel}
                </button>
              </div>
            )}
          </>
        )}
      </div>
      {trashReceipt && <div className="undo-bar" role="status"><span>{t("trash.done", { name: trashReceipt.name })}</span><button className="btn btn-secondary btn-sm" onClick={undoTrash}><Icon name="undo" size={14} />{t("trash.undo")}</button></div>}

      {driftGroup && <DriftSyncModal group={driftGroup} onClose={() => setDriftGroup(null)} onComplete={completeDrift} />}
      {updatePreviewPath && <UpdatePreviewModal path={updatePreviewPath} onClose={() => setUpdatePreviewPath("")} onConfirm={updateSkill} />}

      {adoptionCandidate && <AdoptionModal candidate={adoptionCandidate} onClose={() => setAdoptionCandidate(null)} onComplete={completeAdoption} />}

      {confirmState.open && (
        <ConfirmModal
          confirmState={confirmState}
          onClose={() => setConfirmState({ open: false, title: "", message: "", confirmLabel: "", tone: "danger", onConfirm: null })}
          onConfirm={() => {
            const cb = confirmState.onConfirm;
            setConfirmState({ open: false, title: "", message: "", confirmLabel: "", tone: "danger", onConfirm: null });
            cb?.();
          }}
        />
      )}
    </div>
  );
}

export default App;
