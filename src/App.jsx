import React, { useEffect, useMemo, useState, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import claudeLogo from "./assets/brands/claude.svg";
import codexLogo from "./assets/brands/codex-openai.svg";
import openclawLogo from "./assets/brands/openclaw.svg";
import windsurfLogo from "./assets/brands/windsurf.svg";
import rooCodeLogo from "./assets/brands/roo-code.svg";
import geminiLogo from "./assets/brands/gemini.svg";
import zedLogo from "./assets/brands/zed.png";
import cursorLogo from "./assets/brands/cursor.png";
import vscodeLogo from "./assets/brands/vscode.svg";
import {
  buildAppUpdateView,
  buildImportPreviewSummary,
  buildGitBackupPayload,
  buildValidationSummary,
  buildSkillCardView,
  buildScenarioManifestPreviewSummary,
  buildSkillMateManifestPreviewSummary,
  buildSkillProfilePreviewSummary,
  buildProjectTargetPreviewSummary,
  buildInstallPreviewSummary,
  buildStructureWarningSummary,
  filterSkillsByScenario,
  formatScenarioCopyText,
  getStructureStatusLabel,
  getStructureStatusTone,
  normalizeScenarioSkillPaths,
  resolveScenarioSkills,
  SUPPORTED_INSTALL_SOURCES,
} from "./lib/skillmate.mjs";
import { useAppUpdateFlow, useImportExportFlow, useInstallFlow, useUpdateFlow } from "./lib/skillmateFlows.js";

const EMPTY_DATA = { assistants: [], tags: [], scenarios: [], git: { enabled: false, remote_url: "" } };
const THEME_STORAGE_KEY = "skillmate-theme-mode";
const THEME_MODES = ["system", "light", "dark"];

const AI_META = {
  "Claude Code": { bg: "#f7f3ee", src: claudeLogo, mode: "contain" },
  "Codex": { bg: "#ffffff", src: codexLogo, mode: "contain" },
  "OpenClaw": { bg: "#08111f", src: openclawLogo, mode: "contain" },
  "Windsurf": { bg: "#ffffff", src: windsurfLogo, mode: "contain" },
  "Roo Code": { bg: "#0f172a", src: rooCodeLogo, mode: "contain-wide" },
  "Gemini CLI": { bg: "#ffffff", src: geminiLogo, mode: "contain" },
  "Zed": { bg: "#ffffff", src: zedLogo, mode: "cover" },
  "Cursor": { bg: "#ffffff", src: cursorLogo, mode: "cover" },
  "VSCode": { bg: "#ffffff", src: vscodeLogo, mode: "contain" }
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

const AiAvatar = React.memo(function AiAvatar({ name, size = 36 }) {
  const m = AI_META[name] || { bg: "#eff6ff" };
  return (
    <div
      className="ai-avatar"
      style={{
        width: size,
        height: size,
        minWidth: size,
        minHeight: size,
        borderRadius: Math.max(8, Math.round(size * 0.24)),
        background: m.bg
      }}
      title={name}
      aria-label={name}
    >
      {m.src ? (
        <img
          className={`ai-avatar-img ${m.mode || "contain"}`}
          src={m.src}
          alt={name}
          loading="lazy"
          draggable="false"
        />
      ) : (
        <span style={{ fontSize: Math.max(10, size * 0.34), fontWeight: 700 }}>{name.slice(0, 1)}</span>
      )}
    </div>
  );
});

const Icon = React.memo(function Icon({ name, size = 18 }) {
  const paths = {
    refresh: <><path d="M21 12a9 9 0 1 1-2.64-6.36" /><path d="M21 4v5h-5" /></>,
    plus: <><path d="M12 5v14" /><path d="M5 12h14" /></>,
    skills: <><rect x="4" y="5" width="16" height="14" rx="2" /><path d="M8 9h8" /><path d="M8 13h5" /></>,
    assistants: <><circle cx="12" cy="8" r="3" /><path d="M5 19a7 7 0 0 1 14 0" /></>,
    scenarios: <><path d="M3 7h7l2 2h9v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></>,
    updates: <><path d="M21 12a9 9 0 1 1-2.64-6.36" /><path d="M21 3v6h-6" /></>,
    settings: <><circle cx="12" cy="12" r="3" /><path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42" /></>,
    folder: <><path d="M3 7h7l2 2h9v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></>,
    preview: <><path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6z" /><circle cx="12" cy="12" r="2.5" /></>,
    trash: <><path d="M4 7h16" /><path d="M9 7V5h6v2" /><path d="M7 7l1 12h8l1-12" /></>,
    check: <><path d="M20 6L9 17l-5-5" /></>,
    lock: <><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></>,
    sun: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41" /></>,
    moon: <><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 1 0 9.8 9.8z" /></>,
    monitor: <><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" /></>,
    search: <><circle cx="11" cy="11" r="7" /><path d="M21 21l-4.35-4.35" /></>,
    tag: <><path d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z" /><circle cx="7" cy="7" r="1" /></>,
    x: <><path d="M18 6L6 18M6 6l12 12" /></>,
    upload: <><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="17 8 12 3 7 8" /><line x1="12" y1="3" x2="12" y2="15" /></>,
    clock: <><circle cx="12" cy="12" r="10" /><polyline points="12 6 12 12 16 14" /></>,
    box: <><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" /><polyline points="3.27 6.96 12 12.01 20.73 6.96" /><line x1="12" y1="22.08" x2="12" y2="12" /></>,
    sparkles: <><path d="M12 3l1.5 4.5L18 9l-4.5 1.5L12 15l-1.5-4.5L6 9l4.5-1.5L12 3z" /><path d="M5 19l.5 1.5L7 21l-1.5.5L5 23l-.5-1.5L3 21l1.5-.5L5 19z" /><path d="M19 13l.5 1.5L21 15l-1.5.5L19 17l-.5-1.5L17 15l1.5-.5L19 13z" /></>
  };
  return (
    <svg className="icon" style={{ width: size, height: size }} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      {paths[name]}
    </svg>
  );
});

function getDir(path) {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(0, i) : path;
}

function getRemoteLabel(url) {
  if (!url) return "未配置远端";
  const ssh = url.match(/^[^@]+@([^:]+):(.+)$/);
  if (ssh) return `${ssh[1]}/${ssh[2].replace(/\.git$/, "")}`;
  try {
    const parsed = new URL(url);
    return `${parsed.host}${parsed.pathname.replace(/\.git$/, "")}`;
  } catch {
    return url.replace(/\.git$/, "");
  }
}

function formatRefLabel(value) {
  if (!value) return "—";
  return /^[0-9a-f]{12,}$/i.test(value) ? value.slice(0, 7) : value;
}

function formatProbeTime(value) {
  if (!value) return "从未";
  const date = new Date(Number(value));
  if (Number.isNaN(date.getTime())) return "从未";
  return date.toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });
}

