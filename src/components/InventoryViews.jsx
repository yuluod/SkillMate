import React from "react";
import Icon from "./Icon.jsx";
import { buildSkillCardView } from "../lib/skillmate.mjs";
import claudeLogo from "../assets/brands/claude.svg";
import codexLogo from "../assets/brands/codex-openai.svg";
import openclawLogo from "../assets/brands/openclaw.svg";
import geminiLogo from "../assets/brands/gemini.svg";
import cursorLogo from "../assets/brands/cursor.svg";
import opencodeLogo from "../assets/brands/opencode.svg";
import copilotLogo from "../assets/brands/copilot.svg";
import { useI18n } from "../lib/i18n.jsx";
import { SurfaceHeader } from "./SurfaceHeader.jsx";

const AI_META = {
  claude: { bg: "#d97757", src: claudeLogo, mode: "contain" },
  codex: { bg: "#111827", src: codexLogo, mode: "contain" },
  openclaw: { bg: "#08111f", src: openclawLogo, mode: "contain" },
  gemini: { bg: "#1a73e8", src: geminiLogo, mode: "contain" },
  cursor: { bg: "#111111", src: cursorLogo, mode: "contain" },
  opencode: { bg: "#ffffff", src: opencodeLogo, mode: "contain" },
  copilot: { bg: "#ffffff", src: copilotLogo, mode: "contain" },
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

function sourceStampClass(kind) {
  if (["legacy_npm", "npm"].includes(kind)) return "npm";
  if (["legacy_pip", "pip", "pypi"].includes(kind)) return "pip";
  if (["local", "symlink", "deployment"].includes(kind)) return "local";
  if (["git", "github"].includes(kind)) return "git";
  return "unmanaged";
}

export function SkillsView({
  skills,
  allSkills,
  allSkillCount,
  selectedTagCount,
  tags,
  onInstall,
  onToggleDiscovery = () => {},
  discoveryOpen = false,
  discovery = null,
  onClearFilters,
  onEditTags,
  onOpenDirectory,
  onPreview,
  onEnable = () => {},
  onUnlink,
  onRemove,
  selectedSkillPaths = [],
  onToggleSelection = () => {},
  onToggleVisibleSelection = () => {},
  onClearSelection = () => {},
}) {
  const { t } = useI18n();
  const selectedSkills = (allSkills ?? skills).filter((skill) => selectedSkillPaths.includes(skill.path));
  const allVisibleSelected = skills.length > 0 && skills.every((skill) => selectedSkillPaths.includes(skill.path));
  return (
    <div className="view-shell">
      <SurfaceHeader
        title={t("skills.title")}
        description={t("skills.subtitle")}
        meta={t(skills.length !== allSkillCount ? "skills.countFiltered" : "skills.count", { count: skills.length, total: allSkillCount })}
        actions={(
          <>
          {selectedTagCount > 0 && <div className="filter-tag"><Icon name="tag" size={14} />{t("skills.selectedTags", { count: selectedTagCount })}</div>}
          <button className="btn btn-secondary btn-sm" aria-expanded={discoveryOpen} onClick={onToggleDiscovery}><Icon name="search" size={14} />{t(discoveryOpen ? "skills.hideDiscovery" : "skills.discover")}</button>
          <button className="btn btn-primary btn-sm" onClick={onInstall}><Icon name="plus" size={14} />{t("common.install")}</button>
          </>
        )}
      />
      {discovery}
      {selectedSkills.length > 0 && (
        <div className="bulk-toolbar" role="region" aria-label={t("skills.bulkActions")}>
          <strong>{t("skills.selected", { count: selectedSkills.length })}</strong>
          <div>
            <button className="btn btn-secondary btn-sm" onClick={() => onEditTags(selectedSkills)}><Icon name="tag" size={14} />{t("skills.addTags")}</button>
            <button className="btn btn-ghost btn-sm" onClick={onClearSelection}>{t("common.cancel")}</button>
          </div>
        </div>
      )}
      <div className="registry" role="table" aria-label={t("skills.title")}>
        <div className="registry-colhead registry-colhead-selectable" role="row">
          <label className="registry-col-entry registry-col-selectable" role="columnheader">
            <input
              type="checkbox"
              checked={allVisibleSelected}
              onChange={onToggleVisibleSelection}
              aria-label={t(allVisibleSelected ? "skills.deselectVisible" : "skills.selectVisible")}
            />
            <span className="registry-entry-label">{t("registry.entry")}</span>
            <span className="registry-selection-label" aria-hidden="true">{t(allVisibleSelected ? "skills.deselectVisible" : "skills.selectVisible")}</span>
          </label>
          <span className="registry-col-platform" role="columnheader">{t("registry.platform")}</span>
          <span className="registry-col-size" role="columnheader">{t("registry.size")}</span>
          <span className="registry-col-actions" role="columnheader">{t("registry.actions")}</span>
        </div>
        {skills.length === 0 ? (
          <div className="registry-row registry-empty" role="row">
            <div className="registry-main" role="cell">
              <div className="registry-title">
                <span className="stamp muted" aria-hidden="true">{t("registry.empty")}</span>
                <h3>{t(allSkillCount > 0 ? "skills.noMatch" : "skills.empty")}</h3>
              </div>
              <p className="registry-desc">{t(allSkillCount > 0 ? "skills.noMatchHint" : "skills.emptyHint")}</p>
            </div>
            <div className="registry-platform" role="cell" />
            <div className="registry-size" role="cell" />
            <div className="registry-actions" role="cell">
              {allSkillCount > 0 && <button className="btn btn-secondary btn-sm" onClick={onClearFilters}><Icon name="x" size={14} />{t("skills.clearFilters")}</button>}
            </div>
          </div>
        ) : (
          skills.map((skill, index) => {
            const card = buildSkillCardView(skill, t);
            const selected = selectedSkillPaths.includes(skill.path);
            const needsReview = card.structureTone !== "success" || card.hasManagedDrift || card.securityWarningCount > 0;
            return (
              <article className={`registry-row ${selected ? "selected" : ""}`} key={`${skill.path}-${skill.name}`} style={{ "--i": index }} role="row">
                <div className="registry-main registry-main-selectable" role="cell">
                  <label className="registry-select">
                    <input
                      type="checkbox"
                      checked={selected}
                      onChange={() => onToggleSelection(skill.path)}
                      aria-label={t("skills.select", { name: skill.name })}
                    />
                  </label>
                  <div className="registry-entry">
                    <div className="registry-identity">
                      <h3>{card.title}</h3>
                      {card.sourceLabel && <span className={`stamp stamp-source ${sourceStampClass(skill.source_type)}`}>{card.sourceLabel}</span>}
                    </div>
                    <div className="registry-signals">
                      <span className={`stamp ${card.structureTone}`} title={card.warningSummary}>{card.structureLabel}</span>
                      {card.isShared && <span className="stamp muted" title={card.availabilityLabel}>{t("skills.shared", { count: card.availableIn.length })}</span>}
                      {card.hasManagedDrift && <span className="stamp error">{t("skills.changed")}</span>}
                      {card.securityWarningCount > 0 && <span className="stamp error" title={card.securityWarningSummary}>{t("skills.risk", { count: card.securityWarningCount })}</span>}
                    </div>
                    {skill.tags.length > 0 && (
                      <div className="registry-tags">
                        {skill.tags.slice(0, 2).map(tagId => {
                          const tag = tags.find(item => item.id === tagId);
                          return tag ? <span key={tag.id} className="tag" style={{ "--c": tag.color }}>{tag.name}</span> : null;
                        })}
                        {skill.tags.length > 2 && <span className="tag more">+{skill.tags.length - 2}</span>}
                      </div>
                    )}
                    {card.description && <p className="registry-desc">{card.description}</p>}
                    <div className="registry-path" title={skill.path}>
                      <span>{formatHomePath(skill.path)}</span>
                      {skill.symlink_source && <span className="registry-symlink">→ {formatHomePath(skill.symlink_source)}</span>}
                    </div>
                  </div>
                </div>
                <div className="registry-platform" title={card.availabilityLabel} role="cell">
                  <AiAvatar name={skill.ai} brand={skill.aiIcon} size={16} />
                  <span>{card.availabilityLabel || skill.ai}</span>
                </div>
                <div className="registry-size" role="cell">{skill.size}</div>
                <div className="registry-actions" role="cell">
                  {card.canEnable && <button className="btn btn-primary btn-sm" onClick={() => onEnable(skill)} aria-label={t("skills.enable", { name: skill.name })}><Icon name="plus" size={15} />{t("skills.enableAction")}</button>}
                  <button className={`btn btn-sm ${needsReview ? "btn-review" : "btn-secondary"}`} onClick={() => onPreview(skill.path)} aria-label={t(needsReview ? "skills.reviewOne" : "skills.preview", { name: skill.name })}><Icon name={needsReview ? "shield" : "preview"} size={15} />{t(needsReview ? "skills.review" : "common.details")}</button>
                  <details className="registry-more">
                    <summary className="btn btn-ghost btn-sm" role="button" aria-label={t("skills.moreActions", { name: skill.name })}><Icon name="more" size={17} /></summary>
                    <div className="registry-more-actions">
                      <button className="btn btn-ghost btn-sm" onClick={() => onEditTags(skill)} aria-label={t("skills.editTags", { name: skill.name })}><Icon name="tag" size={15} />{t("skills.tagsAction")}</button>
                      <button className="btn btn-ghost btn-sm" onClick={() => onOpenDirectory(skill.path)} aria-label={t("skills.openFolder", { name: skill.name })}><Icon name="folder" size={15} />{t("skills.folderAction")}</button>
                      {card.canUnlink ? (
                        <button className="btn btn-ghost btn-sm danger" onClick={() => onUnlink(skill.path, skill.name)} aria-label={t("skills.unlink", { name: skill.name })}><Icon name="x" size={15} />{t("skills.unlinkAction")}</button>
                      ) : card.canDelete ? (
                        <button className="btn btn-ghost btn-sm danger" onClick={() => onRemove(skill.path, skill.name, card.availableIn)} aria-label={t("skills.remove", { name: skill.name })}><Icon name="trash" size={15} />{t("skills.removeAction")}</button>
                      ) : null}
                    </div>
                  </details>
                </div>
              </article>
            );
          })
        )}
      </div>
    </div>
  );
}

export function AssistantsView({ assistants, installedCount, onManageSkills }) {
  const { t } = useI18n();
  const [expandedAssistant, setExpandedAssistant] = React.useState(null);
  return (
    <div className="view-shell">
      <SurfaceHeader
        title={t("nav.assistants")}
        description={t("assistants.subtitle")}
        meta={t("assistants.configuredCount", { installed: installedCount, total: assistants.length })}
        actions={onManageSkills && <button className="btn btn-primary btn-sm" onClick={onManageSkills}><Icon name="skills" size={14} />{t("assistants.manage")}</button>}
      />
      <div className="registry assistant-registry" role="table" aria-label={t("nav.assistants")}>
        <div className="registry-colhead" role="row">
          <span role="columnheader">{t("assistants.platform")}</span>
          <span role="columnheader">{t("assistants.location")}</span>
          <span role="columnheader">{t("assistants.status")}</span>
          <span className="registry-col-actions" role="columnheader">{t("assistants.skills")}</span>
        </div>
        {assistants.length === 0 ? (
          <div className="registry-row registry-empty" role="row">
            <div className="registry-main" role="cell">
              <div className="registry-title"><span className="stamp muted">{t("registry.empty")}</span><h3>{t("assistants.empty")}</h3></div>
              <p className="registry-desc">{t("assistants.emptyHint")}</p>
            </div>
            <div className="registry-path" role="cell" />
            <div className="registry-state" role="cell" />
            <div className="registry-actions" role="cell" />
          </div>
        ) : assistants.map((assistant, index) => {
          const expanded = expandedAssistant === assistant.name;
          const paths = assistant.paths?.length ? assistant.paths : [assistant.path].filter(Boolean);
          const hasDetails = paths.length > 1 || assistant.skills.length > 2;
          const detailId = `assistant-details-${index}`;
          return (
            <React.Fragment key={assistant.name}>
              <article className={`registry-row ${expanded ? "assistant-row-expanded" : ""}`} role="row">
                <div className="assistant-entry" role="cell">
                  <AiAvatar name={assistant.name} brand={assistant.icon} size={36} />
                  <div className="registry-entry">
                    <div className="registry-identity"><h3>{assistant.name}</h3></div>
                    {Array.isArray(assistant.diagnostics) && assistant.diagnostics.length > 0 && (
                      <details className="scan-diagnostics">
                        <summary>{t("assistants.diagnostics", { count: assistant.diagnostics.length })}</summary>
                        <ul>
                          {assistant.diagnostics.slice(0, 5).map((diagnostic, diagnosticIndex) => (
                            <li key={`${diagnostic.path}-${diagnostic.code}-${diagnosticIndex}`}>
                              <span title={diagnostic.path}>{formatHomePath(diagnostic.path)}</span>
                              <small>{diagnostic.message}</small>
                            </li>
                          ))}
                        </ul>
                      </details>
                    )}
                  </div>
                </div>
                <div className="registry-path assistant-locations" title={paths.join("\n")} role="cell">
                  <span className="assistant-location-primary">{formatHomePath(assistant.path)}</span>
                  {paths.length > 1 && <span className="assistant-location-count">{t("assistants.directories", { count: paths.length })}</span>}
                </div>
                <div className="registry-state" role="cell"><span className={`stamp ${assistant.exists ? "success" : "muted"}`}>{t(assistant.exists ? "assistants.configured" : "assistants.notConfigured")}</span></div>
                <div className="assistant-skills" role="cell">
                  {assistant.exists && assistant.skills.length > 0 ? (
                    assistant.skills.slice(0, 2).map(skill => <span key={skill.path || skill.name} className="tag">{skill.name}</span>)
                  ) : <span className="registry-desc">{t("assistants.noSkills")}</span>}
                  {hasDetails && (
                    <button
                      className="assistant-skills-toggle"
                      type="button"
                      aria-expanded={expanded}
                      aria-controls={detailId}
                      onClick={() => setExpandedAssistant(current => current === assistant.name ? null : assistant.name)}
                    >
                      {t(expanded
                        ? "assistants.hideDetails"
                        : assistant.skills.length > 2
                          ? "assistants.showAllSkills"
                          : "assistants.showDetails", { count: assistant.skills.length })}
                      <Icon name="arrow" size={13} className={expanded ? "expanded" : ""} />
                    </button>
                  )}
                </div>
              </article>
              {expanded && (
                <div className="assistant-detail-panel" role="row">
                  <div id={detailId} className="assistant-detail-content" role="cell" aria-colspan="4">
                    {paths.length > 1 && (
                      <section className="assistant-detail-section">
                        <div className="assistant-detail-head">
                          <strong>{t("assistants.locationList")}</strong>
                          <span>{t("assistants.directoryCount", { count: paths.length })}</span>
                        </div>
                        <ul className="assistant-location-list">
                          {paths.map(path => <li key={path}>{formatHomePath(path)}</li>)}
                        </ul>
                      </section>
                    )}
                    <section className="assistant-detail-section">
                      <div className="assistant-detail-head">
                        <strong>{t("assistants.skillList", { name: assistant.name })}</strong>
                        <span>{t("assistants.skillCount", { count: assistant.skills.length })}</span>
                      </div>
                      <ul className="assistant-skill-list">
                        {assistant.skills.map(skill => <li key={skill.path || skill.name}>{skill.name}</li>)}
                      </ul>
                    </section>
                  </div>
                </div>
              )}
            </React.Fragment>
          );
        })}
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
    <div className="view-shell">
      <SurfaceHeader
        title={t("nav.updates")}
        description={t("updates.subtitle")}
        meta={skills.length}
        actions={(
          <>
          <div className="update-toolbar"><span className="stamp warn">{t("updates.pending", { count: stats.behind })}</span><span className="stamp muted">{t("updates.syncable", { count: stats.syncable })}</span>{stats.failed > 0 && <span className="stamp error">{t("updates.failed", { count: stats.failed })}</span>}</div>
          <button className="btn btn-primary btn-sm" onClick={checkAll} disabled={skills.some(skill => (updateState[skill.path] || {}).checking)}><Icon name="refresh" size={14} />{t("updates.checkAll")}</button>
          </>
        )}
      />
      <div className="registry" role="table" aria-label={t("nav.updates")}>
        <div className="registry-colhead" role="row">
          <span className="registry-col-entry" role="columnheader">{t("registry.entry")}</span>
          <span className="registry-col-refs" role="columnheader">{t("updates.current")} → {t("updates.latest")}</span>
          <span className="registry-col-state" role="columnheader">{t("updates.status")}</span>
          <span className="registry-col-actions" role="columnheader">{t("registry.actions")}</span>
        </div>
        {skills.length === 0 ? (
          <div className="registry-row registry-empty" role="row">
            <div className="registry-main" role="cell">
              <div className="registry-title">
                <span className="stamp success" aria-hidden="true">{t("registry.clear")}</span>
                <h3>{t("updates.empty")}</h3>
              </div>
              <p className="registry-desc">{t("updates.emptyHint")}</p>
            </div>
            <div className="registry-refs" role="cell" />
            <div className="registry-state" role="cell" />
            <div className="registry-actions" role="cell" />
          </div>
        ) : (
          orderedSkills.map(skill => {
            const info = getSyncInfo(skill);
            const card = buildSkillCardView(skill, t);
            return (
              <article className="registry-row" key={skill.path} role="row">
                <div className="registry-main" role="cell">
                  <div className="registry-title">
                    <h3>{skill.name}</h3>
                    <span className={`stamp stamp-source ${sourceStampClass(info.originKind)}`}>{originKindLabel(info.originKind, t)}</span>
                    <span className="stamp muted">{card.availabilityLabel || skill.ai}</span>
                  </div>
                  <div className="registry-path" title={info.resolvedLocator || info.originLocator || skill.upstream_url}>
                    <span>{remoteLabel(info.resolvedLocator || info.originLocator || skill.upstream_url, t)}</span>
                  </div>
                  <div className="registry-probe">
                    <span className={info.syncState === "behind" ? "warn" : ""}>{lagText(info, t)}</span>
                    <span>{t("updates.probed")} {probeTime(info.lastProbeAt, language, t)}</span>
                  </div>
                </div>
                <div className="registry-refs" aria-label={`${t("updates.current")} → ${t("updates.latest")}`} role="cell">
                  <span>{refLabel(info.installedRef)}</span>
                  <span className="registry-refs-arrow" aria-hidden="true">→</span>
                  <span className={info.syncState === "behind" ? "warn" : ""}>{refLabel(info.latestRef)}</span>
                </div>
                <div className="registry-state" role="cell">
                  <span className={`stamp ${stateTone(info.syncState)}`}>{stateText(info.syncState, t)}</span>
                </div>
                <div className="registry-actions" role="cell">
                  <button className="btn btn-secondary btn-sm" onClick={() => checkOne(skill.path)} disabled={info.checking || info.updating}><Icon name="refresh" size={14} />{t(info.checking ? "common.checking" : "common.check")}</button>
                  {info.canSync && <button className="btn btn-primary btn-sm" onClick={() => updateOne(skill.path)} disabled={info.checking || info.updating}><Icon name="upload" size={14} />{updateButtonText(info, t)}</button>}
                  {!info.canSync && (
                    <span className="update-hint" title={skill.managed_by_app ? (info.message || t("updates.notSupported")) : t("updates.externalManaged")}>
                      {skill.managed_by_app ? (info.message || t("updates.notSupported")) : t("updates.externalManaged")}
                    </span>
                  )}
                </div>
              </article>
            );
          })
        )}
      </div>
    </div>
  );
}
