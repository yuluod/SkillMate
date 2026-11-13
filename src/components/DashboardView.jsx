import { useState } from "react";
import Icon from "./Icon.jsx";
import { skillmateApi } from "../lib/skillmateApi.js";
import { marketItemDescription } from "../lib/skillmate.mjs";
import { useI18n } from "../lib/i18n.jsx";
import { toUserErrorMessage } from "../lib/errorMessage.mjs";
import { SurfaceHeader, SurfaceSectionHeader } from "./SurfaceHeader.jsx";

function AttentionRow({ icon, tone = "", title, body, onClick, action }) {
  return (
    <button className={`dash-row ${tone}`} onClick={onClick} type="button">
      <span className={`dash-row-icon ${tone}`}><Icon name={icon} size={16} /></span>
      <span className="dash-row-copy"><strong>{title}</strong><small>{body}</small></span>
      <span className="dash-row-action">{action}<Icon name="arrow" size={14} /></span>
    </button>
  );
}

function repositoryIdentity(value = "") {
  const normalized = String(value).trim().toLowerCase().replace(/\.git(?:[#/:].*)?$/, "");
  const github = normalized.match(/github\.com[/:]([^/#]+\/[^/#]+)/);
  return (github?.[1] || normalized.split("#")[0]).replace(/\.git$/, "");
}

// 与 skill_install_source.rs 的 `<repo>#<ref>:<subdir>` 登记格式对应：
// 子目录只从首个 # 片段的首个 : 之后取，避免 SSH 与本地路径中的冒号干扰。
function locatorSkillSubdir(locator = "") {
  const fragment = String(locator).split("#")[1];
  if (!fragment) return "";
  const index = fragment.indexOf(":");
  if (index < 0) return "";
  return fragment.slice(index + 1).split("/").filter(Boolean).join("/").toLowerCase();
}

// 「已添加」= 库内存在同来源受管登记（含接管保留的原始来源）；外部只读条目不计。
// GitHub 结果以整仓为粒度，仅匹配仓库根来源；skills.sh 结果再按 skillId 比对子目录。
function isMarketSkillInstalled(item, installedSkills) {
  const repository = repositoryIdentity(item.repository || item.installSource);
  if (!repository) return false;
  const marketId = String(item.skillId || item.skill_id || "").trim().toLowerCase();
  const matchSubdir = item.source === "skills-sh";
  return installedSkills.some((skill) => {
    if (skill.origin_kind !== "git") return false;
    if (skill.managed_by_app !== true && skill.in_library !== true) return false;
    return [skill.origin_locator, skill.resolved_locator, skill.upstream_url].some((locator) => {
      const value = String(locator || "").trim();
      if (!value || repositoryIdentity(value) !== repository) return false;
      const subdir = locatorSkillSubdir(value);
      if (!matchSubdir) return subdir === "";
      if (!marketId) return false;
      return subdir === marketId
        || subdir.split("/").pop() === marketId
        || String(skill.name || "").trim().toLowerCase() === marketId
        || String(skill.manifest_title || "").trim().toLowerCase() === marketId;
    });
  });
}

function MarketResult({ item, installed, onInstall, onOpenSource }) {
  const { t } = useI18n();
  return (
    <article className="market-result">
      <div className="market-result-head">
        <div><strong>{item.name}</strong><span>{item.repository}</span></div>
        <span className="source-badge git">{item.source === "skills-sh" ? "skills.sh" : "GitHub"}</span>
      </div>
      <p>{marketItemDescription(item) || item.skillId || item.repository}</p>
      <div className="market-result-meta">
        {item.installs > 0 && <span>{t("market.installs", { count: item.installs.toLocaleString() })}</span>}
        {item.stars > 0 && <span>{t("market.stars", { count: item.stars.toLocaleString() })}</span>}
      </div>
      <div className="market-result-actions">
        <button className="btn btn-ghost btn-sm" type="button" onClick={() => onOpenSource(item)}><Icon name="external" size={14} />{t("market.open")}</button>
        <button className="btn btn-primary btn-sm" type="button" disabled={installed} onClick={() => onInstall(item)}><Icon name={installed ? "check" : "shield"} size={14} />{t(installed ? "market.added" : "market.safeInstall")}</button>
      </div>
    </article>
  );
}

export function MarketDiscovery({ onInstall, installedSkills = [] }) {
  const { t } = useI18n();
  const [source, setSource] = useState("skills-sh");
  const [query, setQuery] = useState("");
  const [market, setMarket] = useState({ loading: false, items: [], searched: false, error: "" });

  async function search(event) {
    event.preventDefault();
    const trimmed = query.trim();
    if (trimmed.length < 2) {
      setMarket({ loading: false, items: [], searched: false, error: "" });
      return;
    }
    setMarket((current) => ({ ...current, loading: true, error: "" }));
    try {
      const result = await skillmateApi.market.search(source, trimmed);
      setMarket({ loading: false, items: result.items || [], searched: true, error: "" });
    } catch (error) {
      setMarket({ loading: false, items: [], searched: true, error: t("market.error", { message: toUserErrorMessage(error, t("error.safeRetry")) }) });
    }
  }

  async function openSource(item) {
    try {
      await skillmateApi.market.openSource(item.url);
    } catch (error) {
      setMarket((current) => ({
        ...current,
        error: t("market.openError", { message: toUserErrorMessage(error, t("error.safeRetry")) }),
      }));
    }
  }

  return (
    <section className="dashboard-section market-search library-discovery">
      <div className="seclabel">{t("market.title")}</div>
      <p className="seclabel-sub">{t("market.subtitle")}</p>
      <form className="market-search-form" onSubmit={search}>
        <label><span>{t("market.source")}</span><select value={source} onChange={(event) => setSource(event.target.value)}><option value="skills-sh">skills.sh</option><option value="github">GitHub</option></select></label>
        <label className="market-query"><span className="visually-hidden">{t("common.search")}</span><Icon name="search" size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("market.placeholder")} /></label>
        <button className="btn btn-primary" disabled={market.loading || query.trim().length < 2}><Icon name="search" size={15} />{market.loading ? t("market.searching") : t("market.search")}</button>
      </form>
      <p className="market-source-note">{t(source === "skills-sh" ? "market.sourceHint.skillsSh" : "market.sourceHint.github")}</p>
      {market.error && <div className="install-compact error" role="alert"><strong>{market.error}</strong></div>}
      {!market.error && market.items.length === 0 && <div className="market-empty">{market.searched ? t("market.noResults") : t("market.empty")}</div>}
      {market.items.length > 0 && <div className="market-grid">{market.items.map((item) => <MarketResult key={item.id} item={item} installed={isMarketSkillInstalled(item, installedSkills)} onInstall={onInstall} onOpenSource={openSource} />)}</div>}
    </section>
  );
}


export default function DashboardView({ stats, tagCount, driftGroups, onNavigate, onInstall, onOpenDrift }) {
  const { t } = useI18n();

  const attentionCount = stats.updates + stats.structureIssues + stats.securityRisks
    + stats.localChanges + stats.driftGroups + stats.diagnostics + stats.stale;

  return (
    <div className="dashboard view-shell">
      <SurfaceHeader
        title={t("dashboard.title")}
        description={t("dashboard.subtitle")}
        actions={(
          <button className="btn btn-primary btn-sm" type="button" onClick={onInstall}><Icon name="plus" size={14} />{t("common.install")}</button>
        )}
      />

      {stats.skills === 0 && (
        <section className="dashboard-section dashboard-first-run">
          <SurfaceSectionHeader title={t("dashboard.firstRunTitle")} description={t("dashboard.firstRunHint")} />
          <ol className="first-run-steps">
            <li><strong>{t("dashboard.firstRunAdd")}</strong><span>{t("dashboard.firstRunAddHint")}</span></li>
            <li><strong>{t("dashboard.firstRunAdopt")}</strong><span>{t("dashboard.firstRunAdoptHint")}</span></li>
            <li><strong>{t("dashboard.firstRunEnable")}</strong><span>{t("dashboard.firstRunEnableHint")}</span></li>
          </ol>
        </section>
      )}

      <div className="dash-stats" aria-label={t("dashboard.statusSummary")}>
        <button type="button" className="stat" onClick={() => onNavigate("skills")}><span>{stats.external > 0 ? t("dashboard.skillsWithExternal", { count: stats.external }) : t("dashboard.skills")}</span><b>{stats.skills}</b><Icon name="arrow" size={15} /></button>
        <button type="button" className="stat" onClick={() => onNavigate("ai")}><span>{t("dashboard.assistants")}</span><b>{stats.assistants}</b><Icon name="arrow" size={15} /></button>
        <div className="stat"><b>{tagCount}</b><span>{t("dashboard.tags")}</span></div>
        <button type="button" className="stat" onClick={() => onNavigate("skills", "updates")}><span>{t("dashboard.updates")}</span><b>{stats.updates}</b><Icon name="arrow" size={15} /></button>
        <div className={`stat${attentionCount > 0 ? " alert" : ""}`}><b>{attentionCount}</b><span>{t("dashboard.risks")}</span></div>
      </div>

      <section className="dashboard-section dashboard-attention">
        <SurfaceSectionHeader title={t("dashboard.attention")} description={t("dashboard.attentionHint")} meta={attentionCount} />
        <div className="dash-rows">
          {stats.updates > 0 && <AttentionRow icon="updates" tone="warn" title={t("dashboard.updateTitle", { count: stats.updates })} body={t("dashboard.updateBody")} action={t("dashboard.review")} onClick={() => onNavigate("skills", "updates")} />}
          {stats.securityRisks > 0 && <AttentionRow icon="shield" tone="error" title={t("dashboard.securityTitle", { count: stats.securityRisks })} body={t("dashboard.securityBody")} action={t("dashboard.review")} onClick={() => onNavigate("skills", "attention")} />}
          {stats.structureIssues > 0 && <AttentionRow icon="skills" tone="warn" title={t("dashboard.structureTitle", { count: stats.structureIssues })} body={t("dashboard.structureBody")} action={t("dashboard.review")} onClick={() => onNavigate("skills", "attention")} />}
          {stats.localChanges > 0 && <AttentionRow icon="edit" tone="error" title={t("dashboard.localTitle", { count: stats.localChanges })} body={t("dashboard.localBody")} action={t("dashboard.review")} onClick={() => onNavigate("skills", "attention")} />}
          {stats.stale > 0 && <AttentionRow icon="unlink" tone="warn" title={t("dashboard.staleTitle", { count: stats.stale })} body={t("dashboard.staleBody")} action={t("dashboard.review")} onClick={() => onNavigate("skills", "attention")} />}
          {driftGroups.map((group) => <AttentionRow key={group.id} icon="branch" tone="warn" title={`${group.name} · ${t("drift.versions", { count: group.versionCount })}`} body={t("dashboard.driftBody")} action={t("dashboard.review")} onClick={() => onOpenDrift(group)} />)}
          {stats.diagnostics > 0 && <AttentionRow icon="search" title={t("dashboard.diagnosticTitle", { count: stats.diagnostics })} body={t("dashboard.diagnosticBody")} action={t("dashboard.review")} onClick={() => onNavigate("ai")} />}
          {attentionCount === 0 && <div className="dash-empty"><Icon name="check" size={20} /><div><strong>{t("dashboard.allClear")}</strong><p>{t("dashboard.allClearHint")}</p></div></div>}
        </div>
        <p className="dashboard-local-note"><Icon name="shield" size={16} />{t("dashboard.localNote")}</p>
      </section>
    </div>
  );
}