function getOriginKindLabel(kind) {
  switch (kind) {
    case "git": return "Git";
    case "legacy_npm":
    case "npm": return "历史 npm";
    case "legacy_pip":
    case "pip": return "历史 PyPI";
    case "local": return "本地";
    default: return "未托管";
  }
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

function getStateText(state) {
  switch (state) {
    case "behind": return "可更新";
    case "current": return "已是最新";
    case "failed": return "检查失败";
    case "diverged": return "存在分叉";
    case "ahead_local": return "本地领先";
    case "local_fixed": return "本地固定";
    case "source_missing": return "来源缺失";
    case "unsupported": return "暂不支持";
    default: return "待检查";
  }
}

function getStateTone(state) {
  switch (state) {
    case "behind": return "warn";
    case "failed":
    case "source_missing": return "error";
    case "current": return "success";
    default: return "muted";
  }
}

function getLagText(info) {
  if (info.originKind === "git") return `${info.lagCount || 0} 个提交`;
  if (["legacy_npm", "legacy_pip", "npm", "pip"].includes(info.originKind)) {
    if (info.syncState === "behind") return "有新版本";
    if (info.syncState === "current") return "已最新";
    return "—";
  }
  return "—";
}

function getUpdateButtonText(info) {
  if (info.updating) {
    if (info.originKind === "git") return "更新中";
    return "同步中";
  }
  if (info.originKind === "git") return "一键更新";
  return "一键同步";
}

function App() {
  const [data, setData] = useState(EMPTY_DATA);
  const [view, setView] = useState("skills");
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [tags, setTags] = useState([]);
  const [confirmState, setConfirmState] = useState({ open: false, title: "", message: "", onConfirm: null });
  const [sort, setSort] = useState("name");
  const [loading, setLoading] = useState(false);
  const [init, setInit] = useState(true);
  const [installOpen, setInstallOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [preview, setPreview] = useState({ title: "", content: "", validation: null });
  const [tagEditor, setTagEditor] = useState({ open: false, skill: null, selected: [] });
  const [toastState, setToastState] = useState({ show: false, msg: "", type: "" });
  const [theme, setTheme] = useState(getSavedThemeMode);
  const [newScenarioName, setNewScenarioName] = useState("");
  const [newScenarioDesc, setNewScenarioDesc] = useState("");
  const [scenarioSkillPaths, setScenarioSkillPaths] = useState([]);
  const [gitRepoPath, setGitRepoPath] = useState("");
  const [gitBranch, setGitBranch] = useState("main");
  const [gitRemoteUrl, setGitRemoteUrl] = useState("");
  const [newTagName, setNewTagName] = useState("");
  const [newTagColor, setNewTagColor] = useState("#58a6ff");
  const [newScenarioSkillInput, setNewScenarioSkillInput] = useState("");
  const [expandedScenarioId, setExpandedScenarioId] = useState("");
  const [activeScenarioId, setActiveScenarioId] = useState("");
  const [settingsTab, setSettingsTab] = useState("backup");

  const [sysTheme, setSysTheme] = useState(getSystemTheme);
  const searchRef = useRef(null);
  const searchTimerRef = useRef(null);

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

  // Cleanup debounce timer on unmount
  useEffect(() => {
    return () => clearTimeout(searchTimerRef.current);
  }, []);

  // Custom confirm dialog helper
  function confirmAction(title, message, onConfirm) {
    setConfirmState({ open: true, title, message, onConfirm });
  }

  useEffect(() => { loadData(); }, []);
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
    const handler = (e) => { if ((e.metaKey || e.ctrlKey) && e.key === "k") { e.preventDefault(); searchRef.current?.focus(); } };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);
  function cycleTheme() {
    setTheme(prev => THEME_MODES[(THEME_MODES.indexOf(prev) + 1) % THEME_MODES.length]);
  }

  function showToast(msg, type = "") {
    setToastState({ show: true, msg, type });
    setTimeout(() => setToastState(t => ({ ...t, show: false })), 3000);
  }

  async function loadData() {
    setLoading(true);
    try {
      const [assistants, tags, scenarios, git] = await Promise.all([
        invoke("get_all_assistants"), invoke("get_all_tags"),
        invoke("get_scenarios"), invoke("get_git_backup")
      ]);
      setData({ assistants, tags, scenarios, git });
      setTags(tags);
      resetUpdateState();
      setGitRepoPath(git.repo_path || "");
      setGitRemoteUrl(git.remote_url || "");
      setGitBranch(git.branch || "main");
    } catch (e) { showToast(`加载失败: ${e}`, "error"); }
    finally { setLoading(false); setInit(false); }
  }

  const selectedTags = tags.filter(t => t.selected).map(t => t.id);

  const allSkills = useMemo(() => {
    let list = [];
    data.assistants.forEach(a => a.skills.forEach(s => list.push({ ...s, ai: a.name })));
    return list;
  }, [data.assistants]);

  const activeScenario = useMemo(
    () => data.scenarios.find((scenario) => scenario.id === activeScenarioId) || null,
    [data.scenarios, activeScenarioId]
  );

  const skills = useMemo(() => {
    let list = [...allSkills];
    if (search) list = list.filter(s => s.name.toLowerCase().includes(search.toLowerCase()));
    if (selectedTags.length > 0) list = list.filter(s => selectedTags.some(t => s.tags.includes(t)));
    list = filterSkillsByScenario({ skills: list, activeScenarioPaths: activeScenario?.skill_ids || [] });
    list.sort((a, b) => sort === "date" ? Number(b.modified||0) - Number(a.modified||0) : a.name.localeCompare(b.name, "zh-CN"));
    return list;
  }, [activeScenario, allSkills, search, selectedTags, sort]);

  const updateable = useMemo(() => {
    let list = [...allSkills];
    if (search) list = list.filter(s => s.name.toLowerCase().includes(search.toLowerCase()));
    if (selectedTags.length > 0) list = list.filter(s => selectedTags.some(t => s.tags.includes(t)));
    list = filterSkillsByScenario({ skills: list, activeScenarioPaths: activeScenario?.skill_ids || [] });
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
  } = useUpdateFlow({ updateable, showToast, loadData });

  const {
    appUpdateState,
    checkAppUpdate,
    installAppUpdate,
    restartApp,
  } = useAppUpdateFlow({ showToast });

  const {
    src,
    setSrc,
    pkg,
    setPkg,
    installDetectionView,
    installStructurePreview,
    installPreviewView,
    previewingInstall,
    installAssistant,
    setInstallAssistant,
    installMode,
    setInstallMode,
    projectPath,
    setProjectPath,
    projectTargetPreview,
    previewingProjectTargets,
    installPreviewCurrent,
    installPrimaryAction,
    runInstallPrimaryAction,
    showProjectLinkOption,
    installDetailsOpen,
    setInstallDetailsOpen,
    installAdvancedOpen,
    setInstallAdvancedOpen,
    showInstallAdvancedOptions,
    cmd,
  } = useInstallFlow({
    installOpen,
    assistants: data.assistants,
    setInstallOpen,
    showToast,
    loadData,
    setLoading,
  });

  const {
    exportPath,
    setExportPath,
    importPath,
    importMode,
    importPreview,
    previewingImport,
    importPreviewCurrent,
    scenarioManifestPath,
    scenarioManifestMode,
    scenarioManifestPreview,
    previewingScenarioManifest,
    scenarioManifestPreviewCurrent,
    skillMateManifestPath,
    skillMateManifestPreview,
    previewingSkillMateManifest,
    skillMateManifestPreviewCurrent,
    skillProfiles,
    skillProfileName,
    skillProfileDescription,
    skillProfilePreview,
    previewingSkillProfile,
    applyingSkillProfile,
    updateImportPath,
    updateImportMode,
    updateScenarioManifestPath,
    updateScenarioManifestMode,
    updateSkillMateManifestPath,
    setSkillProfileName,
    setSkillProfileDescription,
    exportLibraryFile,
    previewImportLibraryFile,
    importLibraryFile,
    exportSkillMateManifestFile,
    previewSkillMateManifestFile,
    applySkillMateManifestFile,
    saveCurrentSkillProfile,
    previewSkillProfile,
    applySkillProfile,
    rollbackSkillProfile,
    exportScenarioManifestFile,
    previewImportScenarioManifestFile,
    importScenarioManifestFile,
  } = useImportExportFlow({ showToast, loadData });

  function toggleTag(id) {
    setTags(prev => prev.map(t => t.id === id ? { ...t, selected: !t.selected } : t));
  }


  async function addTag() {
    if (!newTagName.trim()) { showToast("请输入标签名", "error"); return; }
    try {
      const tag = await invoke("add_tag", { name: newTagName.trim(), color: newTagColor });
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
      await invoke("update_skill_tags", {
        skillPath: tagEditor.skill.path,
        tags: tagEditor.selected,
      });
      showToast("标签已更新", "success");
      setTagEditor({ open: false, skill: null, selected: [] });
      await loadData();
    } catch (e) {
      showToast(`标签更新失败: ${e}`, "error");
    }
  }

  async function saveGitBackup() {
    const payload = buildGitBackupPayload({
      repoPath: gitRepoPath,
      remoteUrl: gitRemoteUrl,
      branch: gitBranch,
    });
    if (!payload.repoPath) {
      showToast("请输入备份仓库路径", "error");
      return;
    }
    try {
      await invoke("setup_git_backup", payload);
      showToast("Git 备份已保存", "success");
      await loadData();
    } catch (e) { showToast(`保存失败: ${e}`, "error"); }
  }

  async function deleteScenario(id) {
    try {
      await invoke("delete_scenario", { scenarioId: id });
      if (activeScenarioId === id) {
        setActiveScenarioId("");
      }
      showToast("场景已删除", "success");
      await loadData();
    } catch (e) { showToast(`删除失败: ${e}`, "error"); }
  }
  const statAI = data.assistants.filter(a => a.exists).length;
  const statSkills = data.assistants.reduce((s, a) => s + a.skills.length, 0);
  const updateBadge = allSkills.reduce((count, skill) => {
    const state = updateState[skill.path]?.syncState || skill.sync_state;
    return count + (state === "behind" ? 1 : 0);
  }, 0);

  async function openPreview(path) {
    try {
      const [c, validation] = await Promise.all([
        invoke("get_skill_readme", { path }),
        invoke("inspect_skill_validation", { path }),
      ]);
      setPreview({ title: path.split(/[\/]/).pop(), content: c || "无内容", validation });
      setPreviewOpen(true);
    } catch (e) { showToast(`预览失败: ${e}`, "error"); }
  }

  async function remove(path, name) {
    confirmAction("删除确认", `确定要删除「${name}」吗？此操作不可恢复。`, async () => {
      setLoading(true);
      try {
        const r = await invoke("delete_skill", { path });
        showToast(String(r).includes("已删除") ? "已删除" : "删除失败", String(r).includes("已删除") ? "success" : "error");
        await loadData();
      } catch (e) { showToast(`删除失败: ${e}`, "error"); }
      finally { setLoading(false); }
    });
  }

  async function unlinkSymlink(path, name) {
    confirmAction("解除软连接", `确定要解除「${name}」的项目软连接吗？源目录不会被删除。`, async () => {
      setLoading(true);
      try {
        const r = await invoke("unlink_symlink_skill", { path });
        showToast(String(r), "success");
        await loadData();
      } catch (e) { showToast(`解除失败: ${e}`, "error"); }
      finally { setLoading(false); }
    });
  }

  const orderedUpdateable = [...updateable].sort((a, b) => {
    const aInfo = getSyncInfo(a);
    const bInfo = getSyncInfo(b);
    const priority = getStatePriority(aInfo.syncState) - getStatePriority(bInfo.syncState);
    if (priority !== 0) return priority;
    return a.name.localeCompare(b.name, "zh-CN");
  });

  const updateStats = useMemo(() => {
    let behind = 0;
    let syncable = 0;
    let failed = 0;
    orderedUpdateable.forEach((skill) => {
      const info = getSyncInfo(skill);
      if (info.syncState === "behind") behind += 1;
      if (info.canSync) syncable += 1;
      if (info.syncState === "failed") failed += 1;
    });
    return { behind, syncable, failed };
  }, [orderedUpdateable, updateState]);

  const appUpdateView = useMemo(
    () => buildAppUpdateView(appUpdateState),
    [appUpdateState]
  );

  const scenarioDetails = useMemo(() => (
    data.scenarios.reduce((acc, scenario) => {
      acc[scenario.id] = resolveScenarioSkills({ scenario, allSkills });
      return acc;
    }, {})
  ), [data.scenarios, allSkills]);

  async function openDir(path) {
    try { await invoke("open_folder", { path: getDir(path) }); } catch (e) { showToast(`打开失败: ${e}`, "error"); }
  }

  async function copyScenarioPaths(paths) {
    const text = formatScenarioCopyText(paths);
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
      } else {
        const textarea = document.createElement("textarea");
        textarea.value = text;
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand("copy");
        textarea.remove();
      }
      showToast("路径已复制", "success");
    } catch (e) {
      showToast(`复制失败: ${e}`, "error");
    }
  }

  function loadScenarioIntoEditor(scenario) {
    setNewScenarioName(`${scenario.name} 副本`);
    setNewScenarioDesc(scenario.description || "");
    setScenarioSkillPaths([...scenario.skill_ids]);
    setNewScenarioSkillInput("");
    showToast("已回填到场景编辑器", "success");
  }

  function applyScenario(scenario) {
    setActiveScenarioId(scenario.id);
    setView("skills");
    showToast(`已应用场景：${scenario.name}`, "success");
  }

  function toggleScenarioPath(path) {
    setScenarioSkillPaths((current) => (
      current.includes(path)
        ? current.filter((item) => item !== path)
        : [...current, path]
    ));
  }

  async function createScenario() {
    const skillPaths = normalizeScenarioSkillPaths({
      selectedPaths: scenarioSkillPaths,
      manualInput: newScenarioSkillInput,
      skills,
    });
    try {
      await invoke("create_scenario", {
        name: newScenarioName || `场景 ${new Date().toLocaleDateString()}`,
        description: newScenarioDesc || "自动生成场景",
        skillIds: skillPaths,
      });
      showToast("场景已创建", "success");
      setNewScenarioName("");
      setNewScenarioDesc("");
      setScenarioSkillPaths([]);
      setNewScenarioSkillInput("");
      await loadData();
      setView("scenarios");
    } catch (e) { showToast(`创建失败: ${e}`, "error"); }
  }

  async function syncGit() {
    try {
      const r = await invoke("sync_to_git", { message: `SkillMate sync ${new Date().toISOString()}` });
      showToast(String(r), "success");
      await loadData();
    } catch (e) { showToast(`同步失败: ${e}`, "error"); }
  }

  const VIEWS = {
    skills: { title: "Skills", icon: "skills" },
    ai: { title: "AI 助手", icon: "assistants" },
    scenarios: { title: "场景", icon: "scenarios" },
    updates: { title: "更新", icon: "updates" },
    settings: { title: "设置", icon: "settings" }
  };

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
              <input ref={searchRef} type="text" placeholder={view === "updates" ? "搜索更新..." : "搜索 Skills... (⌘K)"} value={searchInput} onChange={e => handleSearchInput(e.target.value)} />
              {search && <button className="search-x" onClick={() => { setSearchInput(""); setSearch(""); }}><Icon name="x" size={14} /></button>}
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
          <button className="btn btn-ghost" onClick={loadData} title="刷新"><Icon name="refresh" size={18} className={loading ? "spin" : ""} /></button>
          <button className="btn btn-primary" onClick={() => setInstallOpen(true)}><Icon name="plus" size={18} /><span>安装</span></button>
          <button className="btn btn-ghost" onClick={cycleTheme} title="切换主题">
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
          {activeScenario && (
            <div className="settings-card" style={{ marginBottom: 16 }}>
              <div className="settings-body" style={{ padding: 14 }}>
                <div className="card-actions" style={{ opacity: 1, justifyContent: "space-between" }}>
                  <div>
                    <strong>当前场景：{activeScenario.name}</strong>
                    <div className="git-meta">{activeScenario.skill_ids.length} 个 Skill 正在生效</div>
                  </div>
                  <button className="btn btn-secondary btn-sm" onClick={() => setActiveScenarioId("")}>
                    <Icon name="x" size={14} />清除场景
                  </button>
                </div>
              </div>
            </div>
          )}
          {init ? <Skeleton /> : view === "skills" && (
            <>
              <div className="content-head">
                <div><h2>所有 Skills</h2><span className="count">{skills.length} 个{skills.length !== allSkills.length ? ` / ${allSkills.length} 总计` : ''}</span></div>
                <div className="content-head-actions">
                  {selectedTags.length > 0 && <div className="filter-tag"><Icon name="tag" size={14} />已选 {selectedTags.length} 个标签</div>}
                  <button className="btn btn-primary btn-sm" onClick={() => setInstallOpen(true)}><Icon name="plus" size={14} />安装</button>
                </div>
              </div>
              {skills.length === 0 ? (
                <div className="empty-state">
                  <div className="empty-icon"><Icon name="box" size={48} /></div>
                  <h3>{allSkills.length > 0 ? "没有匹配的 Skills" : "暂无 Skills"}</h3>
                  <p>{allSkills.length > 0 ? "清除搜索或标签筛选后继续查看已有 Skills" : "从 Git 仓库或本地目录添加第一个 Skill"}</p>
                  <div className="empty-actions">
                    {allSkills.length > 0 && (
                      <button className="btn btn-secondary" onClick={() => { setSearchInput(""); setSearch(""); setTags(t => t.map(tag => ({ ...tag, selected: false }))); }}>
                        <Icon name="x" size={16} />清除筛选
                      </button>
                    )}
                    <button className="btn btn-primary" onClick={() => setInstallOpen(true)}><Icon name="plus" size={16} />安装 Skill</button>
                  </div>
                </div>
              ) : (
                <div className="grid">
                  {skills.map((s, i) => {
                    const skillCard = buildSkillCardView(s);
                    return (
                    <div className="card" key={`${s.path}-${s.name}`} style={{ "--i": i }}>
                      <div className="card-head">
                        <AiAvatar name={s.ai} size={40} />
                        <div className="card-info">
                          <div className="card-title-row">
                            <h3>{skillCard.title}</h3>
                            {skillCard.sourceLabel && <span className={`source-badge ${s.source_type || skillCard.sourceLabel.toLowerCase()}`}>{skillCard.sourceLabel}</span>}
                          </div>
                          <div className="card-tags">
                            <span
                              className={`structure-badge ${skillCard.structureTone}`}
                              title={skillCard.warningSummary}
                            >
                              {skillCard.structureLabel}
                            </span>
                            {s.tags.slice(0, 2).map(tid => { const t = tags.find(x => x.id === tid); return t ? <span key={t.id} className="tag" style={{ background: `${t.color}20`, color: t.color }}>{t.name}</span> : null; })}
                            {s.tags.length > 2 && <span className="tag more">+{s.tags.length - 2}</span>}
                          </div>
                        </div>
                      </div>
                      {skillCard.description && <p className="card-desc">{skillCard.description}</p>}
                      <div className="card-meta">
                        <span><AiAvatar name={s.ai} size={14} />{s.ai}</span>
                        <span><Icon name="folder" size={12} />{s.size}</span>
                      </div>
                      <div className="card-path">{s.path.replace(/^\/Users\/[^/]+/, '~')}</div>
                      {s.symlink_source && <div className="git-meta">源：{s.symlink_source.replace(/^\/Users\/[^/]+/, '~')}</div>}
                      <div className="card-actions">
                        <button className="btn btn-ghost btn-sm" onClick={() => openTagEditor(s)} title="编辑标签"><Icon name="tag" size={16} /></button>
                        <button className="btn btn-ghost btn-sm" onClick={() => openDir(s.path)} title="打开文件夹"><Icon name="folder" size={16} /></button>
                        <button className="btn btn-ghost btn-sm" onClick={() => openPreview(s.path)} title="预览说明"><Icon name="preview" size={16} /></button>
                        {skillCard.canUnlink ? (
                          <button className="btn btn-ghost btn-sm danger" onClick={() => unlinkSymlink(s.path, s.name)} title="解除软连接"><Icon name="x" size={16} /></button>
                        ) : skillCard.canDelete ? (
                          <button className="btn btn-ghost btn-sm danger" onClick={() => remove(s.path, s.name)} title="删除"><Icon name="trash" size={16} /></button>
                        ) : null}
                      </div>
                    </div>
                  )})}
                </div>
              )}
            </>
          )}

          {view === "ai" && (
            <div>
              <div className="content-head"><div><h2>AI 助手</h2><span className="count">{statAI} / {data.assistants.length} 已安装</span></div></div>
              <div className="grid ai-grid">
                {data.assistants.map(a => (
                  <div className={`ai-card ${a.exists ? "ok" : "no-exist"}`} key={a.name}>
                    <AiAvatar name={a.name} size={48} />
                    <h3>{a.name}</h3>
                    <p className="ai-path">{a.path.replace(/^\/Users\/[^/]+/, '~')}</p>
                    <div className={`ai-status ${a.exists ? "ok" : "no"}`}>
                      <Icon name={a.exists ? "check" : "x"} size={14} />{a.exists ? "已安装" : "未安装"}
                    </div>
                    {a.exists && a.skills.length > 0 && (
                      <div className="ai-skill-tags">
                        {a.skills.slice(0, 3).map(sk => <span key={sk.name} className="ai-skill-tag">{sk.name}</span>)}
                        {a.skills.length > 3 && <span className="ai-skill-tag more">+{a.skills.length - 3}</span>}
                      </div>
                    )}
                    {a.exists && a.skills.length === 0 && <div className="ai-empty-hint">暂无 Skills</div>}
                  </div>
                ))}
              </div>
            </div>
          )}

          {view === "scenarios" && (
            <div>
              <div className="content-head"><div><h2>场景</h2><span className="count">{data.scenarios.length}</span></div></div>
              <div className="settings-card" style={{ marginBottom: 20 }}>
                <div className="settings-head"><Icon name="scenarios" size={20} /><h3>场景编辑器</h3></div>
                <div className="settings-body">
                  <div className="form"><label>场景名称</label><input value={newScenarioName} onChange={e => setNewScenarioName(e.target.value)} placeholder="例如：写作模式" /></div>
                  <div className="form"><label>场景描述</label><input value={newScenarioDesc} onChange={e => setNewScenarioDesc(e.target.value)} placeholder="这个场景适合处理什么任务" /></div>
                  <div className="form">
                    <label>手动路径输入</label>
                    <textarea value={newScenarioSkillInput} onChange={e => setNewScenarioSkillInput(e.target.value)} placeholder="可粘贴多个 Skill 路径，使用空格、换行或逗号分隔" />
                  </div>
                  <div className="form">
                    <label>从当前 Skills 选择</label>
                    <div className="scenario-pick">
                      {skills.slice(0, 12).map((skill) => (
                        <label key={skill.path} className="scenario-pick-item">
                          <input
                            type="checkbox"
                            checked={scenarioSkillPaths.includes(skill.path)}
                            onChange={() => toggleScenarioPath(skill.path)}
                          />
                          <span>{skill.name}</span>
                        </label>
                      ))}
                      {skills.length === 0 && <span className="empty-hint">当前没有可选 Skills</span>}
                    </div>
                  </div>
                  <div className="card-actions" style={{ opacity: 1 }}>
                    <button className="btn btn-primary btn-sm" onClick={createScenario}><Icon name="plus" size={14} />保存场景</button>
                    <button className="btn btn-secondary btn-sm" onClick={() => { setNewScenarioName(""); setNewScenarioDesc(""); setScenarioSkillPaths([]); setNewScenarioSkillInput(""); }}>清空</button>
                  </div>
                </div>
              </div>
              {data.scenarios.length === 0 ? (
                <div className="empty-state"><div className="empty-icon"><Icon name="scenarios" size={48} /></div><h3>暂无场景</h3><p>创建场景来组织 Skills</p></div>
              ) : (
                <div className="scenario-list">
                  {data.scenarios.map(s => (
                    <div className="scenario-card" key={s.id}>
                      <div className="scenario-icon"><Icon name="scenarios" size={24} /></div>
                      <div className="scenario-info">
                        <h3>{s.name}</h3>
                        {s.description && <p>{s.description}</p>}
                        <span>{s.skill_ids.length} 个 Skills · {s.created_at}</span>
                        {expandedScenarioId === s.id && (
                          <div className="scenario-detail">
                            {scenarioDetails[s.id]?.map((item) => (
                              <div key={item.path} className={`scenario-path-row ${item.exists ? "" : "missing"}`}>
                                <div>
                                  <strong>{item.skill?.name || "未找到 Skill"}</strong>
                                  <div className="card-path" style={{ marginTop: 6, marginBottom: 0 }}>{item.path.replace(/^\/Users\/[^/]+/, "~")}</div>
                                </div>
                                <span className={`tag more ${item.exists ? "" : "warn"}`}>{item.exists ? (item.skill?.ai || "已存在") : "已缺失"}</span>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                      <div className="card-actions" style={{ opacity: 1 }}>
                        <button className="btn btn-secondary btn-sm" onClick={() => setExpandedScenarioId(expandedScenarioId === s.id ? "" : s.id)}>
                          <Icon name="preview" size={14} />{expandedScenarioId === s.id ? "收起" : "详情"}
                        </button>
                        <button className="btn btn-primary btn-sm" onClick={() => applyScenario(s)}>
                          <Icon name="sparkles" size={14} />应用
                        </button>
                        <button className="btn btn-secondary btn-sm" onClick={() => loadScenarioIntoEditor(s)}>
                          <Icon name="check" size={14} />回填
                        </button>
                        <button className="btn btn-secondary btn-sm" onClick={() => copyScenarioPaths(s.skill_ids)}>
                          <Icon name="folder" size={14} />复制路径
                        </button>
                        <button className="btn btn-ghost btn-sm danger" onClick={() => deleteScenario(s.id)} title="删除"><Icon name="trash" size={16} /></button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {view === "updates" && (
            <div>
              <div className="content-head">
                <div><h2>更新</h2><span className="count">{updateable.length}</span></div>
                <div className="content-head-actions">
                  <div className="update-toolbar">
                    <span className="update-pill warn">待更新 {updateStats.behind}</span>
                    <span className="update-pill">可更新 {updateStats.syncable}</span>
                    {updateStats.failed > 0 && <span className="update-pill error">异常 {updateStats.failed}</span>}
                  </div>
                  <button className="btn btn-primary btn-sm" onClick={checkAllUpdates} disabled={updateable.some(s => (updateState[s.path] || {}).checking)}>
                    <Icon name="refresh" size={14} />全部检查
                  </button>
                </div>
              </div>
              {updateable.length === 0 ? (
                <div className="empty-state success"><div className="empty-icon"><Icon name="sparkles" size={48} /></div><h3>暂无可展示技能</h3><p>先安装或清除搜索条件后再查看</p></div>
              ) : (
                <div className="grid">
                  {orderedUpdateable.map(s => {
                    const info = getSyncInfo(s);
                    return (
                    <div className="card" key={s.path}>
                      <div className="card-head">
                        <AiAvatar name={s.ai} size={40} />
                        <div className="card-info">
                          <h3>{s.name}</h3>
                          <div className="card-tags">
                            <span className="tag more">{s.ai}</span>
                            <span className="tag more">{getOriginKindLabel(info.originKind)}</span>
                          </div>
                        </div>
                      </div>
                      <div className="update-meta">
                        <div><span className="label">来源</span><span className="value mono">{getRemoteLabel(info.resolvedLocator || info.originLocator || s.upstream_url)}</span></div>
                        <div><span className="label">当前</span><span className="value mono">{formatRefLabel(info.installedRef)}</span></div>
                        <div><span className="label">最新</span><span className="value mono">{formatRefLabel(info.latestRef)}</span></div>
                        <div><span className="label">落后</span><span className={`value ${info.syncState === "behind" ? "warn" : ""}`}>{getLagText(info)}</span></div>
                        <div><span className="label">状态</span><span className={`value status ${getStateTone(info.syncState)}`}>{getStateText(info.syncState)}</span></div>
                        <div><span className="label">检查</span><span className="value">{formatProbeTime(info.lastProbeAt)}</span></div>
                      </div>
                      <div className="card-actions">
                        <button className="btn btn-secondary btn-sm" onClick={() => checkUpdate(s.path)} disabled={info.checking || info.updating}>
                          <Icon name="refresh" size={14} />{info.checking ? "检查中" : "检查"}
                        </button>
                        {info.canSync && (
                          <button className="btn btn-primary btn-sm" onClick={() => updateSkill(s.path)} disabled={info.checking || info.updating}>
                            <Icon name="upload" size={14} />{getUpdateButtonText(info)}
                          </button>
                        )}
                        {!info.canSync && <span className="update-hint">{info.message || "当前来源不支持自动更新"}</span>}
                      </div>
                    </div>
                  )})}
                </div>
              )}
            </div>
          )}

          {view === "settings" && (
            <div className="settings">
              <div className="content-head"><div><h2>设置</h2></div></div>
              <div className="sort-tabs" style={{ marginBottom: 16 }}>
                {[
                  ["backup", "备份"],
                  ["app-update", "应用更新"],
                  ["data", "导入导出"],
                  ["skillset", "Skill Set"],
                  ["tags", "标签"],
                ].map(([key, label]) => (
                  <button key={key} className={`sort-tab ${settingsTab === key ? "active" : ""}`} onClick={() => setSettingsTab(key)}>{label}</button>
                ))}
              </div>
              {settingsTab === "backup" && (
              <div className="settings-card">
                <div className="settings-head"><Icon name="lock" size={20} /><h3>Git 备份</h3></div>
                <div className="settings-body">
                  <div className="form"><label>仓库路径</label><input value={gitRepoPath} onChange={e => setGitRepoPath(e.target.value)} placeholder="/path/to/repo" /></div>
                  <div className="form"><label>远端地址</label><input value={gitRemoteUrl} onChange={e => setGitRemoteUrl(e.target.value)} placeholder="git@github.com:example/skills.git" /></div>
                  <div className="form"><label>分支</label><input value={gitBranch} onChange={e => setGitBranch(e.target.value)} placeholder="main" /></div>
                  <div className="git-meta">当前远端: {gitRemoteUrl || "未配置"}</div>
                  <div className="git-meta">上次同步: {data.git.last_sync || "从未"}</div>
                  <div className="card-actions" style={{marginTop:12}}><button className="btn btn-primary btn-sm" onClick={saveGitBackup}><Icon name="check" size={14} />保存</button><button className="btn btn-secondary btn-sm" onClick={syncGit}><Icon name="upload" size={14} />立即同步</button></div>
                </div>
              </div>
              )}
              {settingsTab === "app-update" && (
              <div className="settings-card">
                <div className="settings-head"><Icon name="updates" size={20} /><h3>应用更新</h3></div>
                <div className="settings-body">
                  <div className="app-update-panel">
                    <div className="app-update-main">
                      <span className={`update-pill ${appUpdateView.statusTone}`}>{appUpdateView.statusLabel}</span>
                      <h3>{appUpdateView.nextVersion ? `SkillMate ${appUpdateView.nextVersion}` : "SkillMate"}</h3>
                      <p>{appUpdateView.nextVersion ? "检测到可安装的新版本" : "检查 GitHub Releases 上的最新正式版本"}</p>
                    </div>
                    <div className="app-update-meta">
                      <div><span className="label">当前版本</span><span className="value mono">{appUpdateView.currentVersion || "未知"}</span></div>
                      <div><span className="label">新版本</span><span className="value mono">{appUpdateView.nextVersion || "暂无"}</span></div>
                      <div><span className="label">发布时间</span><span className="value">{appUpdateView.dateLabel}</span></div>
                    </div>
                  </div>
                  {appUpdateView.progressText && (
                    <div className="app-update-progress">
                      <div className="app-update-progress-head">
                        <span>下载进度</span>
                        <strong>{appUpdateView.progressText}</strong>
                      </div>
                      <div className="progress-track">
                        <div className="progress-fill" style={{ width: `${appUpdateView.progressPercent || 8}%` }} />
                      </div>
                    </div>
                  )}
                  {appUpdateView.releaseNotes && (
                    <div className="import-preview">
                      <div className="import-preview-head">
                        <strong>更新日志</strong>
                        <span>来自 release metadata</span>
                      </div>
                      <pre className="app-update-notes">{appUpdateView.releaseNotes}</pre>
                    </div>
                  )}
                  {appUpdateView.error && (
                    <div className="install-compact error">
                      <span>更新检查异常</span>
                      <strong>{appUpdateView.error}</strong>
                    </div>
                  )}
                  <div className="card-actions" style={{ marginTop: 12, opacity: 1 }}>
                    <button className="btn btn-secondary btn-sm" onClick={checkAppUpdate} disabled={!appUpdateView.canCheck}>
                      <Icon name="refresh" size={14} />{appUpdateState.status === "checking" ? "检查中" : "检查更新"}
                    </button>
                    <button className="btn btn-primary btn-sm" onClick={installAppUpdate} disabled={!appUpdateView.canInstall}>
                      <Icon name="upload" size={14} />下载并安装
                    </button>
                    <button className="btn btn-secondary btn-sm" onClick={restartApp} disabled={!appUpdateView.canRestart}>
                      <Icon name="refresh" size={14} />重启应用
                    </button>
                  </div>
                  <div className="git-meta">应用更新使用 GitHub Releases 的 latest.json；更新包会由 Tauri 签名校验后再安装。</div>
                </div>
              </div>
              )}
              {settingsTab === "data" && (
              <div className="settings-card">
                <div className="settings-head"><Icon name="upload" size={20} /><h3>导入 / 导出</h3></div>
                <div className="settings-body">
                  <div className="form"><label>导出文件</label><input value={exportPath} onChange={e => setExportPath(e.target.value)} placeholder="~/skillmate-export.json" /></div>
                  <div className="card-actions" style={{ marginTop: 12, opacity: 1 }}>
                    <button className="btn btn-primary btn-sm" onClick={exportLibraryFile}><Icon name="upload" size={14} />导出组织数据</button>
                  </div>
                  <div className="form" style={{ marginTop: 16 }}><label>导入文件</label><input value={importPath} onChange={e => updateImportPath(e.target.value)} placeholder="~/skillmate-export.json" /></div>
                  <div className="form"><label>导入方式</label><select value={importMode} onChange={e => updateImportMode(e.target.value)}><option value="merge">合并</option><option value="replace">替换现有组织数据</option></select></div>
                  <div className="git-meta">导入和导出只处理标签、场景以及当前受管 Skill 清单，不会直接覆盖本地 Skill 文件。</div>
                  {importPreview && (
                    <div className="import-preview">
                      <div className="import-preview-head">
                        <strong>{importPreview.replace_existing ? "替换导入预览" : "合并导入预览"}</strong>
                        <span>{importMode === "replace" ? "将先清空再恢复组织数据" : "仅写入导入文件中的组织数据"}</span>
                      </div>
                      <ul className="import-preview-list">
                        {buildImportPreviewSummary(importPreview).map((line) => (
                          <li key={line}>{line}</li>
                        ))}
                      </ul>
                    </div>
                  )}
                  <div className="card-actions" style={{ marginTop: 12, opacity: 1 }}>
                    <button className="btn btn-secondary btn-sm" onClick={previewImportLibraryFile} disabled={previewingImport}>
                      <Icon name="preview" size={14} />{previewingImport ? "预览中" : "预览导入"}
                    </button>
                    <button className="btn btn-primary btn-sm" onClick={importLibraryFile} disabled={!importPreview || !importPreviewCurrent}>
                      <Icon name="check" size={14} />导入组织数据
                    </button>
                  </div>
                  <div className="form" style={{ marginTop: 18 }}><label>场景 manifest</label><input value={scenarioManifestPath} onChange={e => updateScenarioManifestPath(e.target.value)} placeholder="~/skillmate-scenarios.json" /></div>
                  <div className="form"><label>场景导入方式</label><select value={scenarioManifestMode} onChange={e => updateScenarioManifestMode(e.target.value)}><option value="merge">合并</option><option value="replace">替换现有场景</option></select></div>
                  <div className="git-meta">场景 manifest 只处理场景和 Skill 路径引用，不会修改标签或本地 Skill 文件。</div>
                  {scenarioManifestPreview && (
                    <div className="import-preview">
                      <div className="import-preview-head">
                        <strong>{scenarioManifestPreview.replace_existing ? "替换场景预览" : "合并场景预览"}</strong>
                        <span>{scenarioManifestMode === "replace" ? "将先清空现有场景" : "仅写入 manifest 中的场景"}</span>
                      </div>
                      <ul className="import-preview-list">
                        {buildScenarioManifestPreviewSummary(scenarioManifestPreview).map((line) => (
                          <li key={line}>{line}</li>
                        ))}
                      </ul>
                    </div>
                  )}
                  <div className="card-actions" style={{ marginTop: 12, opacity: 1 }}>
                    <button className="btn btn-secondary btn-sm" onClick={exportScenarioManifestFile}>
                      <Icon name="upload" size={14} />导出场景
                    </button>
                    <button className="btn btn-secondary btn-sm" onClick={previewImportScenarioManifestFile} disabled={previewingScenarioManifest}>
                      <Icon name="preview" size={14} />{previewingScenarioManifest ? "预览中" : "预览场景"}
                    </button>
                    <button className="btn btn-primary btn-sm" onClick={importScenarioManifestFile} disabled={!scenarioManifestPreview || !scenarioManifestPreviewCurrent}>
                      <Icon name="check" size={14} />导入场景
                    </button>
                  </div>
                </div>
              </div>
              )}
              {settingsTab === "skillset" && (
              <div className="settings-card">
                <div className="settings-head"><Icon name="skills" size={20} /><h3>Skill Set</h3></div>
                <div className="settings-body">
                  <div className="form" style={{ marginTop: 18 }}><label>SkillMate manifest</label><input value={skillMateManifestPath} onChange={e => updateSkillMateManifestPath(e.target.value)} placeholder="~/skillmate.toml" /></div>
                  <div className="git-meta">skillmate.toml 记录 Skill 来源和目标助手；应用前必须先预览，不会覆盖已有目标目录。</div>
                  {skillMateManifestPreview && (
                    <div className="import-preview">
                      <div className="import-preview-head">
                        <strong>SkillMate manifest 预览</strong>
                        <span>{skillMateManifestPreview.can_apply ? "可应用" : "存在冲突"}</span>
                      </div>
                      <ul className="import-preview-list">
                        {buildSkillMateManifestPreviewSummary(skillMateManifestPreview).map((line) => (
                          <li key={line}>{line}</li>
                        ))}
                      </ul>
                    </div>
                  )}
                  <div className="card-actions" style={{ marginTop: 12, opacity: 1 }}>
                    <button className="btn btn-secondary btn-sm" onClick={exportSkillMateManifestFile}>
                      <Icon name="upload" size={14} />导出 SkillMate manifest
                    </button>
                    <button className="btn btn-secondary btn-sm" onClick={previewSkillMateManifestFile} disabled={previewingSkillMateManifest}>
                      <Icon name="preview" size={14} />{previewingSkillMateManifest ? "预览中" : "预览 manifest"}
                    </button>
                    <button className="btn btn-primary btn-sm" onClick={applySkillMateManifestFile} disabled={!skillMateManifestPreview || !skillMateManifestPreviewCurrent || !skillMateManifestPreview.can_apply}>
                      <Icon name="check" size={14} />应用 manifest
                    </button>
                  </div>
                  <div className="form" style={{ marginTop: 18 }}><label>Skill Set Profile</label><input value={skillProfileName} onChange={e => setSkillProfileName(e.target.value)} placeholder="例如：写作模式 / 开发模式" /></div>
                  <div className="form"><label>Profile 说明</label><input value={skillProfileDescription} onChange={e => setSkillProfileDescription(e.target.value)} placeholder="这个组合适合什么工作流" /></div>
                  <div className="git-meta">Profile 会保存当前所有助手下的 Skill 来源组合；应用前会预览，默认只补齐缺失项。</div>
                  <div className="card-actions" style={{ marginTop: 12, opacity: 1 }}>
                    <button className="btn btn-secondary btn-sm" onClick={saveCurrentSkillProfile}>
                      <Icon name="check" size={14} />保存当前组合
                    </button>
                    <button className="btn btn-secondary btn-sm" onClick={rollbackSkillProfile} disabled={!skillProfiles.previous_active_profile_id || applyingSkillProfile}>
                      <Icon name="refresh" size={14} />回滚上个 Profile
                    </button>
                  </div>
                  {skillProfiles.profiles?.length > 0 && (
                    <div className="scenario-detail" style={{ marginTop: 12 }}>
                      {skillProfiles.profiles.map((profile) => (
                        <div key={profile.id} className="scenario-path-row">
                          <div>
                            <strong>{profile.name}{profile.active ? " · 当前" : ""}</strong>
                            <div className="card-path" style={{ marginTop: 6, marginBottom: 0 }}>{profile.description || `${profile.skills.length} 条 Skill 记录`}</div>
                          </div>
                          <div className="card-actions" style={{ opacity: 1 }}>
                            <button className="btn btn-secondary btn-sm" onClick={() => previewSkillProfile(profile.id)} disabled={previewingSkillProfile || applyingSkillProfile}>
                              <Icon name="preview" size={14} />预览
                            </button>
                            <button className="btn btn-primary btn-sm" onClick={() => applySkillProfile(profile.id)} disabled={previewingSkillProfile || applyingSkillProfile}>
                              <Icon name="check" size={14} />应用
                            </button>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                  {skillProfilePreview && (
                    <div className="import-preview">
                      <div className="import-preview-head">
                        <strong>Profile 预览</strong>
                        <span>{skillProfilePreview.manifest_preview?.can_apply && !skillProfilePreview.profile_issues?.length ? "可应用" : "存在问题"}</span>
                      </div>
                      <ul className="import-preview-list">
                        {buildSkillProfilePreviewSummary(skillProfilePreview).map((line) => (
                          <li key={line}>{line}</li>
                        ))}
                      </ul>
                    </div>
                  )}
                </div>
              </div>
              )}
              {settingsTab === "tags" && (
              <div className="settings-card">
                <div className="settings-head"><Icon name="tag" size={20} /><h3>标签管理</h3></div>
                <div className="settings-body">
                  <div className="tag-form"><input value={newTagName} onChange={e => setNewTagName(e.target.value)} placeholder="标签名" /><input type="color" value={newTagColor} onChange={e => setNewTagColor(e.target.value)} /><button className="btn btn-primary btn-sm" onClick={addTag}><Icon name="plus" size={14} />添加</button></div>
                  <div className="tag-list" style={{marginTop:12}}>{tags.map(tag => (<div key={tag.id} className="tag-chip active" style={{"--c": tag.color}}><span className="tag-dot" />{tag.name}</div>))}</div>
                </div>
              </div>
              )}
            </div>
          )}
        </main>
      </div>

      {installOpen && (
        <div className="modal-overlay" onClick={() => setInstallOpen(false)}>
          <div className="modal install-modal" onClick={e => e.stopPropagation()}>
            <div className="modal-head"><h3><Icon name="plus" size={18} />安装 Skill</h3><button className="modal-x" onClick={() => setInstallOpen(false)}><Icon name="x" size={20} /></button></div>
            <div className="form">
              <label>Skill 来源</label>
              <input value={pkg} onChange={e => setPkg(e.target.value)} placeholder="Git URL、owner/repo、GitHub tree URL 或本地目录" />
            </div>
            {installDetectionView && (
              <div className={`install-compact ${installDetectionView.tone}`}>
                <span>{installDetectionView.sourceLabel}</span>
                <strong>{installDetectionView.summary}</strong>
                {installDetectionView.warningSummary && <p>{installDetectionView.warningSummary}</p>}
              </div>
            )}
            <div className="install-target">
              <div className="form">
                <label>安装到</label>
                <select value={installAssistant} onChange={e => setInstallAssistant(e.target.value)}>
                  {data.assistants.map((assistant) => (
                    <option key={assistant.name} value={assistant.name}>{assistant.name}</option>
                  ))}
                </select>
              </div>
              {showProjectLinkOption && (
                <label className="install-switch">
                  <input type="checkbox" checked={installMode === "symlink"} onChange={e => setInstallMode(e.target.checked ? "symlink" : "copy")} />
                  <span>链接到项目</span>
                </label>
              )}
            </div>
            {showProjectLinkOption && installMode === "symlink" && (
              <div className="install-project">
                <div className="form">
                  <label>项目路径</label>
                  <input value={projectPath} onChange={e => setProjectPath(e.target.value)} placeholder="/path/to/project" />
                </div>
                {previewingProjectTargets && <div className="git-meta">正在识别项目目标目录...</div>}
                {projectTargetPreview.length > 0 && (
                  <ul className="import-preview-list">
                    {buildProjectTargetPreviewSummary(projectTargetPreview).map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                )}
              </div>
            )}
            {(showInstallAdvancedOptions || installAdvancedOpen) && (
              <div className="install-advanced">
                <div className="form">
                  <label>来源类型</label>
                  <select value={src} onChange={e => setSrc(e.target.value)}>
                    {SUPPORTED_INSTALL_SOURCES.map((source) => (
                      <option key={source} value={source}>{source === "git" ? "Git 仓库" : "本地目录"}</option>
                    ))}
                  </select>
                </div>
              </div>
            )}
            {installStructurePreview && (
              <div className={`structure-preview install-preview-card ${installPreviewView?.tone || (installStructurePreview.can_install === false ? "error" : getStructureStatusTone(installStructurePreview.structure_status))}`}>
                <div className="structure-preview-head">
                  <span>安装计划</span>
                  <strong>{installPreviewView?.canApply && installPreviewCurrent ? "可安装" : "需要检查"}</strong>
                </div>
                <ul className="install-summary-list">
                  {buildInstallPreviewSummary(installStructurePreview).slice(0, 4).map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
                {!installPreviewCurrent && <p>输入已变化，请重新检查结构。</p>}
              </div>
            )}
            <button className="btn btn-primary full install-primary" onClick={runInstallPrimaryAction} disabled={installPrimaryAction.disabled || loading}>
              <Icon name={installPrimaryAction.icon} size={16} />{installPrimaryAction.label}
            </button>
            <div className="install-secondary-actions">
              <button className="btn btn-ghost btn-sm" onClick={() => setInstallDetailsOpen(!installDetailsOpen)}>
                <Icon name="preview" size={14} />{installDetailsOpen ? "收起执行信息" : "查看执行信息"}
              </button>
              <button className="btn btn-ghost btn-sm" onClick={() => setInstallAdvancedOpen(!installAdvancedOpen)}>
                <Icon name="settings" size={14} />{installAdvancedOpen ? "收起高级选项" : "高级选项"}
              </button>
            </div>
            {installDetailsOpen && (
              <div className="install-details">
                <div className="form"><label>执行方式</label><div className="cmd">{cmd}</div></div>
                {installStructurePreview && (
                  <>
                    <p>{buildStructureWarningSummary(installStructurePreview)}</p>
                    {installPreviewView?.packageWarnings && <p>{installPreviewView.packageWarnings}</p>}
                    {installPreviewView?.needsModel && <p>本地规则置信度不足，可后续启用模型辅助识别。</p>}
                    {installPreviewView?.skills?.length > 0 && (
                      <ul className="import-preview-list">
                        {installPreviewView.skills.map((skill) => (
                          <li key={skill.relative_path}>{skill.relative_path} · {getStructureStatusLabel(skill.structure_status)}</li>
                        ))}
                      </ul>
                    )}
                    {installPreviewView?.actions?.length > 0 && (
                      <ul className="import-preview-list">
                        {installPreviewView.actions.map((action) => (
                          <li key={`${action.action}-${action.target}`}>{action.label}：{action.source} → {action.target}</li>
                        ))}
                      </ul>
                    )}
                    {installPreviewView?.conflicts?.length > 0 && (
                      <ul className="import-preview-list danger">
                        {installPreviewView.conflicts.map((conflict) => (
                          <li key={`${conflict.reason}-${conflict.target}`}>{conflict.target}：{conflict.reason}</li>
                        ))}
                      </ul>
                    )}
                  </>
                )}
              </div>
            )}
          </div>
        </div>
      )}

      {previewOpen && (
        <div className="modal-overlay" onClick={() => setPreviewOpen(false)}>
          <div className="modal large" onClick={e => e.stopPropagation()}>
            <div className="modal-head"><h3>{preview.title}</h3><button className="modal-x" onClick={() => setPreviewOpen(false)}><Icon name="x" size={20} /></button></div>
            {preview.validation && (
              <div className="import-preview" style={{ marginBottom: 14 }}>
                <div className="import-preview-head">
                  <strong>结构验证</strong>
                  <span>{getStructureStatusLabel(preview.validation.structure_status)}</span>
                </div>
                <ul className="import-preview-list">
                  {buildValidationSummary(preview.validation).map((check) => (
                    <li key={check.code}>{check.code}：{check.label} · {check.message}</li>
                  ))}
                </ul>
              </div>
            )}
            <pre className="readme">{preview.content}</pre>
          </div>
        </div>
      )}

      {tagEditor.open && (
        <div className="modal-overlay" onClick={() => setTagEditor({ open: false, skill: null, selected: [] })}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-head">
              <h3>编辑标签</h3>
              <button className="modal-x" onClick={() => setTagEditor({ open: false, skill: null, selected: [] })}><Icon name="x" size={20} /></button>
            </div>
            <p style={{ color: "var(--text2)", fontSize: "0.9rem", marginBottom: 16 }}>{tagEditor.skill?.name}</p>
            <div className="tag-list">
              {tags.map((tag) => (
                <button
                  key={tag.id}
                  className={`tag-chip ${tagEditor.selected.includes(tag.id) ? "active" : ""}`}
                  style={{ "--c": tag.color }}
                  onClick={() => toggleSkillTag(tag.id)}
                >
                  <span className="tag-dot" />
                  {tag.name}
                </button>
              ))}
              {tags.length === 0 && <p className="empty-hint">请先在设置页创建标签</p>}
            </div>
            <div className="card-actions" style={{ justifyContent: "flex-end", marginTop: 20 }}>
              <button className="btn btn-secondary btn-sm" onClick={() => setTagEditor({ open: false, skill: null, selected: [] })}>取消</button>
              <button className="btn btn-primary btn-sm" onClick={saveSkillTags}>保存</button>
            </div>
          </div>
        </div>
      )}

      <div className={`toast ${toastState.show ? "show" : ""} ${toastState.type}`}>{toastState.msg}</div>

      {confirmState.open && (
        <div className="modal-overlay" onClick={() => setConfirmState(s => ({ ...s, open: false }))}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-head"><h3>{confirmState.title}</h3><button className="modal-x" onClick={() => setConfirmState(s => ({ ...s, open: false }))}><Icon name="x" size={20} /></button></div>
            <p style={{ color: "var(--text2)", fontSize: "0.9rem", marginBottom: 20 }}>{confirmState.message}</p>
            <div className="card-actions" style={{ justifyContent: "flex-end" }}>
              <button className="btn btn-secondary btn-sm" onClick={() => setConfirmState(s => ({ ...s, open: false }))}>取消</button>
              <button className="btn btn-danger btn-sm" onClick={() => { const cb = confirmState.onConfirm; setConfirmState({ open: false, title: "", message: "", onConfirm: null }); cb?.(); }}>确认删除</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
