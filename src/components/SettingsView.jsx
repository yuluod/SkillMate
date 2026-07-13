import Icon from "./Icon.jsx";
import {
  buildImportPreviewSummary,
  buildScenarioManifestPreviewSummary,
  buildSkillMateManifestPreviewSummary,
  buildSkillProfilePreviewSummary,
} from "../lib/skillmate.mjs";
import { splitPolicyEntries } from "../lib/installPolicy.mjs";

const SETTINGS_TABS = [
  ["backup", "Git 备份"],
  ["app-update", "应用更新"],
  ["install-policy", "安装安全"],
  ["data", "导入 / 导出"],
  ["skillset", "Skill Set"],
  ["tags", "标签"],
];

function ActionRow({ children }) {
  return <div className="card-actions settings-action-row">{children}</div>;
}

function SummaryList({ lines }) {
  return (
    <ul className="import-preview-list">
      {lines.map((line, index) => <li key={`${line}-${index}`}>{line}</li>)}
    </ul>
  );
}

function BackupSettings({ value }) {
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="lock" size={20} /><h3>Git 备份</h3></div>
      <div className="settings-body">
        <div className="form"><label htmlFor="backup-repo-path">本地仓库路径</label><input id="backup-repo-path" value={value.repoPath} onChange={event => value.setRepoPath(event.target.value)} placeholder="~/skillmate-backup" /></div>
        <div className="form"><label htmlFor="backup-branch">分支</label><input id="backup-branch" value={value.branch} onChange={event => value.setBranch(event.target.value)} placeholder="main" /></div>
        <div className="form"><label htmlFor="backup-remote-url">远程仓库 URL</label><input id="backup-remote-url" value={value.remoteUrl} onChange={event => value.setRemoteUrl(event.target.value)} placeholder="git@github.com:user/skill-backup.git" /></div>
        <div className="git-meta">保存后可手动生成本地 Skill 快照；配置远端时会在同步后推送。</div>
        {value.dirty && <div className="install-compact warn" role="status"><span>设置未保存</span><strong>保存后才能同步，避免使用旧配置。</strong></div>}
        <div className="git-meta">上次同步：{value.lastSync || "从未"}</div>
        <ActionRow>
          <button className="btn btn-primary btn-sm" onClick={value.save} disabled={!value.canSave}><Icon name="check" size={14} />{value.saving ? "保存中" : "保存"}</button>
          <button className="btn btn-secondary btn-sm" onClick={value.sync} disabled={!value.canSync}><Icon name="upload" size={14} />{value.syncing ? "同步中" : "立即同步"}</button>
        </ActionRow>
      </div>
    </div>
  );
}

function AppUpdateSettings({ value }) {
  const view = value.view;
  const progress = Math.max(0, Math.min(100, view.progressPercent ?? 0));
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="updates" size={20} /><h3>应用更新</h3></div>
      <div className="settings-body">
        <div className="app-update-panel">
          <div className="app-update-main">
            <span className={`update-pill ${view.statusTone}`}>{view.statusLabel}</span>
            <h3>{view.nextVersion ? `SkillMate ${view.nextVersion}` : "SkillMate"}</h3>
            <p>{view.nextVersion ? "检测到可安装的新版本" : "检查 GitHub Releases 上的最新正式版本"}</p>
          </div>
          <div className="app-update-meta">
            <div><span className="label">当前版本</span><span className="value mono">{view.currentVersion || "未知"}</span></div>
            <div><span className="label">新版本</span><span className="value mono">{view.nextVersion || "暂无"}</span></div>
            <div><span className="label">发布时间</span><span className="value">{view.dateLabel}</span></div>
          </div>
        </div>
        {view.progressText && (
          <div className="app-update-progress">
            <div className="app-update-progress-head"><span>下载进度</span><strong>{view.progressText}</strong></div>
            <div className="progress-track" role="progressbar" aria-label="应用更新下载进度" aria-valuemin="0" aria-valuemax="100" aria-valuenow={progress}><div className="progress-fill" style={{ width: `${progress}%` }} /></div>
          </div>
        )}
        {view.releaseNotes && (
          <div className="import-preview">
            <div className="import-preview-head"><strong>更新日志</strong><span>来自 release metadata</span></div>
            <pre className="app-update-notes">{view.releaseNotes}</pre>
          </div>
        )}
        {view.error && <div className="install-compact error"><span>更新异常</span><strong>{view.error}</strong></div>}
        <ActionRow>
          <button className="btn btn-primary btn-sm" onClick={value.runPrimaryAction} disabled={!view.canRunPrimaryAction}>
            <Icon name={view.primaryActionIcon} size={14} />{view.primaryActionLabel}
          </button>
          {view.showSecondaryCheck && (
            <button className="btn btn-secondary btn-sm" onClick={value.check} disabled={!view.canCheck}>
              <Icon name="refresh" size={14} />重新检查
            </button>
          )}
        </ActionRow>
        <div className="git-meta">应用更新使用 GitHub Releases 的 latest.json；更新包会由 Tauri 签名校验后再安装。</div>
      </div>
    </div>
  );
}

