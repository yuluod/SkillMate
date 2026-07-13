import Icon from "./Icon.jsx";

function formatHomePath(path) {
  return path.replace(/^\/Users\/[^/]+/, "~");
}

export default function ScenarioView({ scenarios, skills, flow }) {
  return (
    <div>
      <div className="content-head"><div><h2>场景</h2><span className="count">{scenarios.length}</span></div></div>
      <div className="settings-card scenario-editor-card">
        <div className="settings-head"><Icon name="scenarios" size={20} /><h3>场景编辑器</h3></div>
        <div className="settings-body">
          <div className="form"><label htmlFor="scenario-name">场景名称</label><input id="scenario-name" value={flow.editor.name} onChange={event => flow.editor.setName(event.target.value)} placeholder="例如：写作模式" /></div>
          <div className="form"><label htmlFor="scenario-description">场景描述</label><input id="scenario-description" value={flow.editor.description} onChange={event => flow.editor.setDescription(event.target.value)} placeholder="这个场景适合处理什么任务" /></div>
          <div className="form">
            <label htmlFor="scenario-paths">手动路径输入</label>
            <textarea id="scenario-paths" value={flow.editor.manualInput} onChange={event => flow.editor.setManualInput(event.target.value)} placeholder="可粘贴多个 Skill 路径，使用空格、换行或逗号分隔" />
          </div>
          <div className="form">
            <label>从当前 Skills 选择</label>
            <div className="scenario-pick">
              {skills.slice(0, 12).map((skill) => (
                <label key={skill.path} className="scenario-pick-item">
                  <input type="checkbox" checked={flow.editor.selectedPaths.includes(skill.path)} onChange={() => flow.editor.togglePath(skill.path)} />
                  <span>{skill.name}</span>
                </label>
              ))}
              {skills.length === 0 && <span className="empty-hint">当前没有可选 Skills</span>}
            </div>
          </div>
          <div className="card-actions">
            <button className="btn btn-primary btn-sm" onClick={flow.editor.create}><Icon name="plus" size={14} />保存场景</button>
            <button className="btn btn-secondary btn-sm" onClick={flow.editor.clear}>清空</button>
          </div>
        </div>
      </div>
      {scenarios.length === 0 ? (
        <div className="empty-state"><div className="empty-icon"><Icon name="scenarios" size={48} /></div><h3>暂无场景</h3><p>创建场景来组织 Skills</p></div>
      ) : (
        <div className="scenario-list">
          {scenarios.map((scenario) => (
            <div className="scenario-card" key={scenario.id}>
              <div className="scenario-icon"><Icon name="scenarios" size={24} /></div>
              <div className="scenario-info">
                <h3>{scenario.name}</h3>
                {scenario.description && <p>{scenario.description}</p>}
                <span>{scenario.skill_ids.length} 个 Skills · {scenario.created_at}</span>
                {flow.expandedId === scenario.id && (
                  <div className="scenario-detail">
                    {flow.details[scenario.id]?.map((item) => (
                      <div key={item.path} className={`scenario-path-row ${item.exists ? "" : "missing"}`}>
                        <div>
                          <strong>{item.skill?.name || "未找到 Skill"}</strong>
                          <div className="card-path scenario-detail-path">{formatHomePath(item.path)}</div>
                        </div>
                        <span className={`tag more ${item.exists ? "" : "warn"}`}>{item.exists ? (item.skill?.ai || "已存在") : "已缺失"}</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
              <div className="card-actions">
                <button className="btn btn-secondary btn-sm" onClick={() => flow.setExpandedId(flow.expandedId === scenario.id ? "" : scenario.id)}><Icon name="preview" size={14} />{flow.expandedId === scenario.id ? "收起" : "详情"}</button>
                <button className="btn btn-primary btn-sm" onClick={() => flow.apply(scenario)}><Icon name="sparkles" size={14} />应用</button>
                <button className="btn btn-secondary btn-sm" onClick={() => flow.loadIntoEditor(scenario)}><Icon name="check" size={14} />回填</button>
                <button className="btn btn-secondary btn-sm" onClick={() => flow.copyPaths(scenario.skill_ids)}><Icon name="folder" size={14} />复制路径</button>
                <button className="btn btn-ghost btn-sm danger" onClick={() => flow.remove(scenario.id)} title="删除" aria-label={`删除场景 ${scenario.name}`}><Icon name="trash" size={16} /></button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
