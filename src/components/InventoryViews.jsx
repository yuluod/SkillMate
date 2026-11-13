import React from "react";
import Icon from "./Icon.jsx";
import { buildSkillCardView, isStaleSkill } from "../lib/skillmate.mjs";
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
import { SurfaceHeader, SurfaceSectionHeader } from "./SurfaceHeader.jsx";
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

function skillLocations(skill, card) {
  if (Array.isArray(skill.deployments) && skill.deployments.length > 0) return skill.deployments;
  if (skill.in_library) return [];
  return card.availableIn.map((assistant) => ({ ...assistant, scope: skill.scope, projectPath: skill.project_path }));
}

function locationScope(location, t) {
  if (location.scope === "project") {
    const name = location.projectPath?.split(/[\\/]/).filter(Boolean).pop();
    return name ? t("skills.projectLocation", { name }) : t("projectInspection.scope.project");
  }
  return location.scope === "global" ? t("projectInspection.scope.global") : t("skills.discoveredLocation");
}

const SkillRow = React.memo(function SkillRow({
  skill,
  card,
  checked,
  previewed,
  searchSnippet,
  t,
  onToggleSelection,
  onOpenPreview,
  onUpdate,
  onEditTags,
  onOpenDirectory,
  onUnlink,
  onRemove,
}) {
  const locations = skillLocations(skill, card);
  const status = card.needsAttention
    ? isStaleSkill(skill) ? t("skills.stale") : t("skills.filterNeedsAttention")
    : card.hasUpdate ? t("skills.updateReady")
      : skill.in_library ? t("skills.inLibrary") : card.managerLabel;
  // 统一清单行：单位置时行级停用指向该部署路径；多位置收进详情栏按位置停用。
  const singleDeployment = Array.isArray(skill.deployments) && skill.deployments.length === 1
    ? skill.deployments[0]
    : null;
  return (
    <div
      className={`skill-row library-row${previewed ? " previewed" : ""}${checked ? " checked" : ""}`}
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
      <span className="library-skill-icon" aria-hidden="true"><Icon name="box" size={19} /></span>
      <div className="skill-copy" role="cell">
        <div className="skill-name">
          <span className="skill-title">{card.title}</span>
        </div>
        <div className="skill-meta" title={card.description}>{card.description}</div>
        {searchSnippet && (
          <div className="skill-search-hit" title={searchSnippet.snippet}>
            <span className="stamp warn">{t("skills.bodyMatch")}</span>
            <code>{searchSnippet.file}:{searchSnippet.line}</code>
            <span className="skill-search-snippet">{searchSnippet.snippet}</span>
            {searchSnippet.matches > 1 && <small>{t("skills.bodyMatchMore", { count: searchSnippet.matches - 1 })}</small>}
          </div>
        )}
      </div>
      <div className={`library-location${locations.length === 0 ? " empty" : ""}`} role="cell" title={locations.map((location) => `${location.name} · ${locationScope(location, t)}`).join("\n")}>
        <span>{locations.length > 1 ? t("skills.locationCount", { count: locations.length }) : locations[0]?.name || t("skills.filterNotEnabled")}</span>
        {locations.length > 0 && <small>{locations.length === 1 ? locationScope(locations[0], t) : [...new Set(locations.map((location) => location.name))].join(" · ")}</small>}
      </div>
      <div className={`library-status${card.needsAttention ? " attention" : card.hasUpdate ? " update" : ""}`} role="cell">
      {card.hasUpdate && card.canSync ? (
        <button
          className="skill-update"
          title={card.updateSummary}
          disabled={card.updateBusy}
          onClick={(event) => { event.stopPropagation(); onUpdate(skill.path); }}
        >
          <Icon name="download" size={12} />{t("skills.updateAction")}
        </button>
      ) : <span title={status}>{status}</span>}
      {card.hasUpdate && card.canSync && card.needsAttention && <small>{status}</small>}
      </div>
      <details className="registry-more skill-more" onClick={(event) => event.stopPropagation()}>
        <summary className="btn btn-ghost btn-sm" role="button" aria-label={t("skills.moreActions", { name: skill.name })}><Icon name="more" size={17} /></summary>
        <div className="registry-more-actions">
          <button className="btn btn-ghost btn-sm" onClick={() => onEditTags(skill)} aria-label={t("skills.editTags", { name: skill.name })}><Icon name="tag" size={15} />{t("skills.tagsAction")}</button>
          <button className="btn btn-ghost btn-sm" onClick={() => onOpenDirectory(skill.path)} aria-label={t("skills.openFolder", { name: skill.name })}><Icon name="folder" size={15} />{t("skills.folderAction")}</button>
          {card.canUnlink ? (
            <button className="btn btn-ghost btn-sm danger" onClick={() => onUnlink(singleDeployment ? singleDeployment.path : skill.path, skill.name)} aria-label={t("skills.unlink", { name: skill.name })}><Icon name="unlink" size={15} />{t("skills.unlinkAction")}</button>
          ) : card.canDelete ? (
            <button className="btn btn-ghost btn-sm danger" onClick={() => onRemove(skill.path, skill.name, card.availableIn)} aria-label={t("skills.remove", { name: skill.name })}><Icon name="trash" size={15} />{t("skills.removeAction")}</button>
          ) : null}
        </div>
      </details>
    </div>
  );
});