function InstallPolicySettings({ value }) {
  const policy = value.policy;
  const enforced = policy.mode !== "off";
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="lock" size={20} /><h3>安装安全策略</h3></div>
      <div className="settings-body">
        <div className="form">
          <label htmlFor="install-policy-mode">执行模式</label>
          <select id="install-policy-mode" value={policy.mode} onChange={event => value.update("mode", event.target.value)}>
            <option value="off">仅提示，不阻止安装</option>
            <option value="block-critical">阻止关键风险</option>
            <option value="trusted-only">仅允许可信来源</option>
          </select>
        </div>
        <label className="install-switch">
          <input type="checkbox" checked={policy.block_risky_content} onChange={event => value.update("block_risky_content", event.target.checked)} disabled={!enforced} />
          <span>同时阻止脚本、依赖、网络、环境变量等风险内容</span>
        </label>
        <div className="form settings-section">
          <label htmlFor="trusted-git-hosts">可信 Git 主机</label>
          <textarea id="trusted-git-hosts" value={policy.trusted_git_hosts.join("\n")} onChange={event => value.update("trusted_git_hosts", splitPolicyEntries(event.target.value))} placeholder="github.com&#10;gitlab.com" disabled={policy.mode !== "trusted-only"} />
        </div>
        <div className="form">
          <label htmlFor="trusted-local-roots">可信本地根目录</label>
          <textarea id="trusted-local-roots" value={policy.trusted_local_roots.join("\n")} onChange={event => value.update("trusted_local_roots", splitPolicyEntries(event.target.value))} placeholder="~/Projects/skills" disabled={policy.mode !== "trusted-only"} />
        </div>
        <div className="git-meta">策略在预览和执行时都会重新计算；配置变化或来源内容变化后，旧安装计划会自动失效。可信模式未列出的 Git 主机和本地目录会被拒绝。</div>
        {value.dirty && <div className="install-compact warn" role="status"><span>设置未保存</span><strong>保存后才会用于安装和 Manifest。</strong></div>}
        {value.error && <div className="install-compact error" role="alert"><span>策略异常</span><strong>{value.error}</strong></div>}
        <ActionRow>
          <button className="btn btn-primary btn-sm" onClick={value.save} disabled={!value.dirty || value.saving || value.loading}><Icon name="check" size={14} />{value.saving ? "保存中" : "保存策略"}</button>
          <button className="btn btn-secondary btn-sm" onClick={value.reload} disabled={value.saving || value.loading}><Icon name="refresh" size={14} />重新加载</button>
        </ActionRow>
      </div>
    </div>
  );
}

