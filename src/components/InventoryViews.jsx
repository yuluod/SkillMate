import React from "react";
import Icon from "./Icon.jsx";
import { buildSkillCardView } from "../lib/skillmate.mjs";
import claudeLogo from "../assets/brands/claude.svg";
import codexLogo from "../assets/brands/codex-openai.svg";
import openclawLogo from "../assets/brands/openclaw.svg";
import geminiLogo from "../assets/brands/gemini.svg";
import cursorLogo from "../assets/brands/cursor.png";
import { useI18n } from "../lib/i18n.jsx";

const AI_META = {
  claude: { bg: "#f7f3ee", src: claudeLogo, mode: "contain" },
  codex: { bg: "#ffffff", src: codexLogo, mode: "contain" },
  openclaw: { bg: "#08111f", src: openclawLogo, mode: "contain" },
  gemini: { bg: "#ffffff", src: geminiLogo, mode: "contain" },
  cursor: { bg: "#ffffff", src: cursorLogo, mode: "cover" },
};

export const AiAvatar = React.memo(function AiAvatar({ name, brand, size = 36 }) {
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
  const { t } = useI18n();
  return (
    <>
      <div className="content-head">
        <div><h2>{t("skills.title")}</h2><span className="count">{t(skills.length !== allSkillCount ? "skills.countFiltered" : "skills.count", { count: skills.length, total: allSkillCount })}</span></div>
        <div className="content-head-actions">
          {selectedTagCount > 0 && <div className="filter-tag"><Icon name="tag" size={14} />{t("skills.selectedTags", { count: selectedTagCount })}</div>}
          <button className="btn btn-primary btn-sm" onClick={onInstall}><Icon name="plus" size={14} />{t("common.install")}</button>
        </div>
      </div>
      {skills.length === 0 ? (
        <div className="empty-state">
          <div className="empty-icon"><Icon name="box" size={48} /></div>
          <h3>{t(allSkillCount > 0 ? "skills.noMatch" : "skills.empty")}</h3>
          <p>{t(allSkillCount > 0 ? "skills.noMatchHint" : "skills.emptyHint")}</p>
          <div className="empty-actions">
            {allSkillCount > 0 && <button className="btn btn-secondary" onClick={onClearFilters}><Icon name="x" size={16} />{t("skills.clearFilters")}</button>}
            <button className="btn btn-primary" onClick={onInstall}><Icon name="plus" size={16} />{t("skills.installOne")}</button>
          </div>
        </div>
      ) : (
        <div className="grid">
          {skills.map((skill, index) => {
            const card = buildSkillCardView(skill, t);
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
                      {card.isShared && <span className="structure-badge" title={card.availabilityLabel}>{t("skills.shared", { count: card.availableIn.length })}</span>}
                      {card.hasManagedDrift && <span className="structure-badge warn">{t("skills.changed")}</span>}
                      {card.securityWarningCount > 0 && <span className="structure-badge warn" title={card.securityWarningSummary}>{t("skills.risk", { count: card.securityWarningCount })}</span>}
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
                {skill.symlink_source && <div className="git-meta">{t("skills.source", { path: formatHomePath(skill.symlink_source) })}</div>}
                <div className="card-actions">
                  <button className="btn btn-ghost btn-sm" onClick={() => onEditTags(skill)} title={t("skills.editTags", { name: skill.name })} aria-label={t("skills.editTags", { name: skill.name })}><Icon name="tag" size={16} /></button>
                  <button className="btn btn-ghost btn-sm" onClick={() => onOpenDirectory(skill.path)} title={t("skills.openFolder", { name: skill.name })} aria-label={t("skills.openFolder", { name: skill.name })}><Icon name="folder" size={16} /></button>
                  <button className="btn btn-ghost btn-sm" onClick={() => onPreview(skill.path)} title={t("skills.preview", { name: skill.name })} aria-label={t("skills.preview", { name: skill.name })}><Icon name="preview" size={16} /></button>
                  {card.canUnlink ? (
                    <button className="btn btn-ghost btn-sm danger" onClick={() => onUnlink(skill.path, skill.name)} title={t("skills.unlink", { name: skill.name })} aria-label={t("skills.unlink", { name: skill.name })}><Icon name="x" size={16} /></button>
                  ) : card.canDelete ? (
                    <button className="btn btn-ghost btn-sm danger" onClick={() => onRemove(skill.path, skill.name, card.availableIn)} title={t("skills.remove", { name: skill.name })} aria-label={t("skills.remove", { name: skill.name })}><Icon name="trash" size={16} /></button>
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
  const { t } = useI18n();
  return (
    <div>
      <div className="content-head"><div><h2>{t("nav.assistants")}</h2><span className="count">{t("assistants.configuredCount", { installed: installedCount, total: assistants.length })}</span></div></div>
      <div className="grid ai-grid">
        {assistants.map(assistant => (
          <div className={`ai-card ${assistant.exists ? "ok" : "no-exist"}`} key={assistant.name}>
            <AiAvatar name={assistant.name} brand={assistant.icon} size={48} />
            <h3>{assistant.name}</h3>
            <p className="ai-path" title={(assistant.paths || [assistant.path]).join("\n")}>{formatHomePath(assistant.path)}{assistant.paths?.length > 1 ? ` · ${t("assistants.directories", { count: assistant.paths.length })}` : ""}</p>
            <div className={`ai-status ${assistant.exists ? "ok" : "no"}`}><Icon name={assistant.exists ? "check" : "x"} size={14} />{t(assistant.exists ? "assistants.configured" : "assistants.notConfigured")}</div>
            {assistant.exists && assistant.skills.length > 0 && (
              <div className="ai-skill-tags">
                {assistant.skills.slice(0, 3).map(skill => <span key={skill.path || skill.name} className="ai-skill-tag">{skill.name}</span>)}
                {assistant.skills.length > 3 && <span className="ai-skill-tag more">+{assistant.skills.length - 3}</span>}
              </div>
            )}
            {assistant.exists && assistant.skills.length === 0 && <div className="ai-empty-hint">{t("assistants.noSkills")}</div>}
            {Array.isArray(assistant.diagnostics) && assistant.diagnostics.length > 0 && (
              <details className="scan-diagnostics">
                <summary>{t("assistants.diagnostics", { count: assistant.diagnostics.length })}</summary>
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

function remoteLabel(url, t) {
  if (!url) return t("source.unconfigured");
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

function probeTime(value, language, t) {
  if (!value) return t("updates.never");
  const date = new Date(Number(value));
  if (Number.isNaN(date.getTime())) return t("updates.never");
  return date.toLocaleString(language, { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });
}

function originKindLabel(kind, t) {
  if (kind === "git") return "Git";
  if (["legacy_npm", "npm"].includes(kind)) return t("source.legacyNpm");
  if (["legacy_pip", "pip"].includes(kind)) return t("source.legacyPip");
  if (kind === "local") return t("source.local");
  return t("source.unmanaged");
}

function stateText(state, t) {
  const known = new Set(["behind", "current", "failed", "diverged", "ahead_local", "local_fixed", "source_missing", "unsupported"]);
  return t(`updates.state.${known.has(state) ? state : "unknown"}`);
}

function stateTone(state) {
  if (state === "behind") return "warn";
  if (["failed", "source_missing"].includes(state)) return "error";
  if (state === "current") return "success";
  return "muted";
}

function lagText(info, t) {
  if (info.originKind === "git") return t("updates.commits", { count: info.lagCount || 0 });
  if (["legacy_npm", "legacy_pip", "npm", "pip"].includes(info.originKind)) {
    if (info.syncState === "behind") return t("updates.newVersion");
    if (info.syncState === "current") return t("updates.currentShort");
  }
  return "—";
}

function updateButtonText(info, t) {
  if (info.updating) return t(info.originKind === "git" ? "updates.updating" : "updates.syncing");
  return t(info.originKind === "git" ? "updates.updateNow" : "updates.syncNow");
}

export function UpdatesView({ skills, orderedSkills, stats, updateState, getSyncInfo, checkAll, checkOne, updateOne }) {
  const { t, language } = useI18n();
  return (
    <div>
      <div className="content-head">
        <div><h2>{t("nav.updates")}</h2><span className="count">{skills.length}</span></div>
        <div className="content-head-actions">
          <div className="update-toolbar"><span className="update-pill warn">{t("updates.pending", { count: stats.behind })}</span><span className="update-pill">{t("updates.syncable", { count: stats.syncable })}</span>{stats.failed > 0 && <span className="update-pill error">{t("updates.failed", { count: stats.failed })}</span>}</div>
          <button className="btn btn-primary btn-sm" onClick={checkAll} disabled={skills.some(skill => (updateState[skill.path] || {}).checking)}><Icon name="refresh" size={14} />{t("updates.checkAll")}</button>
        </div>
      </div>
      {skills.length === 0 ? (
        <div className="empty-state success"><div className="empty-icon"><Icon name="sparkles" size={48} /></div><h3>{t("updates.empty")}</h3><p>{t("updates.emptyHint")}</p></div>
      ) : (
        <div className="grid">
          {orderedSkills.map(skill => {
            const info = getSyncInfo(skill);
            const card = buildSkillCardView(skill, t);
            return (
              <div className="card" key={skill.path}>
                <div className="card-head"><AiAvatar name={skill.ai} brand={skill.aiIcon} size={40} /><div className="card-info"><h3>{skill.name}</h3><div className="card-tags"><span className="tag more">{card.availabilityLabel || skill.ai}</span><span className="tag more">{originKindLabel(info.originKind, t)}</span></div></div></div>
                <div className="update-meta">
                  <div><span className="label">{t("updates.source")}</span><span className="value mono">{remoteLabel(info.resolvedLocator || info.originLocator || skill.upstream_url, t)}</span></div>
                  <div><span className="label">{t("updates.current")}</span><span className="value mono">{refLabel(info.installedRef)}</span></div>
                  <div><span className="label">{t("updates.latest")}</span><span className="value mono">{refLabel(info.latestRef)}</span></div>
                  <div><span className="label">{t("updates.behind")}</span><span className={`value ${info.syncState === "behind" ? "warn" : ""}`}>{lagText(info, t)}</span></div>
                  <div><span className="label">{t("updates.status")}</span><span className={`value status ${stateTone(info.syncState)}`}>{stateText(info.syncState, t)}</span></div>
                  <div><span className="label">{t("updates.probed")}</span><span className="value">{probeTime(info.lastProbeAt, language, t)}</span></div>
                </div>
                <div className="card-actions">
                  <button className="btn btn-secondary btn-sm" onClick={() => checkOne(skill.path)} disabled={info.checking || info.updating}><Icon name="refresh" size={14} />{t(info.checking ? "common.checking" : "common.check")}</button>
                  {info.canSync && <button className="btn btn-primary btn-sm" onClick={() => updateOne(skill.path)} disabled={info.checking || info.updating}><Icon name="upload" size={14} />{updateButtonText(info, t)}</button>}
                  {!info.canSync && <span className="update-hint">{info.message || t("updates.notSupported")}</span>}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
