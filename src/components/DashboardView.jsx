import { useState } from "react";
import Icon from "./Icon.jsx";
import { skillmateApi } from "../lib/skillmateApi.js";
import { useI18n } from "../lib/i18n.jsx";
import { toUserErrorMessage } from "../lib/errorMessage.mjs";
import { SurfaceHeader, SurfaceSectionHeader } from "./SurfaceHeader.jsx";

function AttentionRow({ icon, tone = "", title, body, onClick, action }) {
  return (
    <button className={`attention-row ${tone}`} onClick={onClick} type="button">
      <span className="attention-icon"><Icon name={icon} size={18} /></span>
      <span className="attention-copy"><strong>{title}</strong><small>{body}</small></span>
      <span className="attention-action">{action}<Icon name="arrow" size={15} /></span>
    </button>
  );
}

function MarketResult({ item, onInstall }) {
  const { t } = useI18n();
  return (
    <article className="market-result">
      <div className="market-result-head">
        <div><strong>{item.name}</strong><span>{item.repository}</span></div>
        <span className="source-badge git">{item.source === "skills-sh" ? "skills.sh" : "GitHub"}</span>
      </div>
      <p>{item.description || item.skillId || item.repository}</p>
      <div className="market-result-meta">
        {item.installs > 0 && <span>{t("market.installs", { count: item.installs.toLocaleString() })}</span>}
        {item.stars > 0 && <span>{t("market.stars", { count: item.stars.toLocaleString() })}</span>}
      </div>
      <div className="market-result-actions">
        <a className="btn btn-ghost btn-sm" href={item.url} target="_blank" rel="noreferrer"><Icon name="external" size={14} />{t("market.open")}</a>
        <button className="btn btn-primary btn-sm" onClick={() => onInstall(item)}><Icon name="shield" size={14} />{t("market.safeInstall")}</button>
      </div>
    </article>
  );
}

export default function DashboardView({ stats, tagCount, driftGroups, onNavigate, onMarketInstall, onOpenDrift }) {
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

  const attentionCount = stats.updates + stats.structureIssues + stats.securityRisks
    + stats.localChanges + stats.driftGroups + stats.diagnostics;

  return (
    <div className="dashboard view-shell">
      <SurfaceHeader title={t("dashboard.title")} description={t("dashboard.subtitle")} />

      <div className="dashboard-status" aria-label={t("dashboard.title")}>
        <span className="stamp"><span className="stamp-num">{stats.skills}</span>{t("dashboard.skills")}</span>
        <span className="stamp"><span className="stamp-num">{stats.assistants}</span>{t("dashboard.assistants")}</span>
        <span className="stamp"><span className="stamp-num">{tagCount}</span>{t("dashboard.tags")}</span>
        <span className={`stamp ${stats.updates ? "warn" : "success"}`}><span className="stamp-num">{stats.updates}</span>{t("dashboard.updates")}</span>
        <span className={`stamp ${attentionCount ? "error" : "success"}`}><span className="stamp-num">{attentionCount}</span>{t("dashboard.risks")}</span>
      </div>

      <section className="dashboard-section">
        <SurfaceSectionHeader title={t("dashboard.attention")} description={t("dashboard.attentionHint")} />
        <div className="attention-queue">
          {stats.updates > 0 && <AttentionRow icon="updates" tone="warn" title={t("dashboard.updateTitle", { count: stats.updates })} body={t("dashboard.updateBody")} action={t("dashboard.review")} onClick={() => onNavigate("updates")} />}
          {stats.securityRisks > 0 && <AttentionRow icon="shield" tone="error" title={t("dashboard.securityTitle", { count: stats.securityRisks })} body={t("dashboard.securityBody")} action={t("dashboard.review")} onClick={() => onNavigate("skills")} />}
          {stats.structureIssues > 0 && <AttentionRow icon="skills" tone="warn" title={t("dashboard.structureTitle", { count: stats.structureIssues })} body={t("dashboard.structureBody")} action={t("dashboard.review")} onClick={() => onNavigate("skills")} />}
          {stats.localChanges > 0 && <AttentionRow icon="lock" tone="error" title={t("dashboard.localTitle", { count: stats.localChanges })} body={t("dashboard.localBody")} action={t("dashboard.review")} onClick={() => onNavigate("skills")} />}
          {driftGroups.map((group) => <AttentionRow key={group.id} icon="branch" tone="warn" title={`${group.name} · ${t("drift.versions", { count: group.versionCount })}`} body={t("dashboard.driftBody")} action={t("dashboard.review")} onClick={() => onOpenDrift(group)} />)}
          {stats.diagnostics > 0 && <AttentionRow icon="search" title={t("dashboard.diagnosticTitle", { count: stats.diagnostics })} body={t("dashboard.diagnosticBody")} action={t("dashboard.review")} onClick={() => onNavigate("ai")} />}
          {attentionCount === 0 && <div className="attention-empty"><Icon name="check" size={22} /><div><strong>{t("dashboard.allClear")}</strong><p>{t("dashboard.allClearHint")}</p></div></div>}
        </div>
      </section>

      <section className="dashboard-section market-search">
        <SurfaceSectionHeader title={t("market.title")} description={t("market.subtitle")} />
        <form className="market-search-form" onSubmit={search}>
          <label><span>{t("market.source")}</span><select value={source} onChange={(event) => setSource(event.target.value)}><option value="skills-sh">skills.sh</option><option value="github">GitHub</option></select></label>
          <label className="market-query"><span className="visually-hidden">{t("common.search")}</span><Icon name="search" size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("market.placeholder")} /></label>
          <button className="btn btn-primary" disabled={market.loading || query.trim().length < 2}><Icon name="search" size={15} />{market.loading ? t("market.searching") : t("market.search")}</button>
        </form>
        {market.error && <div className="install-compact error" role="alert"><strong>{market.error}</strong></div>}
        {!market.error && market.items.length === 0 && <div className="market-empty">{market.searched ? t("market.noResults") : t("market.empty")}</div>}
        {market.items.length > 0 && <div className="market-grid">{market.items.map((item) => <MarketResult key={item.id} item={item} onInstall={onMarketInstall} />)}</div>}
      </section>
    </div>
  );
}
