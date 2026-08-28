import React, { useEffect, useMemo, useState, useRef, useCallback } from "react";
import Icon from "./components/Icon.jsx";
import appIcon from "../src-tauri/icons/128x128.png";
import {
  ConfirmModal,
  DriftSyncModal,
  InstallModal,
  PreviewModal,
  TagEditorModal,
} from "./components/SkillMateModals.jsx";
import SettingsView from "./components/SettingsView.jsx";
import ScenarioView from "./components/ScenarioView.jsx";
import { AssistantsView, SkillsView, UpdatesView } from "./components/InventoryViews.jsx";
import DashboardView from "./components/DashboardView.jsx";
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
import { useScenarioFlow } from "./lib/useScenarioFlow.js";
import { useSearchFlow } from "./lib/useSearchFlow.js";
import { useUpdateFlow } from "./lib/useUpdateFlow.js";
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

const SETTINGS_TAB_LABELS = {
  language: "settings.language",
  appearance: "settings.appearance",
  backup: "settings.tabs.backup",
  "app-update": "settings.tabs.appUpdate",
  "install-policy": "settings.tabs.installPolicy",
  data: "settings.tabs.data",
  skillset: "settings.tabs.skillset",
  tags: "settings.tabs.tags",
};

const VIEW_CONTEXT_LABELS = {
  dashboard: "header.context.dashboard",
  skills: "header.context.skills",
  ai: "header.context.assistants",
  scenarios: "header.context.scenarios",
  updates: "header.context.updates",
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
  if (typeof window === "undefined") return "ledger";
  const override = getParamOverride("skin", SKINS);
  if (override) return override;
  const saved = window.localStorage.getItem(SKIN_STORAGE_KEY);
  return SKINS.includes(saved) ? saved : "ledger";
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

function TagFilterBar({ tags, selectedCount, onToggle, onClear }) {
  const { t } = useI18n();
  if (tags.length === 0) return null;

  return (
    <div className="content-tag-filter" aria-label={t("sidebar.tags")}>
      <div className="content-tag-filter-label">
        <Icon name="tag" size={15} />
        <span>{t("sidebar.tags")}</span>
      </div>
      <div className="tag-list">
        {tags.map(tag => (
          <button
            key={tag.id}
            className={`tag-chip ${tag.selected ? "active" : ""}`}
            style={{ "--c": tag.color }}
            aria-pressed={tag.selected}
            onClick={() => onToggle(tag.id)}
          >
            <span className="tag-dot" />{tag.name}
          </button>
        ))}
      </div>
      {selectedCount > 0 && <button className="content-tag-filter-clear" onClick={onClear}>{t("common.clear")}</button>}
    </div>
  );
}


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
  const [tags, setTags] = useState([]);
  const [confirmState, setConfirmState] = useState({ open: false, title: "", message: "", confirmLabel: "", tone: "danger", onConfirm: null });
  const [sort, setSort] = useState("name");
  const [loading, setLoading] = useState(false);
  const [init, setInit] = useState(true);
  const [installOpen, setInstallOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [preview, setPreview] = useState({ title: "", content: "", validation: null, diagnostics: [] });
  const [tagEditor, setTagEditor] = useState({ open: false, skills: [], selected: [], mode: "replace" });
  const [selectedSkillPaths, setSelectedSkillPaths] = useState([]);
  const [toastState, setToastState] = useState({ show: false, msg: "", type: "" });
  const [theme, setTheme] = useState(getSavedThemeMode);
  const [skin, setSkin] = useState(getSavedSkin);
  const [newTagName, setNewTagName] = useState("");
  const [newTagColor, setNewTagColor] = useState("#58a6ff");
  const [settingsTab, setSettingsTab] = useState(getInitialSettingsTab);
  const [loadError, setLoadError] = useState(null);
  const [trashReceipt, setTrashReceipt] = useState(null);
  const [driftGroup, setDriftGroup] = useState(null);
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
  const mountedRef = useRef(false);
  const loadRequestRef = useRef(0);
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

  // 快捷键：Alt+1~6 切换视图
  useEffect(() => {
    const viewKeys = ["dashboard", "skills", "ai", "scenarios", "updates", "settings"];
    const handler = (e) => {
      if (e.altKey && e.key >= "1" && e.key <= "6") {
        e.preventDefault();
        setView(viewKeys[parseInt(e.key) - 1]);
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
    return () => {
      mountedRef.current = false;
      loadRequestRef.current += 1;
      toastTimerRef.current?.dispose();
      trashTimerRef.current?.dispose();
    };
  }, []);

  // Custom confirm dialog helper
  function confirmAction(title, message, onConfirm, options = {}) {
    setConfirmState({
      open: true,
      title,
      message,
      confirmLabel: options.confirmLabel || t("common.confirm"),
      tone: options.tone || "danger",
      onConfirm,
    });
  }

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
  const showToast = useCallback((msg, type = "") => {
    const safeMessage = type === "error"
      ? toUserErrorMessage(msg, t("error.safeRetry"), t("common.messageSeparator"))
      : msg;
    setToastState({ show: true, msg: safeMessage, type });
    toastTimerRef.current.start(3000, () => {
      if (mountedRef.current) {
        setToastState({ show: false, msg: "", type: "" });
      }
    });
  }, [t]);

  async function loadData(options = {}) {
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
      if (resetUpdates) resetUpdateState();
      if (!failedSections.has("git")) gitBackupFlow.hydrate(git);
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
  }

  const selectedTags = useMemo(
    () => tags.filter(tag => tag.selected).map(tag => tag.id),
    [tags]
  );

  const allSkills = useMemo(() => {
    const deployed = buildUniqueSkillInventory(data.assistants);
    const inLibrary = data.librarySkills.map((skill) => ({
      ...skill,
      in_library: true,
      ai: t("skills.library"),
      aiIcon: "skillmate",
      availableIn: [],
    }));
    return [...deployed, ...inLibrary];
  }, [data.assistants, data.librarySkills, t]);
  const driftGroups = useMemo(() => buildDriftGroups(data.assistants), [data.assistants]);
  const scenarioSkills = useMemo(() => buildScenarioSkillInventory(allSkills), [allSkills]);
  const dashboardStats = useMemo(
    () => buildDashboardStats(data.assistants, data.librarySkills),
    [data.assistants, data.librarySkills]
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

  const {
    appUpdateState,
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

  const installFlow = useInstallFlow({
    installOpen,
    assistants: data.assistants,
    setInstallOpen,
    showToast,
    loadData,
    setLoading,
  });

  const {
    library: libraryFlow,
    scenarios: scenarioManifestFlow,
    manifest: manifestFlow,
    profiles: profileFlow,
  } = useImportExportFlow({ showToast, loadData });

  function toggleTag(id) {
    setTags(prev => prev.map(t => t.id === id ? { ...t, selected: !t.selected } : t));
  }

  function toggleSkillSelection(path) {
    setSelectedSkillPaths((current) => current.includes(path)
      ? current.filter((item) => item !== path)
      : [...current, path]);
  }

  function toggleVisibleSkillSelection() {
    const visiblePaths = skills.map((skill) => skill.path);
    const allSelected = visiblePaths.length > 0 && visiblePaths.every((path) => selectedSkillPaths.includes(path));
    setSelectedSkillPaths((current) => allSelected
      ? current.filter((path) => !visiblePaths.includes(path))
      : [...new Set([...current, ...visiblePaths])]);
  }


  async function addTag() {
    if (!newTagName.trim()) { showToast(t("tags.enterName"), "error"); return; }
    try {
      const tag = await skillmateApi.tags.add(newTagName.trim(), newTagColor);
      setTags(prev => [...prev, { ...tag, selected: false }]);
      setNewTagName("");
      setNewTagColor("#58a6ff");
      showToast(t("tags.added"), "success");
    } catch (e) { showToast(t("tags.addFailed", { message: String(e) }), "error"); }
  }

  function openTagEditor(skillOrSkills) {
    const batch = Array.isArray(skillOrSkills);
    const editorSkills = batch ? skillOrSkills : [skillOrSkills];
    setTagEditor({
      open: true,
      skills: editorSkills,
      selected: batch ? [] : [...(editorSkills[0]?.tags || [])],
      mode: batch ? "add" : "replace",
    });
  }

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

  async function openPreview(path) {
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
    } catch (e) { showToast(t("preview.failed", { message: String(e) }), "error"); }
  }

  async function remove(path, name, availableIn = []) {
    const sharedWarning = availableIn.length > 1
      ? t("trash.shared", { assistants: availableIn.map((assistant) => assistant.name).join(t("common.listSeparator")) })
      : "";
    confirmAction(t("trash.title"), t("trash.message", { name, shared: sharedWarning }), async () => {
      setLoading(true);
      try {
        const receipt = await skillmateApi.inventory.trashSkill(path);
        setTrashReceipt(receipt);
        trashTimerRef.current.start(Math.max(0, Number(receipt.expiresAt || receipt.expires_at) - Date.now()), async () => {
          setTrashReceipt(null);
          try { await skillmateApi.inventory.purgeTrash(receipt.token); } catch { /* 重启维护会清理遗留暂存区。 */ }
        });
        showToast(t("trash.done", { name }), "success");
        await loadData();
      } catch (e) { showToast(t("trash.failed", { message: String(e) }), "error"); }
      finally { setLoading(false); }
    }, { confirmLabel: t("trash.action") });
  }

  async function undoTrash() {
    if (!trashReceipt) return;
    trashTimerRef.current.clear();
    try {
      await skillmateApi.inventory.restoreTrash(trashReceipt.token);
      showToast(t("trash.restored", { name: trashReceipt.name }), "success");
      setTrashReceipt(null);
      await loadData();
    } catch (error) {
      showToast(t("trash.restoreFailed", { message: String(error) }), "error");
    }
  }

  async function unlinkSymlink(path, name) {
    confirmAction(t("unlink.title"), t("unlink.message", { name }), async () => {
      setLoading(true);
      try {
        const r = await skillmateApi.inventory.unlinkSkill(path);
        showToast(String(r), "success");
        await loadData();
      } catch (e) { showToast(t("unlink.failed", { message: String(e) }), "error"); }
      finally { setLoading(false); }
    }, { confirmLabel: t("unlink.action"), tone: "primary" });
  }

  const orderedUpdatable = useMemo(() => [...updatable].sort((a, b) => {
    const aInfo = getSyncInfo(a);
    const bInfo = getSyncInfo(b);
    const priority = getStatePriority(aInfo.syncState) - getStatePriority(bInfo.syncState);
    if (priority !== 0) return priority;
    return a.name.localeCompare(b.name, "zh-CN");
  }), [getSyncInfo, updatable]);

  const updateStats = useMemo(() => {
    let behind = 0;
    let syncable = 0;
    let failed = 0;
    orderedUpdatable.forEach((skill) => {
      const info = getSyncInfo(skill);
      if (info.syncState === "behind") behind += 1;
      if (info.canSync) syncable += 1;
      if (info.syncState === "failed") failed += 1;
    });
    return { behind, syncable, failed };
  }, [getSyncInfo, orderedUpdatable]);

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

  async function openDir(path) {
    try { await skillmateApi.inventory.openFolder(getDir(path)); } catch (e) { showToast(t("folder.openFailed", { message: String(e) }), "error"); }
  }

  function installMarketSkill(item) {
    const request = getMarketInstallRequest(item);
    if (!request) {
      showToast(t("market.error", { message: t("common.unknown") }), "error");
      return;
    }
    installFlow.source.prepare(request.source, request.preferredSkillId);
    setInstallOpen(true);
  }

  function enableLibrarySkill(skill) {
    installFlow.source.prepare(skill.symlink_source || skill.path, "", "local", "enable");
    setInstallOpen(true);
  }

  async function completeDrift(message) {
    showToast(String(message || t("drift.complete")), "success");
    await loadData();
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
          <div className="header-context" aria-label={t(VIEWS[view].titleKey)}>
            <span>{t(VIEWS[view].titleKey)}</span>
            <span className="header-context-separator">/</span>
            <strong>{t(view === "settings" ? SETTINGS_TAB_LABELS[settingsTab] : VIEW_CONTEXT_LABELS[view])}</strong>
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
        <nav className="sidebar">
          <div className="nav-items">
            {Object.entries(VIEWS).map(([k, v]) => (
              <button key={k} className={`nav-item ${view === k ? "active" : ""}`} onClick={() => setView(k)}>
                <Icon name={v.icon} size={18} />
                <span>{t(v.titleKey)}</span>
                {k === "skills" && statSkills > 0 && <span className="badge">{statSkills}</span>}
                {k === "ai" && <span className="badge">{statAI}</span>}
                {k === "updates" && updateBadge > 0 && <span className="badge warn">{updateBadge}</span>}
              </button>
            ))}
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
          {activeScenario && (
            <div className="settings-card" style={{ marginBottom: 16 }}>
              <div className="settings-body" style={{ padding: 14 }}>
                <div className="card-actions" style={{ justifyContent: "space-between" }}>
                  <div>
                    <strong>{t("scenarioFilter.title", { name: activeScenario.name })}</strong>
                    <div className="git-meta">{t("scenarioFilter.count", { count: activeScenario.skill_ids.length })}</div>
                  </div>
                  <button className="btn btn-secondary btn-sm" onClick={() => scenarioFlow.setActiveId("")}>
                    <Icon name="x" size={14} />{t("scenarioFilter.clear")}
                  </button>
                </div>
              </div>
            </div>
          )}
          {(view === "skills" || view === "updates") && (
            <TagFilterBar
              tags={tags}
              selectedCount={selectedTags.length}
              onToggle={toggleTag}
              onClear={() => setTags(current => current.map(tag => ({ ...tag, selected: false })))}
            />
          )}
          {init ? <Skeleton /> : view === "dashboard" && (
            <DashboardView stats={dashboardStats} tagCount={tags.length} driftGroups={driftGroups} onNavigate={setView} onMarketInstall={installMarketSkill} onOpenDrift={setDriftGroup} />
          )}

          {!init && view === "skills" && (
            <SkillsView
              skills={skills}
              allSkills={allSkills}
              allSkillCount={allSkills.length}
              selectedTagCount={selectedTags.length}
              tags={tags}
              onInstall={() => {
                installFlow.startAdd();
                setInstallOpen(true);
              }}
              onClearFilters={() => {
                clearSearch();
                setTags(current => current.map(tag => ({ ...tag, selected: false })));
                scenarioFlow.setActiveId("");
              }}
              onEditTags={openTagEditor}
              onOpenDirectory={openDir}
              onPreview={openPreview}
              onEnable={enableLibrarySkill}
              onUnlink={unlinkSymlink}
              onRemove={remove}
              selectedSkillPaths={selectedSkillPaths}
              onToggleSelection={toggleSkillSelection}
              onToggleVisibleSelection={toggleVisibleSkillSelection}
              onClearSelection={() => setSelectedSkillPaths([])}
            />
          )}

          {view === "ai" && (
            <AssistantsView assistants={data.assistants} installedCount={statAI} />
          )}

          {view === "scenarios" && (
            <ScenarioView scenarios={data.scenarios} skills={scenarioSkills} flow={scenarioFlow} />
          )}

          {view === "updates" && (
            <UpdatesView
              skills={updatable}
              orderedSkills={orderedUpdatable}
              stats={updateStats}
              updateState={updateState}
              getSyncInfo={getSyncInfo}
              checkAll={checkAllUpdates}
              checkOne={checkUpdate}
              updateOne={updateSkill}
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
                runPrimaryAction: runAppUpdatePrimaryAction,
                check: checkAppUpdate,
              }}
              installPolicy={installPolicyFlow}
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
              tags={{
                tags,
                name: newTagName,
                color: newTagColor,
                setName: setNewTagName,
                setColor: setNewTagColor,
                add: addTag,
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

      <div className={`toast ${toastState.show ? "show" : ""} ${toastState.type}`} role="status" aria-live="polite" aria-atomic="true">{toastState.show ? toastState.msg : ""}</div>
      {trashReceipt && <div className="undo-bar" role="status"><span>{t("trash.done", { name: trashReceipt.name })}</span><button className="btn btn-secondary btn-sm" onClick={undoTrash}><Icon name="undo" size={14} />{t("trash.undo")}</button></div>}

      {driftGroup && <DriftSyncModal group={driftGroup} onClose={() => setDriftGroup(null)} onComplete={completeDrift} />}

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