function SkillDetailPane({ skill, card, tags, t, onClose, onPreview, onUpdate, onCheck, onEnable, onAdopt, onEditTags, onUnlink, onOpenDirectory }) {
  if (!skill || !card) return null;
  const singleDeployment = skill.deployments?.length === 1 ? skill.deployments[0] : null;
  const locations = skillLocations(skill, card);
  return (
    <aside className="skill-pane library-detail" aria-label={t("skills.detailLabel", { name: card.title })}>
      <div className="library-detail-top">
        <span>{t("skills.details")}</span>
        <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label={t("skills.detailClose")}><Icon name="x" size={16} /></button>
      </div>
      <div className="skill-pane-head">
        <span className="library-skill-icon"><Icon name="box" size={24} /></span>
        <div className="skill-pane-id">
          <h2>{card.title}</h2>
          <span className="library-detail-owner">{card.managerLabel} · {card.contentSourceLabel}</span>
        </div>
      </div>
      {card.description && <p className="library-detail-description">{card.description}</p>}
      <div className="library-primary-actions">
        {card.hasUpdate && card.canSync ? (
          <button className="btn btn-primary" disabled={card.updateBusy} onClick={() => onUpdate(skill.path)}><Icon name="download" size={16} />{t("skills.viewUpdate")}</button>
        ) : card.canEnable ? (
          <button className="btn btn-primary" onClick={() => onEnable(skill)} aria-label={t("skills.enable", { name: skill.name })}><Icon name="plus" size={16} />{t("skills.enableTo")}</button>
        ) : card.canAdopt ? (
          <button className="btn btn-primary" onClick={() => onAdopt({ skill, assistant: skill.ai, projectPath: skill.scope === "project" ? skill.project_path || "" : "" })} aria-label={t("adoption.actionFor", { name: skill.name })}><Icon name="branch" size={16} />{t("adoption.action")}</button>
        ) : null}
        <button className={`btn ${card.canEnable || card.canAdopt || (card.hasUpdate && card.canSync) ? "btn-ghost" : "btn-primary"}`} onClick={() => onPreview(skill.path)} aria-label={t("skills.reviewOne", { name: skill.name })}><Icon name="preview" size={16} />{t("skills.readGuide")}</button>
      </div>
      {(card.needsAttention || card.hasUpdate || skill.manager === "skills_cli") && (
        <div className="library-detail-notice">
          {card.needsAttention && <strong>{[card.hasManagedDrift ? t("skills.changed") : "", card.securityWarningSummary, card.warningSummary, isStaleSkill(skill) ? t("skills.stale") : ""].filter(Boolean).join(" · ")}</strong>}
          {card.updateSummary && <p>{card.updateSummary}</p>}
          {skill.manager === "skills_cli" && <p><code>{t("skills.skillsCliHint", { name: skill.name })}</code></p>}
        </div>
      )}
      <section className="skill-pane-block library-detail-section">
        <div className="library-section-title"><h3>{t("skills.detailAgents")}</h3><span>{t("skills.locationCount", { count: locations.length })}</span></div>
        <div className="skill-pane-agents">
          {locations.length === 0 ? <p className="skill-pane-note">{t("skills.detailNoAgents")}</p> : locations.map((location) => (
            <div className="skill-pane-agent" key={location.path || location.name}>
              <AiAvatar name={location.name} brand={location.icon} size={28} />
              <span className="library-deployment-copy"><strong>{location.name}</strong><small title={location.projectPath || location.path}>{locationScope(location, t)}</small></span>
              {location.managed && location.isSymlink && onUnlink ? (
                <button className="skill-pane-unlink" onClick={() => onUnlink(location.path, skill.name)} aria-label={t("skills.unlinkAt", { assistant: location.name, name: card.title })}><Icon name="unlink" size={15} /></button>
              ) : <Icon name="check" size={14} />}
            </div>
          ))}
        </div>
        {card.hasUpdate && card.canSync && card.canEnable && <button className="btn btn-ghost btn-sm" onClick={() => onEnable(skill)} aria-label={t("skills.enable", { name: skill.name })}><Icon name="plus" size={14} />{t("skills.addLocation")}</button>}
      </section>
      <section className="skill-pane-block library-detail-section">
        <div className="library-section-title"><h3>{t("skills.sourceAndManager")}</h3></div>
        <div className="skill-pane-kv">
          <span>{t("provenance.sourceLabel")}</span><span>{card.contentSourceLabel}</span>
          <span>{t("provenance.managerLabel")}</span><span>{card.managerLabel}</span>
          <span>{t("provenance.updateLabel")}</span><span>{card.updateStrategyLabel}</span>
        </div>
        {skill.tags?.length > 0 && <div className="skill-pane-tags">{skill.tags.map((tagId) => {
          const tag = tags.find((item) => item.id === tagId);
          return tag ? <span key={tag.id} className="tag" style={{ "--c": tag.color }}>{tag.name}</span> : null;
        })}</div>}
      </section>
      <details className="library-technical library-detail-section">
        <summary>{t("skills.technicalDetails")}</summary>
        <div className="skill-pane-chips">
          <span className={`stamp ${card.structureTone}`}>{card.structureLabel}</span>
          {card.advisoryWarningCount > 0 && <span className="stamp muted" title={card.advisoryWarningSummary}>{t("skills.advisory", { count: card.advisoryWarningCount })}</span>}
        </div>
        <p className="skill-pane-path" title={skill.path}>{formatHomePath(skill.path)}</p>
        {card.updateSummary && !card.needsAttention && !card.hasUpdate && <p className="skill-pane-note">{card.updateSummary}</p>}
      </details>
      <div className="skill-pane-foot">
        {card.canCheck && onCheck && <button className="btn btn-ghost btn-sm" disabled={card.updateBusy} onClick={() => onCheck(skill.path)} aria-label={t("preview.checkUpdate")}><Icon name="refresh" size={14} />{t("preview.checkUpdate")}</button>}
        <button className="btn btn-ghost btn-sm" onClick={() => onEditTags(skill)} aria-label={t("skills.editTags", { name: skill.name })}><Icon name="tag" size={14} />{t("skills.tagsAction")}</button>
        <button className="btn btn-ghost btn-sm" onClick={() => onOpenDirectory(skill.path)} aria-label={t("skills.openFolder", { name: skill.name })}><Icon name="folder" size={14} />{t("skills.folderAction")}</button>
        {card.canUnlink && !card.isShared && onUnlink && <button className="btn btn-ghost btn-sm danger" onClick={() => onUnlink(singleDeployment ? singleDeployment.path : skill.path, skill.name)} aria-label={t("skills.unlink", { name: skill.name })}><Icon name="unlink" size={14} />{t("skills.unlinkAction")}</button>}
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
  onCheckAll,
  checkBusy = false,
  onCheck,
  getSyncInfo,
  searchControl,
  sortControl,
  searchSnippets = null,
  bodySearchLimited = false,
  enablePanel,
  onCloseEnable,
}) {
  const { t } = useI18n();
  const [statusFilter, setStatusFilter] = React.useState("all");
  const [previewPath, setPreviewPath] = React.useState("");
  const [detailOpen, setDetailOpen] = React.useState(false);

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
    () => skillCards.filter((card) => card.needsAttention).length,
    [skillCards]
  );
  const updatesCount = React.useMemo(() => skillCards.filter((card) => card.hasUpdate).length, [skillCards]);
  const notEnabledCount = React.useMemo(
    () => skills.filter((skill) => skill.in_library && !(Array.isArray(skill.deployments) && skill.deployments.length > 0)).length,
    [skills]
  );
  const visibleSkills = React.useMemo(() => {
    if (statusFilter === "attention") {
      return skills.filter((_, index) => skillCards[index].needsAttention);
    }
    if (statusFilter === "updates") return skills.filter((_, index) => skillCards[index].hasUpdate);
    if (statusFilter === "notEnabled") {
      return skills.filter((skill) => skill.in_library && !(Array.isArray(skill.deployments) && skill.deployments.length > 0));
    }
    return skills;
  }, [skills, skillCards, statusFilter]);
  const previewSkill = React.useMemo(
    () => previewPath === null ? null : visibleSkills.find((skill) => skill.path === previewPath) ?? visibleSkills[0] ?? null,
    [visibleSkills, previewPath]
  );
  const previewCard = React.useMemo(
    () => (previewSkill ? buildSkillCardView(previewSkill, t, getSyncInfo?.(previewSkill)) : null),
    [previewSkill, t, getSyncInfo]
  );
  const allVisibleSelected = visibleSkills.length > 0 && visibleSkills.every((skill) => selectedSkillPaths.includes(skill.path));
  return (
    <div className={`view-shell skills-view${detailOpen || enablePanel ? " detail-open" : ""}`}>
      <SurfaceHeader
        title={t("skills.title")}
        description={t("skills.subtitle")}
        actions={(
          <>
          {onCheckAll && <button className="btn btn-ghost btn-sm" disabled={checkBusy} onClick={onCheckAll} aria-label={t("updates.checkAll")}><Icon name="refresh" size={14} />{t("updates.checkAll")}</button>}
          <button className="btn btn-secondary btn-sm" aria-expanded={discoveryOpen} onClick={onToggleDiscovery}><Icon name="search" size={14} />{t(discoveryOpen ? "skills.hideDiscovery" : "skills.discover")}</button>
          <button className="btn btn-primary btn-sm" onClick={onInstall}><Icon name="plus" size={14} />{t("common.install")}</button>
          </>
        )}
      />
      {discovery}
      <div className="skills-shell">
        <div className="skills-main">
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
          <div className="skills-filters">
            <div className="skills-status-filters">
            <button className={`filter-chip${statusFilter === "all" ? " active" : ""}`} aria-pressed={statusFilter === "all"} onClick={() => setStatusFilter("all")}>{t("skills.filterAll")}<span>{skills.length}</span></button>
              <button className={`filter-chip${statusFilter === "notEnabled" ? " active" : ""}`} aria-pressed={statusFilter === "notEnabled"} onClick={() => setStatusFilter("notEnabled")}>{t("skills.filterNotEnabled")}<span>{notEnabledCount}</span></button>
            <button className={`filter-chip warn${statusFilter === "updates" ? " active" : ""}`} aria-pressed={statusFilter === "updates"} onClick={() => setStatusFilter("updates")}>{t("skills.filterUpdates")}<span>{updatesCount}</span></button>
            <button className={`filter-chip danger${statusFilter === "attention" ? " active" : ""}`} aria-pressed={statusFilter === "attention"} onClick={() => setStatusFilter("attention")}>{t("skills.filterNeedsAttention")}<span>{attentionCount}</span></button>
            </div>
            <div className="skills-filter-actions">
              {onManageTags && <button className="btn btn-ghost btn-sm" onClick={onManageTags}><Icon name="tag" size={14} />{t("tags.manage")}</button>}
              <button className="btn btn-ghost btn-sm" onClick={() => onToggleVisibleSelection(visibleSkills.map((skill) => skill.path))}><Icon name="check" size={15} />{t(allVisibleSelected ? "skills.deselectVisible" : "skills.selectVisible")}</button>
            </div>
          </div>
          {(searchControl || sortControl) && <div className="library-search-row">{searchControl}{sortControl}</div>}
          {bodySearchLimited && <p className="library-search-note"><Icon name="search" size={13} />{t("skills.bodySearchLimited")}</p>}
          {(tags.length > 0 || activeScenario || selectedTagCount > 0) && <div className="skills-tag-filters">
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
          </div>}
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
            <div className="skill-list library-list" role="table" aria-label={t("skills.title")}>
              <div className="library-columns" role="row">
                <span role="columnheader">{t("skills.columnSkill")}</span>
                <span role="columnheader">{t("skills.detailAgents")}</span>
                <span role="columnheader">{t("skills.detailStatus")}</span>
              </div>
              {visibleSkills.map((skill) => {
                const index = skills.indexOf(skill);
                return (
                  <SkillRow
                    key={`${skill.path}-${skill.name}`}
                    skill={skill}
                    card={skillCards[index]}
                    checked={selectedSkillPaths.includes(skill.path)}
                    previewed={skill.path === previewSkill?.path}
                    searchSnippet={searchSnippets?.get?.(skill.path)}
                    t={t}
                    onToggleSelection={onToggleSelection}
                    onOpenPreview={(path) => { onCloseEnable?.(); setPreviewPath(path); setDetailOpen(true); }}
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
          <footer className="library-list-footer"><span>{t("skills.countFiltered", { count: visibleSkills.length, total: totalSkillCount })}</span><span><Icon name="shield" size={13} />{t("skills.readOnlyNote")}</span></footer>
        </div>
        {enablePanel || <SkillDetailPane
          skill={previewSkill}
          card={previewCard}
          tags={tags}
          t={t}
          onClose={() => { setPreviewPath(null); setDetailOpen(false); }}
          onPreview={onPreview}
          onUpdate={onUpdate}
          onCheck={onCheck}
          onEnable={onEnable}
          onAdopt={onAdopt}
          onEditTags={onEditTags}
          onUnlink={onUnlink}
          onOpenDirectory={onOpenDirectory}
        />}
        {!enablePanel && !previewSkill && totalSkillCount > 0 && <aside className="library-detail-empty"><Icon name="preview" size={24} /><p>{t("skills.selectDetail")}</p></aside>}
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
        actions={toolbar}
      />
      <ProjectInspectionPanel onAdopt={onAdopt} onMaterialize={onMaterialize} revision={projectRevision} workspace={workspace} />
      <SurfaceSectionHeader title={t("assistants.platforms")} meta={assistants.length} />
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
          const hasDetails = paths.length > 0 || assistant.skills.length > 0;
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
                    {paths.length > 0 && (
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
    setFoldOpen(true);
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