function DataSettings({ value }) {
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="upload" size={20} /><h3>导入 / 导出</h3></div>
      <div className="settings-body">
        <div className="form"><label htmlFor="library-export-path">导出文件</label><input id="library-export-path" value={value.exportPath} onChange={event => value.setExportPath(event.target.value)} placeholder="~/skillmate-export.json" /></div>
        <ActionRow><button className="btn btn-primary btn-sm" onClick={value.exportLibrary}><Icon name="upload" size={14} />导出组织数据</button></ActionRow>
        <div className="form settings-section"><label htmlFor="library-import-path">导入文件</label><input id="library-import-path" value={value.importPath} onChange={event => value.updateImportPath(event.target.value)} placeholder="~/skillmate-export.json" /></div>
        <div className="form"><label htmlFor="library-import-mode">导入方式</label><select id="library-import-mode" value={value.importMode} onChange={event => value.updateImportMode(event.target.value)}><option value="merge">合并</option><option value="replace">替换现有组织数据</option></select></div>
        <div className="git-meta">导入和导出只处理标签、场景以及当前受管 Skill 清单，不会直接覆盖本地 Skill 文件。</div>
        {value.importPreview && (
          <div className="import-preview">
            <div className="import-preview-head"><strong>{value.importPreview.replace_existing ? "替换导入预览" : "合并导入预览"}</strong><span>{value.importMode === "replace" ? "将先清空再恢复组织数据" : "仅写入导入文件中的组织数据"}</span></div>
            <SummaryList lines={buildImportPreviewSummary(value.importPreview)} />
          </div>
        )}
        <ActionRow>
          <button className="btn btn-secondary btn-sm" onClick={value.previewImport} disabled={value.previewingImport || value.applyingImport}><Icon name="preview" size={14} />{value.previewingImport ? "预览中" : "预览导入"}</button>
          <button className="btn btn-primary btn-sm" onClick={value.importLibrary} disabled={!value.importPreview || !value.importPreviewCurrent || value.applyingImport}><Icon name="check" size={14} />{value.applyingImport ? "导入中" : "导入组织数据"}</button>
        </ActionRow>
        <div className="form settings-section"><label htmlFor="scenario-manifest-path">场景 manifest</label><input id="scenario-manifest-path" value={value.scenarioManifestPath} onChange={event => value.updateScenarioManifestPath(event.target.value)} placeholder="~/skillmate-scenarios.json" /></div>
        <div className="form"><label htmlFor="scenario-manifest-mode">场景导入方式</label><select id="scenario-manifest-mode" value={value.scenarioManifestMode} onChange={event => value.updateScenarioManifestMode(event.target.value)}><option value="merge">合并</option><option value="replace">替换现有场景</option></select></div>
        <div className="git-meta">场景 manifest 只处理场景和 Skill 路径引用，不会修改标签或本地 Skill 文件。</div>
        {value.scenarioManifestPreview && (
          <div className="import-preview">
            <div className="import-preview-head"><strong>{value.scenarioManifestPreview.replace_existing ? "替换场景预览" : "合并场景预览"}</strong><span>{value.scenarioManifestMode === "replace" ? "将先清空现有场景" : "仅写入 manifest 中的场景"}</span></div>
            <SummaryList lines={buildScenarioManifestPreviewSummary(value.scenarioManifestPreview)} />
          </div>
        )}
        <ActionRow>
          <button className="btn btn-secondary btn-sm" onClick={value.exportScenarioManifest}><Icon name="upload" size={14} />导出场景</button>
          <button className="btn btn-secondary btn-sm" onClick={value.previewScenarioManifest} disabled={value.previewingScenarioManifest || value.applyingScenarioManifest}><Icon name="preview" size={14} />{value.previewingScenarioManifest ? "预览中" : "预览场景"}</button>
          <button className="btn btn-primary btn-sm" onClick={value.importScenarioManifest} disabled={!value.scenarioManifestPreview || !value.scenarioManifestPreviewCurrent || value.applyingScenarioManifest}><Icon name="check" size={14} />{value.applyingScenarioManifest ? "导入中" : "导入场景"}</button>
        </ActionRow>
      </div>
    </div>
  );
}

