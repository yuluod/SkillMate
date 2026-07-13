import React, { useEffect, useMemo, useState, useRef, useCallback } from "react";
import Icon from "./components/Icon.jsx";
import {
  ConfirmModal,
  InstallModal,
  PreviewModal,
  TagEditorModal,
} from "./components/SkillMateModals.jsx";
import SettingsView from "./components/SettingsView.jsx";
import ScenarioView from "./components/ScenarioView.jsx";
import { AssistantsView, SkillsView, UpdatesView } from "./components/InventoryViews.jsx";
import {
  buildAppUpdateView,
  buildUniqueSkillInventory,
  filterSkillsByScenario,
} from "./lib/skillmate.mjs";
import {
  useAppUpdateFlow,
  useGitBackupFlow,
  useImportExportFlow,
  useInstallFlow,
  useInstallPolicyFlow,
  useScenarioFlow,
  useUpdateFlow,
} from "./lib/skillmateFlows.js";
import { createResettableTimer } from "./lib/toastTimer.mjs";
import { skillmateApi } from "./lib/skillmateApi.js";

const EMPTY_DATA = { assistants: [], tags: [], scenarios: [], git: { enabled: false, remote_url: "" } };
const THEME_STORAGE_KEY = "skillmate-theme-mode";
const THEME_MODES = ["system", "light", "dark"];
const VIEWS = {
  skills: { title: "Skills", icon: "skills" },
  ai: { title: "AI 助手", icon: "assistants" },
  scenarios: { title: "场景", icon: "scenarios" },
  updates: { title: "更新", icon: "updates" },
  settings: { title: "设置", icon: "settings" },
};



function getSavedThemeMode() {
  if (typeof window === "undefined") return "system";
  const saved = window.localStorage.getItem(THEME_STORAGE_KEY);
  return THEME_MODES.includes(saved) ? saved : "system";
}

