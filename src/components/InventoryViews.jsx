import React from "react";
import Icon from "./Icon.jsx";
import { buildSkillCardView } from "../lib/skillmate.mjs";
import claudeLogo from "../assets/brands/claude.svg";
import codexLogo from "../assets/brands/codex-openai.svg";
import openclawLogo from "../assets/brands/openclaw.svg";
import geminiLogo from "../assets/brands/gemini.svg";

const AI_META = {
  claude: { bg: "#f7f3ee", src: claudeLogo, mode: "contain" },
  codex: { bg: "#ffffff", src: codexLogo, mode: "contain" },
  openclaw: { bg: "#08111f", src: openclawLogo, mode: "contain" },
  gemini: { bg: "#ffffff", src: geminiLogo, mode: "contain" },
};

const AiAvatar = React.memo(function AiAvatar({ name, brand, size = 36 }) {
  const metadata = AI_META[brand] || { bg: "#eff6ff" };
  return (
    <div
      className="ai-avatar"
      style={{
        width: size,
        height: size,
        minWidth: size,
        minHeight: size,
        borderRadius: Math.max(8, Math.round(size * 0.24)),
        background: metadata.bg,
      }}
      title={name}
      aria-label={name}
    >
      {metadata.src ? (
        <img className={`ai-avatar-img ${metadata.mode || "contain"}`} src={metadata.src} alt={name} loading="lazy" draggable="false" />
      ) : (
        <span style={{ fontSize: Math.max(10, size * 0.34), fontWeight: 700 }}>{name.slice(0, 1)}</span>
      )}
    </div>
  );
});

function formatHomePath(path = "") {
  return path
    .replace(/^\/Users\/[^/]+/, "~")
    .replace(/^[A-Za-z]:\\Users\\[^\\]+/i, "~");
}