function SkillSetSettings({ value }) {
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="skills" size={20} /><h3>Skill Set</h3></div>
      <div className="settings-body">
        <div className="form"><label htmlFor="project-manifest-root">项目锁定清单</label><input id="project-manifest-root" value={value.projectManifestRoot} onChange={event => value.setProjectManifestRoot(event.target.value)} placeholder="/path/to/project" /></div>
        <div className="git-meta">导出项目内受管 Skill 到项目根目录的 skillmate.toml；路径和记录顺序会规范化，适合提交到版本库。</div>
        <ActionRow>
          <button className="btn btn-secondary btn-sm" onClick={value.exportProjectManifest} disabled={!value.projectManifestRoot.trim()}><Icon name="lock" size={14} />导出项目锁定清单</button>
        </ActionRow>
        <div className="form"><label htmlFor="skillmate-manifest-path">SkillMate manifest</label><input id="skillmate-manifest-path" value={value.manifestPath} onChange={event => value.updateManifestPath(event.target.value)} placeholder="~/skillmate.toml" /></div>
        <div className="git-meta">skillmate.toml 记录来源、目标助手、解析提交和内容哈希；项目清单只对齐同一项目，完整清单才会对齐全部受管 Skill。</div>
        {value.manifestPreview && (
          <div className="import-preview">
            <div className="import-preview-head"><strong>SkillMate manifest 预览</strong><span>{value.manifestPreview.can_apply ? "可应用" : "存在冲突"}</span></div>
            <SummaryList lines={buildSkillMateManifestPreviewSummary(value.manifestPreview)} />
          </div>
        )}
        <ActionRow>
          <button className="btn btn-secondary btn-sm" onClick={value.exportManifest}><Icon name="upload" size={14} />导出 SkillMate manifest</button>
          <button className="btn btn-secondary btn-sm" onClick={value.previewManifest} disabled={value.previewingManifest || value.applyingManifest}><Icon name="preview" size={14} />{value.previewingManifest ? "预览中" : "预览 manifest"}</button>
          <button className="btn btn-primary btn-sm" onClick={value.applyManifest} disabled={!value.manifestPreview || !value.manifestPreviewCurrent || !value.manifestPreview.can_apply || value.applyingManifest}><Icon name="check" size={14} />{value.applyingManifest ? "应用中" : "应用 manifest"}</button>
        </ActionRow>
        <div className="form settings-section"><label htmlFor="profile-name">Skill Set Profile</label><input id="profile-name" value={value.profileName} onChange={event => value.setProfileName(event.target.value)} placeholder="例如：写作模式 / 开发模式" /></div>
        <div className="form"><label htmlFor="profile-description">Profile 说明</label><input id="profile-description" value={value.profileDescription} onChange={event => value.setProfileDescription(event.target.value)} placeholder="这个组合适合什么工作流" /></div>
        <div className="git-meta">Profile 会保存当前所有助手下的 Skill 来源组合；应用前会预览，并将受管 Skill 对齐到目标组合。</div>
        <ActionRow>
          <button className="btn btn-secondary btn-sm" onClick={value.saveProfile}><Icon name="check" size={14} />保存当前组合</button>
          <button className="btn btn-secondary btn-sm" onClick={value.rollbackProfile} disabled={!value.profiles.previous_active_profile_id || value.applyingProfile}><Icon name="refresh" size={14} />回滚上个 Profile</button>
        </ActionRow>
        {value.profiles.profiles?.length > 0 && (
          <div className="scenario-detail settings-action-row">
            {value.profiles.profiles.map((profile) => (
              <div key={profile.id} className="scenario-path-row">
                <div><strong>{profile.name}{profile.active ? " · 当前" : ""}</strong><div className="card-path profile-description">{profile.description || `${profile.skills.length} 条 Skill 记录`}</div></div>
                <div className="card-actions">
                  <button className="btn btn-secondary btn-sm" onClick={() => value.previewProfile(profile.id)} disabled={value.previewingProfile || value.applyingProfile}><Icon name="preview" size={14} />预览</button>
                  <button
                    className="btn btn-primary btn-sm"
                    onClick={() => value.applyProfile(profile.id)}
                    disabled={
                      value.previewingProfile
                      || value.applyingProfile
                      || value.profilePreview?.profile?.id !== profile.id
                      || !value.profilePreview?.manifest_preview?.can_apply
                      || Boolean(value.profilePreview?.profile_issues?.length)
                    }
                  ><Icon name="check" size={14} />应用预览</button>
                </div>
              </div>
            ))}
          </div>
        )}
        {value.profilePreview && (
          <div className="import-preview">
            <div className="import-preview-head"><strong>Profile 预览</strong><span>{value.profilePreview.manifest_preview?.can_apply && !value.profilePreview.profile_issues?.length ? "可应用" : "存在问题"}</span></div>
            <SummaryList lines={buildSkillProfilePreviewSummary(value.profilePreview)} />
          </div>
        )}
      </div>
    </div>
  );
}

function TagSettings({ value }) {
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="tag" size={20} /><h3>标签管理</h3></div>
      <div className="settings-body">
        <div className="tag-form"><input aria-label="标签名" value={value.name} onChange={event => value.setName(event.target.value)} placeholder="标签名" /><input aria-label="标签颜色" type="color" value={value.color} onChange={event => value.setColor(event.target.value)} /><button className="btn btn-primary btn-sm" onClick={value.add}><Icon name="plus" size={14} />添加</button></div>
        <div className="tag-list settings-action-row">{value.tags.map(tag => <div key={tag.id} className="tag-chip active" style={{ "--c": tag.color }}><span className="tag-dot" />{tag.name}</div>)}</div>
      </div>
    </div>
  );
}

export default function SettingsView({ activeTab, setActiveTab, backup, appUpdate, installPolicy, data, skillSet, tags }) {
  return (
    <div className="settings">
      <div className="content-head"><div><h2>设置</h2></div></div>
      <div className="sort-tabs settings-tabs" role="tablist" aria-label="设置分类">
        {SETTINGS_TABS.map(([key, label]) => (
          <button key={key} role="tab" aria-selected={activeTab === key} className={`sort-tab ${activeTab === key ? "active" : ""}`} onClick={() => setActiveTab(key)}>{label}</button>
        ))}
      </div>
      {activeTab === "backup" && <BackupSettings value={backup} />}
      {activeTab === "app-update" && <AppUpdateSettings value={appUpdate} />}
      {activeTab === "install-policy" && <InstallPolicySettings value={installPolicy} />}
      {activeTab === "data" && <DataSettings value={data} />}
      {activeTab === "skillset" && <SkillSetSettings value={skillSet} />}
      {activeTab === "tags" && <TagSettings value={tags} />}
    </div>
  );
}
