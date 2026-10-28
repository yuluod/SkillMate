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
import devinLogo from "../assets/brands/devin.svg";
import deepseekLogo from "../assets/brands/deepseek.png";
import zcodeLogo from "../assets/brands/zcode.png";
import kimiLogo from "../assets/brands/kimi.png";
import { useI18n } from "../lib/i18n.jsx";
import { toUserErrorMessage } from "../lib/errorMessage.mjs";
import { SurfaceHeader } from "./SurfaceHeader.jsx";
import { skillmateApi } from "../lib/skillmateApi.js";

const AI_META = {
  claude: { bg: "#d97757", src: claudeLogo, mode: "contain" },
  codex: { bg: "#111827", src: codexLogo, mode: "contain" },
  openclaw: { bg: "#08111f", src: openclawLogo, mode: "contain" },
  gemini: { bg: "#1a73e8", src: geminiLogo, mode: "contain" },
  cursor: { bg: "#111111", src: cursorLogo, mode: "contain" },
  opencode: { bg: "#ffffff", src: opencodeLogo, mode: "contain" },
  copilot: { bg: "#ffffff", src: copilotLogo, mode: "contain" },
  devin: { bg: "#ffffff", src: devinLogo, mode: "contain" },
  deepseek: { bg: "#ffffff", src: deepseekLogo, mode: "contain" },
  zcode: { bg: "#18181b", src: zcodeLogo, mode: "contain" },
  kimi: { bg: "#111111", src: kimiLogo, mode: "contain" },
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
        background: metadata.bg,
      }}
      title={name}
      aria-hidden="true"
    >
      {metadata.src ? (
        <img className={`ai-avatar-img ${metadata.mode || "contain"}`} src={metadata.src} alt="" loading="lazy" draggable="false" />
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

function skillHue(name = "") {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.codePointAt(0)) % 360;
  return hash;
}

const SkillRow = React.memo(function SkillRow({
  skill,
  card,
  checked,
  previewed,
  t,
  onToggleSelection,
  onOpenPreview,
  onUpdate,
  onEditTags,
  onOpenDirectory,
  onUnlink,
  onRemove,
}) {
  const needsAttention = card.structureTone !== "success" || card.hasManagedDrift || card.securityWarningCount > 0;
  const metaParts = [
    card.description,
    card.sourceKind !== "unknown" ? card.contentSourceLabel : "",
    skill.size,
  ].filter(Boolean);
  return (
    <div
      className={`skill-row${previewed ? " previewed" : ""}${checked ? " checked" : ""}`}
      role="row"
      tabIndex={0}
      aria-selected={previewed}
      onClick={() => onOpenPreview(skill.path)}
      onKeyDown={(event) => {
        if ((event.key === "Enter" || event.key === " ") && event.target === event.currentTarget) {
          event.preventDefault();
          onOpenPreview(skill.path);
        }
      }}
    >
      <label className="skill-select" onClick={(event) => event.stopPropagation()}>
        <input
          type="checkbox"
          checked={checked}
          onChange={() => onToggleSelection(skill.path)}
          aria-label={t("skills.select", { name: skill.name })}
        />
      </label>
      <span className="skill-logo" style={{ "--hue": String(skillHue(skill.name)) }} aria-hidden="true">{card.title.slice(0, 2)}</span>
      <div className="skill-copy" role="cell">
        <div className="skill-name">
          <span className="skill-title">{card.title}</span>
          {needsAttention && (
            <span
              className={`skill-flag ${card.securityWarningCount > 0 || card.hasManagedDrift ? "error" : "warn"}`}
              title={[card.warningSummary, card.securityWarningSummary].filter(Boolean).join(" · ")}
            />
          )}
        </div>
        <div className="skill-meta">{metaParts.join(" · ")}</div>
      </div>
      {card.hasUpdate && card.canSync && (
        <button
          className="skill-update"
          title={card.updateSummary}
          disabled={card.updateBusy}
          onClick={(event) => { event.stopPropagation(); onUpdate(skill.path); }}
        >
          <Icon name="download" size={12} />{t("skills.updateAction")}
        </button>
      )}
      <div className="skill-badges" role="cell" title={card.availabilityLabel}>
        {card.availableIn.slice(0, 4).map((assistant) => (
          <AiAvatar key={assistant.name} name={assistant.name} brand={assistant.icon} size={22} />
        ))}
        {card.availableIn.length > 4 && <span className="agent-badge-more">+{card.availableIn.length - 4}</span>}
      </div>
      <details className="registry-more skill-more" onClick={(event) => event.stopPropagation()}>
        <summary className="btn btn-ghost btn-sm" role="button" aria-label={t("skills.moreActions", { name: skill.name })}><Icon name="more" size={17} /></summary>
        <div className="registry-more-actions">
          <button className="btn btn-ghost btn-sm" onClick={() => onEditTags(skill)} aria-label={t("skills.editTags", { name: skill.name })}><Icon name="tag" size={15} />{t("skills.tagsAction")}</button>
          <button className="btn btn-ghost btn-sm" onClick={() => onOpenDirectory(skill.path)} aria-label={t("skills.openFolder", { name: skill.name })}><Icon name="folder" size={15} />{t("skills.folderAction")}</button>
          {card.canUnlink ? (
            <button className="btn btn-ghost btn-sm danger" onClick={() => onUnlink(skill.path, skill.name)} aria-label={t("skills.unlink", { name: skill.name })}><Icon name="unlink" size={15} />{t("skills.unlinkAction")}</button>
          ) : card.canDelete ? (
            <button className="btn btn-ghost btn-sm danger" onClick={() => onRemove(skill.path, skill.name, card.availableIn)} aria-label={t("skills.remove", { name: skill.name })}><Icon name="trash" size={15} />{t("skills.removeAction")}</button>
          ) : null}
        </div>
      </details>
    </div>
  );
});

function SkillDetailPane({ skill, card, tags, t, onClose, onPreview, onUpdate, onEnable, onAdopt, onEditTags }) {
  if (!skill || !card) return null;
  return (
    <aside className="skill-pane" aria-label={t("skills.detailLabel", { name: card.title })}>
      <div className="skill-pane-head">
        <span className="skill-logo" style={{ "--hue": String(skillHue(skill.name)) }} aria-hidden="true">{card.title.slice(0, 2)}</span>
        <div className="skill-pane-id">
          <h2>{card.title}</h2>
          <span className="skill-pane-path" title={skill.path}>{formatHomePath(skill.path)}</span>
        </div>
        <button className="btn btn-ghost btn-sm skill-pane-close" onClick={onClose} aria-label={t("skills.detailClose")}><Icon name="x" size={15} /></button>
      </div>
      <div className="skill-pane-block">
        <div className="skill-pane-label">{t("skills.detailStatus")}</div>
        <div className="skill-pane-chips">
          <span className={`stamp ${card.structureTone}`}>{card.structureLabel}</span>
          {card.isShared && <span className="stamp muted">{t("skills.shared", { count: card.availableIn.length })}</span>}
          {card.hasManagedDrift && <span className="stamp error">{t("skills.changed")}</span>}
          {card.securityWarningCount > 0 && <span className="stamp error" title={card.securityWarningSummary}>{t("skills.risk", { count: card.securityWarningCount })}</span>}
          {card.hasUpdate && <span className="stamp warn">{t("skills.updateReady")}</span>}
        </div>
        {card.updateSummary && <p className="skill-pane-note">{card.updateSummary}</p>}
      </div>
      <div className="skill-pane-block">
        <div className="skill-pane-label">{t("skills.detailSource")}</div>
        <div className="skill-pane-kv">
          <span>{t("provenance.sourceLabel")}</span><span>{card.contentSourceLabel}</span>
          <span>{t("provenance.managerLabel")}</span><span>{card.managerLabel}</span>
          <span>{t("provenance.updateLabel")}</span><span>{card.updateStrategyLabel}</span>
        </div>
      </div>
      <div className="skill-pane-block">
        <div className="skill-pane-label">{t("skills.detailAgents")}</div>
        <div className="skill-pane-agents">
          {card.availableIn.length === 0 ? (
            <span className="skill-meta">{t("skills.detailNoAgents")}</span>
          ) : card.availableIn.map((assistant) => (
            <span className="skill-pane-agent" key={assistant.name}>
              <AiAvatar name={assistant.name} brand={assistant.icon} size={20} />
              {assistant.name}
              <Icon name="check" size={13} />
            </span>
          ))}
        </div>
        {skill.tags.length > 0 && (
          <div className="skill-pane-tags">
            {skill.tags.slice(0, 4).map((tagId) => {
              const tag = tags.find((item) => item.id === tagId);
              return tag ? <span key={tag.id} className="tag" style={{ "--c": tag.color }}>{tag.name}</span> : null;
            })}
          </div>
        )}
      </div>
      <div className="skill-pane-foot">
        <button className="btn btn-secondary btn-sm" onClick={() => onEditTags(skill)} aria-label={t("skills.editTags", { name: skill.name })}><Icon name="tag" size={14} />{t("skills.tagsAction")}</button>
        <button className="btn btn-secondary btn-sm" onClick={() => onPreview(skill.path)} aria-label={t("skills.reviewOne", { name: skill.name })}><Icon name="shield" size={14} />{t("skills.review")}</button>
        {card.hasUpdate && card.canSync && <button className="btn btn-secondary btn-sm" disabled={card.updateBusy} onClick={() => onUpdate(skill.path)} aria-label={t("skills.updateAction")}><Icon name="download" size={14} />{t("skills.updateAction")}</button>}
        {card.canAdopt && <button className="btn btn-secondary btn-sm" onClick={() => onAdopt({ skill, assistant: skill.ai, projectPath: "" })} aria-label={t("adoption.actionFor", { name: skill.name })}><Icon name="branch" size={14} />{t("adoption.action")}</button>}
        {card.canEnable && <button className="btn btn-primary btn-sm" onClick={() => onEnable(skill)} aria-label={t("skills.enable", { name: skill.name })}><Icon name="power" size={14} />{t("skills.enableAction")}</button>}
      </div>
    </aside>
  );
}

export function SkillsView({
  skills,
  allSkills,
  allSkillCount,
  selectedTagCount,
  tags,
  selectedTagIds = [],
  onInstall,
  onToggleDiscovery = () => {},
  discoveryOpen = false,
  discovery = null,
  onClearFilters,
  onEditTags,
  onOpenDirectory,
  onPreview,
  onEnable = () => {},
  onAdopt = () => {},
  onUnlink,
  onRemove,
  selectedSkillPaths = [],
  onToggleSelection = () => {},
  onToggleVisibleSelection = () => {},
  onClearSelection = () => {},
  onToggleTag = () => {},
  onClearTags,
  onManageTags,
  filterRequest,
  activeScenario,
  onClearScenario,
  onUpdate = () => {},
  getSyncInfo,
}) {
  const { t } = useI18n();
  const [statusFilter, setStatusFilter] = React.useState("all");
  const [previewPath, setPreviewPath] = React.useState("");

  React.useEffect(() => {
    if (filterRequest?.filter) setStatusFilter(filterRequest.filter);
  }, [filterRequest]);
  const selectedSkills = React.useMemo(
    () => (allSkills ?? skills).filter((skill) => selectedSkillPaths.includes(skill.path)),
    [allSkills, skills, selectedSkillPaths]
  );
  const canEnableSelection = React.useMemo(
    () => selectedSkills.every((skill) => buildSkillCardView(skill, t).canEnable),
    [selectedSkills, t]
  );
  const skillCards = React.useMemo(
    () => skills.map((skill) => buildSkillCardView(skill, t, getSyncInfo?.(skill))),
    [skills, t, getSyncInfo]
  );
  const totalSkillCount = allSkillCount ?? skills.length;
  const attentionCount = React.useMemo(
    () => skillCards.filter((card) => card.structureTone !== "success" || card.hasManagedDrift || card.securityWarningCount > 0).length,
    [skillCards]
  );
  const updatesCount = React.useMemo(() => skillCards.filter((card) => card.hasUpdate).length, [skillCards]);
  const visibleSkills = React.useMemo(() => {
    if (statusFilter === "attention") {
      return skills.filter((_, index) => skillCards[index].structureTone !== "success" || skillCards[index].hasManagedDrift || skillCards[index].securityWarningCount > 0);
    }
    if (statusFilter === "updates") return skills.filter((_, index) => skillCards[index].hasUpdate);
    return skills;
  }, [skills, skillCards, statusFilter]);
  const previewSkill = React.useMemo(
    () => (allSkills ?? skills).find((skill) => skill.path === previewPath) ?? null,
    [allSkills, skills, previewPath]
  );
  const previewCard = React.useMemo(
    () => (previewSkill ? buildSkillCardView(previewSkill, t, getSyncInfo?.(previewSkill)) : null),
    [previewSkill, t, getSyncInfo]
  );
  const allVisibleSelected = visibleSkills.length > 0 && visibleSkills.every((skill) => selectedSkillPaths.includes(skill.path));
  return (
    <div className="view-shell skills-view">
      <SurfaceHeader
        title={t("skills.title")}
        description={t("skills.subtitle")}
        meta={t(skills.length !== totalSkillCount ? "skills.countFiltered" : "skills.count", { count: skills.length, total: totalSkillCount })}
        actions={(
          <>
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
            <button className="btn btn-secondary btn-sm" disabled={!canEnableSelection} onClick={() => onEnable(selectedSkills)}><Icon name="power" size={14} />{t("skills.enableMany")}</button>
            <button className="btn btn-secondary btn-sm" onClick={() => onEditTags(selectedSkills)}><Icon name="tag" size={14} />{t("skills.addTags")}</button>
            <button className="btn btn-ghost btn-sm" onClick={onClearSelection}>{t("common.cancel")}</button>
          </div>
          {!canEnableSelection && <span>{t("skills.enableLibraryOnly")}</span>}
        </div>
      )}
      <div className="skills-shell">
        <div className="skills-main">
          <div className="skills-filters">
            <button className={`filter-chip${statusFilter === "all" ? " active" : ""}`} aria-pressed={statusFilter === "all"} onClick={() => setStatusFilter("all")}>{t("skills.filterAll")}<span>{skills.length}</span></button>
            <button className={`filter-chip danger${statusFilter === "attention" ? " active" : ""}`} aria-pressed={statusFilter === "attention"} onClick={() => setStatusFilter("attention")}>{t("skills.filterNeedsAttention")}<span>{attentionCount}</span></button>
            <button className={`filter-chip warn${statusFilter === "updates" ? " active" : ""}`} aria-pressed={statusFilter === "updates"} onClick={() => setStatusFilter("updates")}>{t("skills.filterUpdates")}<span>{updatesCount}</span></button>
            <span className="filter-divider" aria-hidden="true" />
            {tags.map((tag) => (
              <button
                key={tag.id}
                className={`filter-chip tag${selectedTagIds.includes(tag.id) ? " active" : ""}`}
                style={{ "--c": tag.color }}
                aria-pressed={selectedTagIds.includes(tag.id)}
                onClick={() => onToggleTag(tag.id)}
              >
                <span className="filter-dot" aria-hidden="true" />{tag.name}
              </button>
            ))}
            {activeScenario && (
              <button
                className="filter-chip active"
                onClick={onClearScenario}
                title={t("scenarioFilter.title", { name: activeScenario.name })}
                aria-label={t("scenarioFilter.clear")}
              >
                <Icon name="scenarios" size={13} />
                {activeScenario.name}
                <span>{activeScenario.skill_ids.length}</span>
                <Icon name="x" size={12} />
              </button>
            )}
            {selectedTagCount > 0 && onClearTags && <button className="btn btn-ghost btn-sm" onClick={onClearTags}>{t("common.clear")}</button>}
            <span className="filter-spacer" aria-hidden="true" />
            {onManageTags && <button className="btn btn-ghost btn-sm" onClick={onManageTags}><Icon name="tag" size={14} />{t("tags.manage")}</button>}
            <button className="btn btn-ghost btn-sm" onClick={() => onToggleVisibleSelection(visibleSkills.map((skill) => skill.path))} aria-label={t(allVisibleSelected ? "skills.deselectVisible" : "skills.selectVisible")}>{t(allVisibleSelected ? "skills.deselectVisible" : "skills.selectVisible")}</button>
          </div>
          {visibleSkills.length === 0 ? (
            <div className="empty-state">
              <div className="empty-icon"><Icon name="skills" size={22} /></div>
              <h3>{t(totalSkillCount > 0 ? "skills.noMatch" : "skills.empty")}</h3>
              <p>{t(totalSkillCount > 0 ? "skills.noMatchHint" : "skills.emptyHint")}</p>
              <div className="empty-actions">
                {totalSkillCount === 0 ? (
                  <button className="btn btn-primary btn-sm" onClick={onInstall}><Icon name="plus" size={14} />{t("common.install")}</button>
                ) : statusFilter !== "all" ? (
                  <button className="btn btn-secondary btn-sm" onClick={() => setStatusFilter("all")}>{t("skills.clearFilters")}</button>
                ) : (
                  <button className="btn btn-secondary btn-sm" onClick={onClearFilters}><Icon name="x" size={14} />{t("skills.clearFilters")}</button>
                )}
              </div>
            </div>
          ) : (
            <div className="skill-list" role="table" aria-label={t("skills.title")}>
              {visibleSkills.map((skill) => {
                const index = skills.indexOf(skill);
                return (
                  <SkillRow
                    key={`${skill.path}-${skill.name}`}
                    skill={skill}
                    card={skillCards[index]}
                    checked={selectedSkillPaths.includes(skill.path)}
                    previewed={skill.path === previewPath}
                    t={t}
                    onToggleSelection={onToggleSelection}
                    onOpenPreview={setPreviewPath}
                    onUpdate={onUpdate}
                    onEditTags={onEditTags}
                    onOpenDirectory={onOpenDirectory}
                    onUnlink={onUnlink}
                    onRemove={onRemove}
                  />
                );
              })}
            </div>
          )}
        </div>
        <SkillDetailPane
          skill={previewSkill}
          card={previewCard}
          tags={tags}
          t={t}
          onClose={() => setPreviewPath("")}
          onPreview={onPreview}
          onUpdate={onUpdate}
          onEnable={onEnable}
          onAdopt={onAdopt}
          onEditTags={onEditTags}
        />
      </div>
    </div>
  );
}

export function AssistantsView({ assistants, installedCount, onAdopt, onMaterialize, projectRevision = 0, workspace, toolbar }) {
  const { t } = useI18n();
  const [expandedAssistant, setExpandedAssistant] = React.useState(null);
  return (
    <div className="view-shell">
      <SurfaceHeader
        title={t("nav.assistants")}
        description={t("assistants.subtitle")}
        meta={t("assistants.configuredCount", { installed: installedCount, total: assistants.length })}
      />
      {toolbar}
      <ProjectInspectionPanel onAdopt={onAdopt} onMaterialize={onMaterialize} revision={projectRevision} workspace={workspace} />
      <div
        className="skill-list assistant-list"
        role={assistants.length > 0 ? "table" : undefined}
        aria-label={assistants.length > 0 ? t("nav.assistants") : undefined}
      >
        {assistants.length === 0 ? (
          <div className="empty-state">
            <div className="empty-icon"><Icon name="skills" size={22} /></div>
            <h3>{t("assistants.empty")}</h3>
            <p>{t("assistants.emptyHint")}</p>
          </div>
        ) : assistants.map((assistant, index) => {
          const expanded = expandedAssistant === assistant.name;
          const paths = assistant.paths?.length ? assistant.paths : [assistant.path].filter(Boolean);
          const hasDetails = paths.length > 1 || assistant.skills.length > 2;
          const detailId = `assistant-details-${index}`;
          return (
            <React.Fragment key={assistant.name}>
              <article className={`skill-row agent-row${expanded ? " previewed" : ""}`} role="row">
                <AiAvatar name={assistant.name} brand={assistant.icon} size={34} />
                <div className="skill-copy" role="cell">
                  <div className="skill-name">
                    <span className="skill-title">{assistant.name}</span>
                  </div>
                  <div className="skill-meta" title={paths.join("\n")}>
                    {[
                      formatHomePath(assistant.path),
                      ...(paths.length > 1 ? [t("assistants.directories", { count: paths.length })] : []),
                      t("assistants.skillCount", { count: assistant.exists ? assistant.skills.length : 0 }),
                    ].join(" · ")}
                  </div>
                </div>
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
                <span className={`stamp ${assistant.exists ? "success" : "muted"}`} role="cell">{t(assistant.exists ? "assistants.configured" : "assistants.notConfigured")}</span>
                {hasDetails && (
                  <button
                    className="row-chevron"
                    type="button"
                    aria-expanded={expanded}
                    aria-controls={detailId}
                    aria-label={t(expanded
                      ? "assistants.hideDetails"
                      : assistant.skills.length > 2
                        ? "assistants.showAllSkills"
                        : "assistants.showDetails", { count: assistant.skills.length })}
                    onClick={() => setExpandedAssistant(current => current === assistant.name ? null : assistant.name)}
                  >
                    <Icon name="arrow" size={14} className={expanded ? "expanded" : ""} />
                  </button>
                )}
              </article>
              {expanded && (
                <div className="assistant-detail-panel" role="row">
                  <div id={detailId} className="assistant-detail-content" role="cell">
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

function ProjectInspectionPanel({ onAdopt, onMaterialize, revision, workspace }) {
  const { t } = useI18n();
  const [projectPath, setProjectPath] = React.useState("");
  const [inspection, setInspection] = React.useState(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState("");
  const [expanded, setExpanded] = React.useState("");
  const [request, setRequest] = React.useState(0);
  const [foldOpen, setFoldOpen] = React.useState(false);
  const selectedPath = React.useRef("");
  const rememberRef = React.useRef(workspace?.remember);
  rememberRef.current = workspace?.remember;

  React.useEffect(() => {
    const project = workspace?.path || selectedPath.current;
    if (!project) return;
    let cancelled = false;
    setBusy(true);
    setError("");
    setInspection(null);
    setProjectPath(project);
    skillmateApi.inventory.inspectProject(project).then((result) => {
      if (!cancelled) {
        setInspection(result);
        setExpanded("");
        rememberRef.current?.(result.project_path);
      }
    }).catch((reason) => {
      if (!cancelled) {
        setInspection(null);
        setError(toUserErrorMessage(reason, t("error.safeRetry")));
      }
    }).finally(() => {
      if (!cancelled) setBusy(false);
    });
    return () => { cancelled = true; };
  }, [revision, request, workspace?.path, workspace?.request]);

  function inspect() {
    if (!projectPath.trim()) return;
    if (workspace) workspace.select(projectPath);
    else { selectedPath.current = projectPath.trim(); setRequest(value => value + 1); }
  }

  return (
    <section className="project-inspector" aria-labelledby="project-inspector-title">
      <button className="inspector-fold" type="button" aria-expanded={foldOpen} onClick={() => setFoldOpen(current => !current)}>
        <Icon name="search" size={14} />
        <strong id="project-inspector-title">{t("projectInspection.title")}</strong>
        <span className="inspector-fold-sub">{workspace?.path ? formatHomePath(workspace.path) : t("projectInspection.hint")}</span>
        <Icon name="arrow" size={13} className={foldOpen ? "expanded" : ""} />
      </button>
      {foldOpen && (<>
      <div className="project-inspector-head">
        <div><p>{t("projectInspection.hint")}</p></div>
        <div className="project-inspector-input">
          {!workspace && <>
            <label className="visually-hidden" htmlFor="project-inspection-path">{t("projectInspection.path")}</label>
            <input id="project-inspection-path" value={projectPath} onChange={(event) => setProjectPath(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void inspect(); }} placeholder={t("projectInspection.placeholder")} />
          </>}
          <button className="btn btn-primary btn-sm" onClick={inspect} disabled={busy || !projectPath.trim()}><Icon name="search" size={14} />{t(busy ? "projectInspection.inspecting" : workspace ? "common.refresh" : "projectInspection.inspect")}</button>
        </div>
      </div>
      {error && <div className="install-compact error" role="alert"><strong>{t("projectInspection.failed")}</strong><span>{error}</span></div>}
      <p className="empty-hint">{t("projectInspection.inferred")}</p>
      {inspection && (
        <div className="project-agent-results">
          {inspection.assistants.map((assistant) => {
            const open = expanded === assistant.name;
            return <article className={`project-agent-result ${open ? "expanded" : ""}`} key={assistant.name}>
              <button className="project-agent-summary" type="button" aria-expanded={open} onClick={() => setExpanded(open ? "" : assistant.name)}>
                <AiAvatar name={assistant.name} brand={assistant.icon} size={30} />
                <span><strong>{assistant.name}</strong><small>{t("projectInspection.counts", { total: assistant.skills.length, project: assistant.project_count, global: assistant.global_count })}</small></span>
                {assistant.shadowed_count > 0 && <span className="stamp warn">{t("projectInspection.shadowed", { count: assistant.shadowed_count })}</span>}
                <Icon name="arrow" size={14} className={open ? "expanded" : ""} />
              </button>
              {open && <div className="project-effective-list">
                {assistant.skills.length === 0 && <p className="empty-hint">{t("projectInspection.empty")}</p>}
                {assistant.skills.map((entry) => {
                  const card = buildSkillCardView(entry, t);
                  return <div className="project-effective-row" key={`${entry.scope}-${entry.path}`}>
                    <span className={`stamp ${entry.scope === "project" ? "success" : "muted"}`}>{t(`projectInspection.scope.${entry.scope}`)}</span>
                    <span className="project-effective-copy"><strong>{entry.manifest_title || entry.name}</strong><small title={entry.path}>{formatHomePath(entry.path)}</small></span>
                    <span className="project-effective-owner">{card.managerLabel}</span>
                    {card.canAdopt && <button className="btn btn-secondary btn-sm" onClick={() => onAdopt?.({ skill: entry, assistant: assistant.name, projectPath: entry.scope === "project" ? inspection.project_path : "" })}><Icon name="branch" size={13} />{t("adoption.action")}</button>}
                    {card.canMaterialize && <button className="btn btn-secondary btn-sm" disabled={busy} onClick={() => onMaterialize?.({ skill: entry, assistant: assistant.name, projectPath: inspection.project_path, operation: "materialize" })} aria-label={t("materialization.actionFor", { name: entry.name })}><Icon name="copy" size={13} />{t("materialization.action")}</button>}
                  </div>;
                })}
              </div>}
            </article>;
          })}
        </div>
      )}
      </>)}
    </section>
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
  return t("updatePreview.action");
}

const UpdateRow = React.memo(function UpdateRow({ skill, getSyncInfo, t, language, checkOne, updateOne }) {
  const info = getSyncInfo(skill);
  const metaParts = [
    remoteLabel(info.resolvedLocator || info.originLocator || skill.upstream_url, t),
    lagText(info, t),
    `${t("updates.probed")} ${probeTime(info.lastProbeAt, language, t)}`,
  ].filter(Boolean);
  return (
    <article className="skill-row update-row" role="row">
      <span className="skill-logo" style={{ "--hue": String(skillHue(skill.name)) }} aria-hidden="true">{skill.name.slice(0, 2)}</span>
      <div className="skill-copy" role="cell">
        <div className="skill-name">
          <span className="skill-title">{skill.name}</span>
          <span className={`stamp stamp-source ${sourceStampClass(info.originKind)}`}>{originKindLabel(info.originKind, t)}</span>
        </div>
        <div className="skill-meta" title={info.resolvedLocator || info.originLocator || skill.upstream_url}>{metaParts.join(" · ")}</div>
      </div>
      <div className="registry-refs" aria-label={`${t("updates.current")} → ${t("updates.latest")}`} role="cell">
        <span>{refLabel(info.installedRef)}</span>
        <span className="registry-refs-arrow" aria-hidden="true">→</span>
        <span className={info.syncState === "behind" ? "warn" : ""}>{refLabel(info.latestRef)}</span>
      </div>
      <div className="skill-badges" role="cell">
        <span className={`stamp ${stateTone(info.syncState)}`}>{stateText(info.syncState, t)}</span>
      </div>
      <div className="skill-badges" role="cell">
        <button className="btn btn-secondary btn-sm" onClick={() => checkOne(skill.path)} disabled={!info.canCheck || info.checking || info.updating}><Icon name="refresh" size={14} />{t(info.checking ? "common.checking" : "common.check")}</button>
        {info.canSync && <button className="btn btn-primary btn-sm" onClick={() => updateOne(skill.path)} disabled={info.checking || info.updating}><Icon name="download" size={14} />{updateButtonText(info, t)}</button>}
        {!info.canSync && (
          <span className="update-hint" title={skill.managed_by_app ? (info.message || t("updates.notSupported")) : t("updates.externalManaged")}>
            {skill.managed_by_app ? (info.message || t("updates.notSupported")) : t("updates.externalManaged")}
          </span>
        )}
      </div>
    </article>
  );
});

export function UpdatesView({
  orderedSkills,
  updateState,
  getSyncInfo,
  checkAll,
  checkOne,
  updateOne,
  tags = [],
  selectedTagCount = 0,
  selectedTagIds = [],
  onToggleTag = () => {},
  onClearTags,
  onManageTags,
  activeScenario,
  onClearScenario,
}) {
  const { t, language } = useI18n();
  const [statusFilter, setStatusFilter] = React.useState("all");
  const pendingRows = React.useMemo(
    () => orderedSkills
      .map((skill) => ({ skill, tone: stateTone(getSyncInfo(skill).syncState) }))
      .filter((row) => row.tone !== "success"),
    [orderedSkills, getSyncInfo]
  );
  const countByTone = React.useMemo(() => ({
    all: pendingRows.length,
    warn: pendingRows.filter((row) => row.tone === "warn").length,
    error: pendingRows.filter((row) => row.tone === "error").length,
  }), [pendingRows]);
  const visibleSkills = React.useMemo(
    () => (statusFilter === "all" ? pendingRows : pendingRows.filter((row) => row.tone === statusFilter))
      .map((row) => row.skill),
    [pendingRows, statusFilter]
  );
  return (
    <div className="view-shell skills-view">
      <SurfaceHeader
        title={t("nav.updates")}
        description={t("updates.subtitle")}
        meta={pendingRows.length}
        actions={<button className="btn btn-primary btn-sm" onClick={checkAll} disabled={Object.values(updateState).some(state => state.checking || state.updating)}><Icon name="refresh" size={14} />{t("updates.checkAll")}</button>}
      />
      <div className="skills-filters">
        <button className={`filter-chip${statusFilter === "all" ? " active" : ""}`} aria-pressed={statusFilter === "all"} onClick={() => setStatusFilter("all")}>{t("skills.filterAll")}<span>{countByTone.all}</span></button>
        <button className={`filter-chip warn${statusFilter === "warn" ? " active" : ""}`} aria-pressed={statusFilter === "warn"} onClick={() => setStatusFilter("warn")}>{t("updates.filterBehind")}<span>{countByTone.warn}</span></button>
        <button className={`filter-chip danger${statusFilter === "error" ? " active" : ""}`} aria-pressed={statusFilter === "error"} onClick={() => setStatusFilter("error")}>{t("updates.filterFailed")}<span>{countByTone.error}</span></button>
        {tags.length > 0 && <span className="filter-divider" aria-hidden="true" />}
        {tags.map((tag) => (
          <button
            key={tag.id}
            className={`filter-chip tag${selectedTagIds.includes(tag.id) ? " active" : ""}`}
            style={{ "--c": tag.color }}
            aria-pressed={selectedTagIds.includes(tag.id)}
            onClick={() => onToggleTag(tag.id)}
          >
            <span className="filter-dot" aria-hidden="true" />{tag.name}
          </button>
        ))}
        {activeScenario && (
          <button
            className="filter-chip active"
            onClick={onClearScenario}
            title={t("scenarioFilter.title", { name: activeScenario.name })}
            aria-label={t("scenarioFilter.clear")}
          >
            <Icon name="scenarios" size={13} />
            {activeScenario.name}
            <span>{activeScenario.skill_ids.length}</span>
            <Icon name="x" size={12} />
          </button>
        )}
        {selectedTagCount > 0 && onClearTags && <button className="btn btn-ghost btn-sm" onClick={onClearTags}>{t("common.clear")}</button>}
        <span className="filter-spacer" aria-hidden="true" />
        {onManageTags && <button className="btn btn-ghost btn-sm" onClick={onManageTags}><Icon name="tag" size={14} />{t("tags.manage")}</button>}
      </div>
      {pendingRows.length === 0 ? (
        <div className="empty-state">
          <div className="empty-icon"><Icon name="updates" size={22} /></div>
          <h3>{t("updates.empty")}</h3>
          <p>{t("updates.emptyHint")}</p>
        </div>
      ) : visibleSkills.length === 0 ? (
        <div className="empty-state">
          <div className="empty-icon"><Icon name="search" size={22} /></div>
          <h3>{t("skills.noMatch")}</h3>
          <p>{t("skills.noMatchHint")}</p>
          <div className="empty-actions">
            <button className="btn btn-secondary btn-sm" onClick={() => setStatusFilter("all")}>{t("skills.clearFilters")}</button>
          </div>
        </div>
      ) : (
        <div className="skill-list" role="table" aria-label={t("nav.updates")}>
          {visibleSkills.map((skill) => (
            <UpdateRow
              key={skill.path}
              skill={skill}
              getSyncInfo={getSyncInfo}
              t={t}
              language={language}
              checkOne={checkOne}
              updateOne={updateOne}
            />
          ))}
        </div>
      )}
    </div>
  );
}