function getSystemTheme() {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
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

function Loader() {
  return (
    <div className="loader-overlay">
      <div className="loader-spinner"><div /><div /><div /></div>
      <p>加载中...</p>
    </div>
  );
}

const Logo = React.memo(function Logo() {
  return (
    <svg className="logo" viewBox="0 0 48 48">
      <defs><linearGradient id="g" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" stopColor="#58a6ff" /><stop offset="100%" stopColor="#a855f7" /></linearGradient></defs>
      <rect x="4" y="4" width="40" height="40" rx="12" fill="url(#g)" />
      <path d="M14 18L20 24L14 30" stroke="white" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" fill="none" />
      <path d="M22 24H34" stroke="white" strokeWidth="3" strokeLinecap="round" />
      <circle cx="34" cy="34" r="5" fill="white" fillOpacity="0.9" />
    </svg>
  );
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
  const [data, setData] = useState(EMPTY_DATA);
  const [view, setView] = useState("skills");
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [tags, setTags] = useState([]);
  const [confirmState, setConfirmState] = useState({ open: false, title: "", message: "", confirmLabel: "确认", tone: "danger", onConfirm: null });
  const [sort, setSort] = useState("name");
  const [loading, setLoading] = useState(false);
  const [init, setInit] = useState(true);
  const [installOpen, setInstallOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [preview, setPreview] = useState({ title: "", content: "", validation: null });
  const [tagEditor, setTagEditor] = useState({ open: false, skill: null, selected: [] });
  const [toastState, setToastState] = useState({ show: false, msg: "", type: "" });
  const [theme, setTheme] = useState(getSavedThemeMode);
  const [newTagName, setNewTagName] = useState("");
  const [newTagColor, setNewTagColor] = useState("#58a6ff");
  const [settingsTab, setSettingsTab] = useState("backup");
  const [loadError, setLoadError] = useState("");

  const [sysTheme, setSysTheme] = useState(getSystemTheme);
  const searchRef = useRef(null);
  const searchTimerRef = useRef(null);
  const toastTimerRef = useRef(null);
  const mountedRef = useRef(false);
  const loadRequestRef = useRef(0);
  if (!toastTimerRef.current) {
    toastTimerRef.current = createResettableTimer();
  }

  const resolved = theme === "system" ? sysTheme : theme;

  // 搜索防抖：延迟 200ms 后再应用过滤
  const handleSearchInput = useCallback((value) => {
    setSearchInput(value);
    clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => setSearch(value), 200);
  }, []);

  // 快捷键：Alt+1~5 切换视图
  useEffect(() => {
    const viewKeys = ["skills", "ai", "scenarios", "updates", "settings"];
    const handler = (e) => {
      if (e.altKey && e.key >= "1" && e.key <= "5") {
        e.preventDefault();
        setView(viewKeys[parseInt(e.key) - 1]);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  // 初始加载需要在 StrictMode 下保持幂等，并避免卸载后继续写状态。
  useEffect(() => {
    mountedRef.current = true;
    loadData({ resetUpdates: false });
    return () => {
      mountedRef.current = false;
      loadRequestRef.current += 1;
      clearTimeout(searchTimerRef.current);
      toastTimerRef.current?.dispose();
    };
  }, []);

  // Custom confirm dialog helper
  function confirmAction(title, message, onConfirm, options = {}) {
    setConfirmState({
      open: true,
      title,
      message,
      confirmLabel: options.confirmLabel || "确认",
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
  useEffect(() => { localStorage.setItem(THEME_STORAGE_KEY, theme); }, [theme]);
  useEffect(() => { document.documentElement.setAttribute("data-theme", resolved); }, [resolved]);
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
  function cycleTheme() {
    setTheme(prev => THEME_MODES[(THEME_MODES.indexOf(prev) + 1) % THEME_MODES.length]);
  }

  const showToast = useCallback((msg, type = "") => {
    setToastState({ show: true, msg, type });
    toastTimerRef.current.start(3000, () => {
      if (mountedRef.current) {
        setToastState({ show: false, msg: "", type: "" });
      }
    });
  }, []);

  async function loadData(options = {}) {
    const resetUpdates = options?.resetUpdates ?? true;
    const requestId = ++loadRequestRef.current;
    setLoading(true);
    try {
      const { assistants, tags, scenarios, git } = await skillmateApi.inventory.loadDashboard();
      if (!mountedRef.current || requestId !== loadRequestRef.current) return;
      setData({ assistants, tags, scenarios, git });
      setTags((current) => tags.map((tag) => ({
        ...tag,
        selected: current.some((item) => item.id === tag.id && item.selected),
      })));
      setLoadError("");
      if (resetUpdates) resetUpdateState();
      gitBackupFlow.hydrate(git);
    } catch (e) {
      if (mountedRef.current && requestId === loadRequestRef.current) {
        setLoadError(String(e));
        showToast(`加载失败: ${e}`, "error");
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
    return buildUniqueSkillInventory(data.assistants);
  }, [data.assistants]);

  const scenarioFlow = useScenarioFlow({
    scenarios: data.scenarios,
    allSkills,
    selectableSkills: allSkills,
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


  async function addTag() {
    if (!newTagName.trim()) { showToast("请输入标签名", "error"); return; }
    try {
      const tag = await skillmateApi.tags.add(newTagName.trim(), newTagColor);
      setTags(prev => [...prev, { ...tag, selected: false }]);
      setNewTagName("");
      setNewTagColor("#58a6ff");
      showToast("标签已添加", "success");
    } catch (e) { showToast(`添加标签失败: ${e}`, "error"); }
  }

  function openTagEditor(skill) {
    setTagEditor({ open: true, skill, selected: [...skill.tags] });
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
    if (!tagEditor.skill) return;
    try {
      await skillmateApi.tags.updateSkill(tagEditor.skill.path, tagEditor.selected);
      showToast("标签已更新", "success");
      setTagEditor({ open: false, skill: null, selected: [] });
      await loadData();
    } catch (e) {
      showToast(`标签更新失败: ${e}`, "error");
    }
  }

  const statAI = data.assistants.filter(a => a.exists).length;
  const statSkills = allSkills.length;
  const updateBadge = allSkills.reduce((count, skill) => {
    const state = updateState[skill.path]?.syncState || skill.sync_state;
    return count + (state === "behind" ? 1 : 0);
  }, 0);

  async function openPreview(path) {
    try {
      const { content, validation } = await skillmateApi.inventory.readSkill(path);
      setPreview({ title: path.split(/[\/]/).pop(), content: content || "无内容", validation });
      setPreviewOpen(true);
    } catch (e) { showToast(`预览失败: ${e}`, "error"); }
  }

  async function remove(path, name, availableIn = []) {
    const sharedWarning = availableIn.length > 1
      ? `该目录同时被 ${availableIn.map((assistant) => assistant.name).join("、")} 使用，删除后这些助手都会失去该 Skill。`
      : "";
    confirmAction("删除 Skill", `确定要删除「${name}」吗？${sharedWarning}此操作不可恢复。`, async () => {
      setLoading(true);
      try {
        await skillmateApi.inventory.deleteSkill(path);
        showToast("已删除", "success");
        await loadData();
      } catch (e) { showToast(`删除失败: ${e}`, "error"); }
      finally { setLoading(false); }
    }, { confirmLabel: "删除 Skill" });
  }

  async function unlinkSymlink(path, name) {
    confirmAction("解除软连接", `确定要解除「${name}」的项目软连接吗？源目录不会被删除。`, async () => {
      setLoading(true);
      try {
        const r = await skillmateApi.inventory.unlinkSkill(path);
        showToast(String(r), "success");
        await loadData();
      } catch (e) { showToast(`解除失败: ${e}`, "error"); }
      finally { setLoading(false); }
    }, { confirmLabel: "解除软连接", tone: "primary" });
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
    () => buildAppUpdateView(appUpdateState),
    [appUpdateState]
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
    try { await skillmateApi.inventory.openFolder(getDir(path)); } catch (e) { showToast(`打开失败: ${e}`, "error"); }
  }

  return (
    <div className="app">
      {loading && <Loader />}

      <header className="header">
        <div className="header-left">
          <Logo />
          <div>
            <h1 className="app-name">SkillMate</h1>
            <p className="app-sub">{VIEWS[view].title}</p>
          </div>
        </div>

        <div className="header-center">
          {(view === "skills" || view === "updates") && (
            <div className="search-box">
              <Icon name="search" size={16} />
              <input ref={searchRef} type="text" aria-label={view === "updates" ? "搜索更新" : "搜索 Skills"} placeholder={view === "updates" ? "搜索更新..." : `搜索 Skills... (${typeof navigator !== "undefined" && navigator.platform?.startsWith("Mac") ? "⌘K" : "Ctrl+K"})`} value={searchInput} onChange={e => handleSearchInput(e.target.value)} />
              {search && <button className="search-x" aria-label="清除搜索" onClick={() => { setSearchInput(""); setSearch(""); }}><Icon name="x" size={14} /></button>}
            </div>
          )}
        </div>

        <div className="header-right">
          {view === "skills" && (
            <div className="sort-tabs">
              <button className={`sort-tab ${sort === "name" ? "active" : ""}`} onClick={() => setSort("name")}><Icon name="tag" size={14} />名称</button>
              <button className={`sort-tab ${sort === "date" ? "active" : ""}`} onClick={() => setSort("date")}><Icon name="clock" size={14} />时间</button>
            </div>
          )}
          <button className="btn btn-ghost" onClick={loadData} title="刷新" aria-label="刷新数据"><Icon name="refresh" size={18} className={loading ? "spin" : ""} /></button>
          <button className="btn btn-primary" onClick={() => setInstallOpen(true)}><Icon name="plus" size={18} /><span>安装</span></button>
          <button className="btn btn-ghost" onClick={cycleTheme} title="切换主题" aria-label="切换主题">
            <Icon name={theme === "system" ? "monitor" : theme === "light" ? "sun" : "moon"} size={18} />
          </button>
        </div>
      </header>

      <div className="layout">
        <nav className="sidebar">
          <div className="nav-items">
            {Object.entries(VIEWS).map(([k, v]) => (
              <button key={k} className={`nav-item ${view === k ? "active" : ""}`} onClick={() => setView(k)}>
                <Icon name={v.icon} size={18} />
                <span>{v.title}</span>
                {k === "skills" && statSkills > 0 && <span className="badge">{statSkills}</span>}
                {k === "ai" && <span className="badge">{statAI}</span>}
                {k === "updates" && updateBadge > 0 && <span className="badge warn">{updateBadge}</span>}
              </button>
            ))}
          </div>

          <div className="sidebar-section">
            <div className="section-header">
              <span>标签</span>
              {selectedTags.length > 0 && <button onClick={() => setTags(t => t.map(tag => ({ ...tag, selected: false })))}>清除</button>}
            </div>
            <div className="tag-list">
              {tags.map(tag => (
                <button key={tag.id} className={`tag-chip ${tag.selected ? "active" : ""}`} style={{ "--c": tag.color }} onClick={() => toggleTag(tag.id)}>
                  <span className="tag-dot" />{tag.name}
                </button>
              ))}
              {tags.length === 0 && <p className="empty-hint">暂无标签</p>}
            </div>
          </div>

          <div className="sidebar-footer">
            <div className="mini-stats">
              <div><span className="val">{statSkills}</span><span className="lbl">Skills</span></div>
              <div><span className="val">{statAI}</span><span className="lbl">AI</span></div>
              <div><span className="val">{tags.length}</span><span className="lbl">标签</span></div>
            </div>
          </div>
        </nav>

        <main className="content">
          {loadError && (
            <div className="load-error-banner" role="alert">
              <div><strong>数据加载失败</strong><span>{loadError}</span></div>
              <button className="btn btn-secondary btn-sm" onClick={() => loadData({ resetUpdates: false })}>重试</button>
            </div>
          )}
          {activeScenario && (
            <div className="settings-card" style={{ marginBottom: 16 }}>
              <div className="settings-body" style={{ padding: 14 }}>
                <div className="card-actions" style={{ justifyContent: "space-between" }}>
                  <div>
                    <strong>按场景筛选：{activeScenario.name}</strong>
                    <div className="git-meta">仅显示该场景中的 {activeScenario.skill_ids.length} 个 Skill</div>
                  </div>
                  <button className="btn btn-secondary btn-sm" onClick={() => scenarioFlow.setActiveId("")}>
                    <Icon name="x" size={14} />清除场景筛选
                  </button>
                </div>
              </div>
            </div>
          )}
          {init ? <Skeleton /> : view === "skills" && (
            <SkillsView
              skills={skills}
              allSkillCount={allSkills.length}
              selectedTagCount={selectedTags.length}
              tags={tags}
              onInstall={() => setInstallOpen(true)}
              onClearFilters={() => {
                setSearchInput("");
                setSearch("");
                setTags(current => current.map(tag => ({ ...tag, selected: false })));
                scenarioFlow.setActiveId("");
              }}
              onEditTags={openTagEditor}
              onOpenDirectory={openDir}
              onPreview={openPreview}
              onUnlink={unlinkSymlink}
              onRemove={remove}
            />
          )}

          {view === "ai" && (
            <AssistantsView assistants={data.assistants} installedCount={statAI} />
          )}

          {view === "scenarios" && (
            <ScenarioView scenarios={data.scenarios} skills={skills} flow={scenarioFlow} />
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
        <PreviewModal preview={preview} onClose={() => setPreviewOpen(false)} />
      )}

      {tagEditor.open && (
        <TagEditorModal
          tagEditor={tagEditor}
          tags={tags}
          toggleSkillTag={toggleSkillTag}
          saveSkillTags={saveSkillTags}
          onClose={() => setTagEditor({ open: false, skill: null, selected: [] })}
        />
      )}

      <div className={`toast ${toastState.show ? "show" : ""} ${toastState.type}`} role="status" aria-live="polite" aria-atomic="true">{toastState.show ? toastState.msg : ""}</div>

      {confirmState.open && (
        <ConfirmModal
          confirmState={confirmState}
          onClose={() => setConfirmState({ open: false, title: "", message: "", confirmLabel: "确认", tone: "danger", onConfirm: null })}
          onConfirm={() => {
            const cb = confirmState.onConfirm;
            setConfirmState({ open: false, title: "", message: "", confirmLabel: "确认", tone: "danger", onConfirm: null });
            cb?.();
          }}
        />
      )}
    </div>
  );
}

export default App;