export function SkillsView({
  skills,
  allSkillCount,
  selectedTagCount,
  tags,
  onInstall,
  onClearFilters,
  onEditTags,
  onOpenDirectory,
  onPreview,
  onUnlink,
  onRemove,
}) {
  return (
    <>
      <div className="content-head">
        <div><h2>所有 Skills</h2><span className="count">{skills.length} 个{skills.length !== allSkillCount ? ` / ${allSkillCount} 总计` : ""}</span></div>
        <div className="content-head-actions">
          {selectedTagCount > 0 && <div className="filter-tag"><Icon name="tag" size={14} />已选 {selectedTagCount} 个标签</div>}
          <button className="btn btn-primary btn-sm" onClick={onInstall}><Icon name="plus" size={14} />安装</button>
        </div>
      </div>
      {skills.length === 0 ? (
        <div className="empty-state">
          <div className="empty-icon"><Icon name="box" size={48} /></div>
          <h3>{allSkillCount > 0 ? "没有匹配的 Skills" : "暂无 Skills"}</h3>
          <p>{allSkillCount > 0 ? "清除搜索或标签筛选后继续查看已有 Skills" : "从 Git 仓库或本地目录添加第一个 Skill"}</p>
          <div className="empty-actions">
            {allSkillCount > 0 && <button className="btn btn-secondary" onClick={onClearFilters}><Icon name="x" size={16} />清除筛选</button>}
            <button className="btn btn-primary" onClick={onInstall}><Icon name="plus" size={16} />安装 Skill</button>
          </div>
        </div>
      ) : (
        <div className="grid">
          {skills.map((skill, index) => {
            const card = buildSkillCardView(skill);
            return (
              <div className="card" key={`${skill.path}-${skill.name}`} style={{ "--i": index }}>
                <div className="card-head">
                  <AiAvatar name={skill.ai} brand={skill.aiIcon} size={40} />
                  <div className="card-info">
                    <div className="card-title-row">
                      <h3>{card.title}</h3>
                      {card.sourceLabel && <span className={`source-badge ${skill.source_type || card.sourceLabel.toLowerCase()}`}>{card.sourceLabel}</span>}
                    </div>
                    <div className="card-tags">
                      <span className={`structure-badge ${card.structureTone}`} title={card.warningSummary}>{card.structureLabel}</span>
                      {card.isShared && <span className="structure-badge" title={card.availabilityLabel}>共享 {card.availableIn.length}</span>}
                      {card.hasManagedDrift && <span className="structure-badge warn" title="内容已偏离安装时状态">内容已变更</span>}
                      {card.securityWarningCount > 0 && <span className="structure-badge warn" title={card.securityWarningSummary}>风险 {card.securityWarningCount}</span>}
                      {skill.tags.slice(0, 2).map(tagId => {
                        const tag = tags.find(item => item.id === tagId);
                        return tag ? <span key={tag.id} className="tag" style={{ background: `${tag.color}20`, color: tag.color }}>{tag.name}</span> : null;
                      })}
                      {skill.tags.length > 2 && <span className="tag more">+{skill.tags.length - 2}</span>}
                    </div>
                  </div>
                </div>
                {card.description && <p className="card-desc">{card.description}</p>}
                <div className="card-meta"><span title={card.availabilityLabel}><AiAvatar name={skill.ai} brand={skill.aiIcon} size={14} />{card.availabilityLabel || skill.ai}</span><span><Icon name="folder" size={12} />{skill.size}</span></div>
                <div className="card-path" title={skill.path}>{formatHomePath(skill.path)}</div>
                {skill.symlink_source && <div className="git-meta">源：{formatHomePath(skill.symlink_source)}</div>}
                <div className="card-actions">
                  <button className="btn btn-ghost btn-sm" onClick={() => onEditTags(skill)} title="编辑标签" aria-label={`编辑 ${skill.name} 标签`}><Icon name="tag" size={16} /></button>
                  <button className="btn btn-ghost btn-sm" onClick={() => onOpenDirectory(skill.path)} title="打开文件夹" aria-label={`打开 ${skill.name} 文件夹`}><Icon name="folder" size={16} /></button>
                  <button className="btn btn-ghost btn-sm" onClick={() => onPreview(skill.path)} title="预览说明" aria-label={`预览 ${skill.name} 说明`}><Icon name="preview" size={16} /></button>
                  {card.canUnlink ? (
                    <button className="btn btn-ghost btn-sm danger" onClick={() => onUnlink(skill.path, skill.name)} title="解除软连接" aria-label={`解除 ${skill.name} 软连接`}><Icon name="x" size={16} /></button>
                  ) : card.canDelete ? (
                    <button className="btn btn-ghost btn-sm danger" onClick={() => onRemove(skill.path, skill.name, card.availableIn)} title="删除" aria-label={`删除 ${skill.name}`}><Icon name="trash" size={16} /></button>
                  ) : null}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </>
  );
}

export function AssistantsView({ assistants, installedCount }) {
  return (
    <div>
      <div className="content-head"><div><h2>AI 助手</h2><span className="count">{installedCount} / {assistants.length} 已配置</span></div></div>
      <div className="grid ai-grid">
        {assistants.map(assistant => (
          <div className={`ai-card ${assistant.exists ? "ok" : "no-exist"}`} key={assistant.name}>
            <AiAvatar name={assistant.name} brand={assistant.icon} size={48} />
            <h3>{assistant.name}</h3>
            <p className="ai-path" title={(assistant.paths || [assistant.path]).join("\n")}>{formatHomePath(assistant.path)}{assistant.paths?.length > 1 ? ` · ${assistant.paths.length} 个目录` : ""}</p>
            <div className={`ai-status ${assistant.exists ? "ok" : "no"}`}><Icon name={assistant.exists ? "check" : "x"} size={14} />{assistant.exists ? "已配置" : "未配置"}</div>
            {assistant.exists && assistant.skills.length > 0 && (
              <div className="ai-skill-tags">
                {assistant.skills.slice(0, 3).map(skill => <span key={skill.path || skill.name} className="ai-skill-tag">{skill.name}</span>)}
                {assistant.skills.length > 3 && <span className="ai-skill-tag more">+{assistant.skills.length - 3}</span>}
              </div>
            )}
            {assistant.exists && assistant.skills.length === 0 && <div className="ai-empty-hint">暂无 Skills</div>}
            {Array.isArray(assistant.diagnostics) && assistant.diagnostics.length > 0 && (
              <details className="scan-diagnostics">
                <summary>扫描诊断 {assistant.diagnostics.length}</summary>
                <ul>
                  {assistant.diagnostics.slice(0, 5).map((diagnostic, index) => (
                    <li key={`${diagnostic.path}-${diagnostic.code}-${index}`}>
                      <span title={diagnostic.path}>{formatHomePath(diagnostic.path)}</span>
                      <small>{diagnostic.message}</small>
                    </li>
                  ))}
                </ul>
              </details>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function remoteLabel(url) {
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

function refLabel(value) {
  if (!value) return "—";
  return /^[0-9a-f]{12,}$/i.test(value) ? value.slice(0, 7) : value;
}

function probeTime(value) {
  if (!value) return "从未";
  const date = new Date(Number(value));
  if (Number.isNaN(date.getTime())) return "从未";
  return date.toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });
}

function originKindLabel(kind) {
  if (kind === "git") return "Git";
  if (["legacy_npm", "npm"].includes(kind)) return "历史 npm";
  if (["legacy_pip", "pip"].includes(kind)) return "历史 PyPI";
  if (kind === "local") return "本地";
  return "未托管";
}

function stateText(state) {
  return {
    behind: "可更新",
    current: "已是最新",
    failed: "检查失败",
    diverged: "存在分叉",
    ahead_local: "本地领先",
    local_fixed: "本地固定",
    source_missing: "来源缺失",
    unsupported: "暂不支持",
  }[state] || "待检查";
}

function stateTone(state) {
  if (state === "behind") return "warn";
  if (["failed", "source_missing"].includes(state)) return "error";
  if (state === "current") return "success";
  return "muted";
}

function lagText(info) {
  if (info.originKind === "git") return `${info.lagCount || 0} 个提交`;
  if (["legacy_npm", "legacy_pip", "npm", "pip"].includes(info.originKind)) {
    if (info.syncState === "behind") return "有新版本";
    if (info.syncState === "current") return "已最新";
  }
  return "—";
}

function updateButtonText(info) {
  if (info.updating) return info.originKind === "git" ? "更新中" : "同步中";
  return info.originKind === "git" ? "一键更新" : "一键同步";
}

export function UpdatesView({ skills, orderedSkills, stats, updateState, getSyncInfo, checkAll, checkOne, updateOne }) {
  return (
    <div>
      <div className="content-head">
        <div><h2>更新</h2><span className="count">{skills.length}</span></div>
        <div className="content-head-actions">
          <div className="update-toolbar"><span className="update-pill warn">待更新 {stats.behind}</span><span className="update-pill">可更新 {stats.syncable}</span>{stats.failed > 0 && <span className="update-pill error">异常 {stats.failed}</span>}</div>
          <button className="btn btn-primary btn-sm" onClick={checkAll} disabled={skills.some(skill => (updateState[skill.path] || {}).checking)}><Icon name="refresh" size={14} />全部检查</button>
        </div>
      </div>
      {skills.length === 0 ? (
        <div className="empty-state success"><div className="empty-icon"><Icon name="sparkles" size={48} /></div><h3>暂无可展示技能</h3><p>先安装或清除搜索条件后再查看</p></div>
      ) : (
        <div className="grid">
          {orderedSkills.map(skill => {
            const info = getSyncInfo(skill);
            const card = buildSkillCardView(skill);
            return (
              <div className="card" key={skill.path}>
                <div className="card-head"><AiAvatar name={skill.ai} brand={skill.aiIcon} size={40} /><div className="card-info"><h3>{skill.name}</h3><div className="card-tags"><span className="tag more">{card.availabilityLabel || skill.ai}</span><span className="tag more">{originKindLabel(info.originKind)}</span></div></div></div>
                <div className="update-meta">
                  <div><span className="label">来源</span><span className="value mono">{remoteLabel(info.resolvedLocator || info.originLocator || skill.upstream_url)}</span></div>
                  <div><span className="label">当前</span><span className="value mono">{refLabel(info.installedRef)}</span></div>
                  <div><span className="label">最新</span><span className="value mono">{refLabel(info.latestRef)}</span></div>
                  <div><span className="label">落后</span><span className={`value ${info.syncState === "behind" ? "warn" : ""}`}>{lagText(info)}</span></div>
                  <div><span className="label">状态</span><span className={`value status ${stateTone(info.syncState)}`}>{stateText(info.syncState)}</span></div>
                  <div><span className="label">检查</span><span className="value">{probeTime(info.lastProbeAt)}</span></div>
                </div>
                <div className="card-actions">
                  <button className="btn btn-secondary btn-sm" onClick={() => checkOne(skill.path)} disabled={info.checking || info.updating}><Icon name="refresh" size={14} />{info.checking ? "检查中" : "检查"}</button>
                  {info.canSync && <button className="btn btn-primary btn-sm" onClick={() => updateOne(skill.path)} disabled={info.checking || info.updating}><Icon name="upload" size={14} />{updateButtonText(info)}</button>}
                  {!info.canSync && <span className="update-hint">{info.message || "当前来源不支持自动更新"}</span>}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
