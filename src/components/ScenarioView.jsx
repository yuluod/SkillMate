import Icon from "./Icon.jsx";
import { useI18n } from "../lib/i18n.jsx";

function formatHomePath(path) {
  return path.replace(/^\/Users\/[^/]+/, "~");
}

export default function ScenarioView({ scenarios, skills, flow }) {
  const { t } = useI18n();
  return (
    <div>
      <div className="content-head"><div><h2>{t("nav.scenarios")}</h2><span className="count">{scenarios.length}</span></div></div>
      <div className="settings-card scenario-editor-card">
        <div className="settings-head"><Icon name="scenarios" size={20} /><h3>{t("scenarios.editor")}</h3></div>
        <div className="settings-body">
          <div className="form"><label htmlFor="scenario-name">{t("scenarios.name")}</label><input id="scenario-name" value={flow.editor.name} onChange={event => flow.editor.setName(event.target.value)} placeholder={t("scenarios.namePlaceholder")} /></div>
          <div className="form"><label htmlFor="scenario-description">{t("scenarios.description")}</label><input id="scenario-description" value={flow.editor.description} onChange={event => flow.editor.setDescription(event.target.value)} placeholder={t("scenarios.descriptionPlaceholder")} /></div>
          <div className="form">
            <label htmlFor="scenario-paths">{t("scenarios.manualPaths")}</label>
            <textarea id="scenario-paths" value={flow.editor.manualInput} onChange={event => flow.editor.setManualInput(event.target.value)} placeholder={t("scenarios.manualPathsPlaceholder")} />
          </div>
          <div className="form">
            <label>{t("scenarios.pick")}</label>
            <div className="scenario-pick">
              {skills.slice(0, 12).map((skill) => (
                <label key={skill.path} className="scenario-pick-item">
                  <input type="checkbox" checked={flow.editor.selectedPaths.includes(skill.path)} onChange={() => flow.editor.togglePath(skill.path)} />
                  <span>{skill.name}</span>
                </label>
              ))}
              {skills.length === 0 && <span className="empty-hint">{t("scenarios.noChoices")}</span>}
            </div>
          </div>
          <div className="card-actions">
            <button className="btn btn-primary btn-sm" onClick={flow.editor.create}><Icon name="plus" size={14} />{t("scenarios.save")}</button>
            <button className="btn btn-secondary btn-sm" onClick={flow.editor.clear}>{t("scenarios.clear")}</button>
          </div>
        </div>
      </div>
      {scenarios.length === 0 ? (
        <div className="empty-state"><div className="empty-icon"><Icon name="scenarios" size={48} /></div><h3>{t("scenarios.empty")}</h3><p>{t("scenarios.emptyHint")}</p></div>
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
