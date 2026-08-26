import { useMemo, useState } from "react";
import Icon from "./Icon.jsx";
import { AiAvatar } from "./InventoryViews.jsx";
import { useI18n } from "../lib/i18n.jsx";
import { SurfaceHeader, SurfaceSectionHeader } from "./SurfaceHeader.jsx";

function formatHomePath(path) {
  return path
    .replace(/^\/Users\/[^/]+/, "~")
    .replace(/^[A-Za-z]:\\Users\\[^\\]+/i, "~");
}

function skillPlatforms(skill, unknownLabel) {
  if (Array.isArray(skill.availableIn) && skill.availableIn.length > 0) {
    return skill.availableIn;
  }
  return [{ name: skill.ai || unknownLabel, icon: skill.aiIcon || "" }];
}

export default function ScenarioView({ scenarios, skills, flow }) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const filteredSkills = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    if (!normalizedQuery) return skills;
    return skills.filter((skill) => [
      skill.name,
      skill.ai,
      skill.path,
      ...(skill.availableIn || []).map((platform) => platform.name),
    ]
      .some((value) => String(value || "").toLocaleLowerCase().includes(normalizedQuery)));
  }, [query, skills]);
  const groupedSkills = useMemo(() => {
    const groups = new Map();
    for (const skill of filteredSkills) {
      const platforms = skillPlatforms(skill, t("common.unknown"));
      const shared = platforms.length > 1;
      const platform = shared ? t("scenarios.sharedPlatforms") : platforms[0].name;
      const groupKey = shared ? "shared" : `platform:${platform}`;
      const group = groups.get(groupKey);
      if (group) group.skills.push(skill);
      else groups.set(groupKey, {
        platform,
        icon: shared ? "" : platforms[0].icon,
        shared,
        skills: [skill],
      });
    }
    return [...groups.values()];
  }, [filteredSkills, t]);
  const selectedCount = flow.editor.selectedPaths.length;

  return (
    <div className="scenario-view view-shell">
      <SurfaceHeader title={t("nav.scenarios")} description={t("scenarios.subtitle")} meta={scenarios.length} />
      <section className="scenario-editor-card">
        <header className="scenario-editor-head">
          <span className="scenario-editor-icon"><Icon name="scenarios" size={20} /></span>
          <div><h3>{t("scenarios.editor")}</h3><p>{t("scenarios.editorHint")}</p></div>
        </header>

        <div className="scenario-editor-layout">
          <div className="scenario-meta-panel">
            <div className="form"><label htmlFor="scenario-name">{t("scenarios.name")}</label><input id="scenario-name" value={flow.editor.name} onChange={event => flow.editor.setName(event.target.value)} placeholder={t("scenarios.namePlaceholder")} /></div>
            <div className="form"><label htmlFor="scenario-description">{t("scenarios.description")}</label><input id="scenario-description" value={flow.editor.description} onChange={event => flow.editor.setDescription(event.target.value)} placeholder={t("scenarios.descriptionPlaceholder")} /></div>
            <details className="scenario-manual-paths">
              <summary><Icon name="folder" size={15} /><span>{t("scenarios.manualPaths")}</span></summary>
              <p>{t("scenarios.manualPathsHint")}</p>
              <div className="form">
                <label className="visually-hidden" htmlFor="scenario-paths">{t("scenarios.manualPaths")}</label>
                <textarea id="scenario-paths" value={flow.editor.manualInput} onChange={event => flow.editor.setManualInput(event.target.value)} placeholder={t("scenarios.manualPathsPlaceholder")} />
              </div>
            </details>
          </div>

          <div className="scenario-picker-panel">
            <div className="scenario-picker-head">
              <div><strong>{t("scenarios.pick")}</strong><span>{t("scenarios.availableCount", { count: skills.length })}</span></div>
              <label className="scenario-skill-search">
                <span className="visually-hidden">{t("scenarios.search")}</span>
                <Icon name="search" size={15} />
                <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("scenarios.searchPlaceholder")} />
              </label>
            </div>
            <div className="scenario-skill-catalog" role="group" aria-label={t("scenarios.pick")}>
              {groupedSkills.map((group) => (
                <section key={group.platform} className="scenario-skill-group" role="group" aria-label={group.platform}>
                  <header className="scenario-skill-group-head">
                    {group.shared
                      ? <span className="scenario-shared-platform-icon"><Icon name="branch" size={15} /></span>
                      : <AiAvatar name={group.platform} brand={group.icon} size={24} />}
                    <strong>{group.platform}</strong>
                    <span>{group.skills.length}</span>
                  </header>
                  <div className="scenario-skill-rows">
                    {group.skills.map((skill) => {
                      const selected = flow.editor.selectedPaths.includes(skill.path);
                      const platforms = skillPlatforms(skill, t("common.unknown"));
                      const availabilityLabel = platforms.map((platform) => platform.name).join(t("common.listSeparator"));
                      return <label key={skill.path} className={`scenario-skill-row ${selected ? "selected" : ""}`}>
                        <input type="checkbox" checked={selected} onChange={() => flow.editor.togglePath(skill.path)} />
                        <span className="scenario-skill-copy">
                          <strong>{skill.name}</strong>
                          {platforms.length > 1 && <span className="scenario-skill-platforms">{availabilityLabel}</span>}
                          <small title={skill.path}>{formatHomePath(skill.path)}</small>
                        </span>
                        <span className="scenario-skill-state">
                          <Icon name={selected ? "check" : "plus"} size={13} />
                          {t(selected ? "scenarios.selected" : "scenarios.add")}
                        </span>
                      </label>;
                    })}
                  </div>
                </section>
              ))}
              {skills.length === 0 && <div className="scenario-picker-empty"><Icon name="skills" size={22} /><span>{t("scenarios.noChoices")}</span></div>}
              {skills.length > 0 && filteredSkills.length === 0 && <div className="scenario-picker-empty"><Icon name="search" size={22} /><span>{t("scenarios.noMatch")}</span></div>}
            </div>
          </div>
        </div>

        <footer className="scenario-editor-actions">
          <span>{t("scenarios.selectedCount", { count: selectedCount })}</span>
          <div className="card-actions">
            <button className="btn btn-secondary btn-sm" onClick={flow.editor.clear}>{t("scenarios.clear")}</button>
            <button className="btn btn-primary btn-sm" onClick={flow.editor.create}><Icon name="plus" size={14} />{t("scenarios.save")}</button>
          </div>
        </footer>
      </section>

      <SurfaceSectionHeader title={t("scenarios.saved")} meta={scenarios.length} />
      {scenarios.length === 0 ? (
        <div className="scenario-empty"><Icon name="scenarios" size={22} /><div><strong>{t("scenarios.empty")}</strong><p>{t("scenarios.emptyHint")}</p></div></div>
      ) : (
        <div className="scenario-list">
          {scenarios.map((scenario) => (
            <div className="scenario-card" key={scenario.id}>
              <div className="scenario-icon"><Icon name="scenarios" size={24} /></div>
              <div className="scenario-info">
                <h3>{scenario.name}</h3>
                {scenario.description && <p>{scenario.description}</p>}
                <span>{t("scenarios.skillCount", { count: scenario.skill_ids.length })} · {scenario.created_at}</span>
                {flow.expandedId === scenario.id && (
                  <div className="scenario-detail">
                    {flow.details[scenario.id]?.map((item) => (
                      <div key={item.path} className={`scenario-path-row ${item.exists ? "" : "missing"}`}>
                        <div>
                          <strong>{item.skill?.name || t("scenarios.missingSkill")}</strong>
                          <div className="card-path scenario-detail-path">{formatHomePath(item.path)}</div>
                        </div>
                        <span className={`tag more ${item.exists ? "" : "warn"}`}>{item.exists ? (item.skill?.ai || t("scenarios.exists")) : t("scenarios.missing")}</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
              <div className="card-actions">
                <button className="btn btn-secondary btn-sm" onClick={() => flow.setExpandedId(flow.expandedId === scenario.id ? "" : scenario.id)}><Icon name="preview" size={14} />{t(flow.expandedId === scenario.id ? "scenarios.collapse" : "scenarios.expand")}</button>
                <button className="btn btn-primary btn-sm" onClick={() => flow.apply(scenario)}><Icon name="sparkles" size={14} />{t("scenarios.apply")}</button>
                <button className="btn btn-secondary btn-sm" onClick={() => flow.loadIntoEditor(scenario)}><Icon name="check" size={14} />{t("scenarios.load")}</button>
                <button className="btn btn-secondary btn-sm" onClick={() => flow.copyPaths(scenario.skill_ids)}><Icon name="folder" size={14} />{t("scenarios.copyPaths")}</button>
                <button className="btn btn-ghost btn-sm danger" onClick={() => flow.remove(scenario.id)} title={t("scenarios.remove", { name: scenario.name })} aria-label={t("scenarios.remove", { name: scenario.name })}><Icon name="trash" size={16} /></button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
