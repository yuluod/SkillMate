import Icon from "./Icon.jsx";
import {
  buildImportPreviewSummary,
  buildScenarioManifestPreviewSummary,
  buildSkillMateManifestPreviewSummary,
  buildSkillProfilePreviewSummary,
} from "../lib/skillmate.mjs";
import { splitPolicyEntries } from "../lib/installPolicy.mjs";
import { useI18n } from "../lib/i18n.jsx";

const SETTINGS_TABS = [
  ["backup", "settings.tabs.backup"],
  ["app-update", "settings.tabs.appUpdate"],
  ["install-policy", "settings.tabs.installPolicy"],
  ["data", "settings.tabs.data"],
  ["skillset", "settings.tabs.skillset"],
  ["tags", "settings.tabs.tags"],
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
  const { t } = useI18n();
  const busy = value.saving || value.syncing;
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="lock" size={20} /><h3>{t("settings.tabs.backup")}</h3></div>
      <div className="settings-body">
        <div className="form"><label htmlFor="backup-repo-path">{t("settings.backup.repoPath")}</label><input id="backup-repo-path" value={value.repoPath} onChange={event => value.setRepoPath(event.target.value)} placeholder="~/skillmate-backup" disabled={busy} /></div>
        <div className="form"><label htmlFor="backup-branch">{t("settings.backup.branch")}</label><input id="backup-branch" value={value.branch} onChange={event => value.setBranch(event.target.value)} placeholder="main" disabled={busy} /></div>
        <div className="form"><label htmlFor="backup-remote-url">{t("settings.backup.remote")}</label><input id="backup-remote-url" value={value.remoteUrl} onChange={event => value.setRemoteUrl(event.target.value)} placeholder="git@github.com:user/skill-backup.git" disabled={busy} /></div>
        <div className="git-meta">{t("settings.backup.help")}</div>
        {value.dirty && <div className="install-compact warn" role="status"><span>{t("settings.unsaved")}</span><strong>{t("settings.backup.unsavedHelp")}</strong></div>}
        <div className="git-meta">{t("settings.backup.lastSync", { value: value.lastSync || t("settings.never") })}</div>
        <ActionRow>
          <button className="btn btn-primary btn-sm" onClick={value.save} disabled={!value.canSave}><Icon name="check" size={14} />{t(value.saving ? "settings.saving" : "common.save")}</button>
          <button className="btn btn-secondary btn-sm" onClick={value.sync} disabled={!value.canSync}><Icon name="upload" size={14} />{t(value.syncing ? "settings.syncing" : "settings.syncNow")}</button>
        </ActionRow>
      </div>
    </div>
  );
}

function AppUpdateSettings({ value }) {
  const { t } = useI18n();
  const view = value.view;
  const progress = Math.max(0, Math.min(100, view.progressPercent ?? 0));
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="updates" size={20} /><h3>{t("settings.tabs.appUpdate")}</h3></div>
      <div className="settings-body">
        <div className="app-update-panel">
          <div className="app-update-main">
            <span className={`update-pill ${view.statusTone}`}>{t(`settings.appUpdate.status.${view.status}`)}</span>
            <h3>{view.nextVersion ? `SkillMate ${view.nextVersion}` : "SkillMate"}</h3>
            <p>{t(view.nextVersion ? "settings.appUpdate.available" : "settings.appUpdate.checkHint")}</p>
          </div>
          <div className="app-update-meta">
            <div><span className="label">{t("settings.appUpdate.current")}</span><span className="value mono">{view.currentVersion || t("common.unknown")}</span></div>
            <div><span className="label">{t("settings.appUpdate.next")}</span><span className="value mono">{view.nextVersion || t("settings.appUpdate.none")}</span></div>
            <div><span className="label">{t("settings.appUpdate.date")}</span><span className="value">{view.dateLabel}</span></div>
          </div>
        </div>
        {view.progressText && (
          <div className="app-update-progress">
            <div className="app-update-progress-head"><span>{t("settings.appUpdate.progress")}</span><strong>{view.progressText}</strong></div>
            <div className="progress-track" role="progressbar" aria-label={t("settings.appUpdate.progress")} aria-valuemin="0" aria-valuemax="100" aria-valuenow={progress}><div className="progress-fill" style={{ width: `${progress}%` }} /></div>
          </div>
        )}
        {view.releaseNotes && (
          <div className="import-preview">
            <div className="import-preview-head"><strong>{t("settings.appUpdate.releaseNotes")}</strong><span>{t("settings.appUpdate.metadata")}</span></div>
            <pre className="app-update-notes">{view.releaseNotes}</pre>
          </div>
        )}
        {view.error && <div className="install-compact error"><span>{t("settings.appUpdate.error")}</span><strong>{view.error}</strong></div>}
        <ActionRow>
          <button className="btn btn-primary btn-sm" onClick={value.runPrimaryAction} disabled={!view.canRunPrimaryAction}>
            <Icon name={view.primaryActionIcon} size={14} />{t(`settings.appUpdate.action.${view.primaryAction}`)}
          </button>
          {view.showSecondaryCheck && (
            <button className="btn btn-secondary btn-sm" onClick={value.check} disabled={!view.canCheck}>
              <Icon name="refresh" size={14} />{t("settings.appUpdate.recheck")}
            </button>
          )}
        </ActionRow>
        <div className="git-meta">{t("settings.appUpdate.help")}</div>
      </div>
    </div>
  );
}

function InstallPolicySettings({ value }) {
  const { t } = useI18n();
  const policy = value.policy;
  const enforced = policy.mode !== "off";
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="lock" size={20} /><h3>{t("settings.policy.title")}</h3></div>
      <div className="settings-body">
        <div className="form">
          <label htmlFor="install-policy-mode">{t("settings.policy.mode")}</label>
          <select id="install-policy-mode" value={policy.mode} onChange={event => value.update("mode", event.target.value)}>
            <option value="off">{t("settings.policy.off")}</option>
            <option value="block-critical">{t("settings.policy.critical")}</option>
            <option value="trusted-only">{t("settings.policy.trusted")}</option>
          </select>
        </div>
        <label className="install-switch">
          <input type="checkbox" checked={policy.block_risky_content} onChange={event => value.update("block_risky_content", event.target.checked)} disabled={!enforced} />
          <span>{t("settings.policy.risky")}</span>
        </label>
        <div className="form settings-section">
          <label htmlFor="trusted-git-hosts">{t("settings.policy.gitHosts")}</label>
          <textarea id="trusted-git-hosts" value={policy.trusted_git_hosts.join("\n")} onChange={event => value.update("trusted_git_hosts", splitPolicyEntries(event.target.value))} placeholder="github.com&#10;gitlab.com" disabled={policy.mode !== "trusted-only"} />
        </div>
        <div className="form">
          <label htmlFor="trusted-local-roots">{t("settings.policy.localRoots")}</label>
          <textarea id="trusted-local-roots" value={policy.trusted_local_roots.join("\n")} onChange={event => value.update("trusted_local_roots", splitPolicyEntries(event.target.value))} placeholder="~/Projects/skills" disabled={policy.mode !== "trusted-only"} />
        </div>
        <div className="git-meta">{t("settings.policy.help")}</div>
        {value.dirty && <div className="install-compact warn" role="status"><span>{t("settings.unsaved")}</span><strong>{t("settings.policy.unsavedHelp")}</strong></div>}
        {value.error && <div className="install-compact error" role="alert"><span>{t("settings.policy.error")}</span><strong>{value.error}</strong></div>}
        <ActionRow>
          <button className="btn btn-primary btn-sm" onClick={value.save} disabled={!value.dirty || value.saving || value.loading}><Icon name="check" size={14} />{t(value.saving ? "settings.saving" : "settings.policy.save")}</button>
          <button className="btn btn-secondary btn-sm" onClick={value.reload} disabled={value.saving || value.loading}><Icon name="refresh" size={14} />{t("settings.policy.reload")}</button>
        </ActionRow>
      </div>
    </div>
  );
}

function DataSettings({ value }) {
  const { t } = useI18n();
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="upload" size={20} /><h3>{t("settings.tabs.data")}</h3></div>
      <div className="settings-body">
        <div className="form"><label htmlFor="library-export-path">{t("settings.data.exportFile")}</label><input id="library-export-path" value={value.exportPath} onChange={event => value.setExportPath(event.target.value)} placeholder="~/skillmate-export.json" /></div>
        <ActionRow><button className="btn btn-primary btn-sm" onClick={value.exportLibrary}><Icon name="upload" size={14} />{t("settings.data.export")}</button></ActionRow>
        <div className="form settings-section"><label htmlFor="library-import-path">{t("settings.data.importFile")}</label><input id="library-import-path" value={value.importPath} onChange={event => value.updateImportPath(event.target.value)} placeholder="~/skillmate-export.json" /></div>
        <div className="form"><label htmlFor="library-import-mode">{t("settings.data.importMode")}</label><select id="library-import-mode" value={value.importMode} onChange={event => value.updateImportMode(event.target.value)}><option value="merge">{t("settings.data.merge")}</option><option value="replace">{t("settings.data.replace")}</option></select></div>
        <div className="git-meta">{t("settings.data.help")}</div>
        {value.importPreview && (
          <div className="import-preview">
            <div className="import-preview-head"><strong>{t(value.importPreview.replace_existing ? "settings.data.replacePreview" : "settings.data.mergePreview")}</strong><span>{t(value.importMode === "replace" ? "settings.data.replacePreviewHelp" : "settings.data.mergePreviewHelp")}</span></div>
            <SummaryList lines={buildImportPreviewSummary(value.importPreview, t)} />
          </div>
        )}
        <ActionRow>
          <button className="btn btn-secondary btn-sm" onClick={value.previewImport} disabled={value.previewingImport || value.applyingImport}><Icon name="preview" size={14} />{t(value.previewingImport ? "settings.data.previewing" : "settings.data.previewImport")}</button>
          <button className="btn btn-primary btn-sm" onClick={value.importLibrary} disabled={!value.importPreview || !value.importPreviewCurrent || value.applyingImport}><Icon name="check" size={14} />{t(value.applyingImport ? "settings.data.importing" : "settings.data.import")}</button>
        </ActionRow>
        <div className="form settings-section"><label htmlFor="scenario-manifest-path">{t("settings.data.scenarioManifest")}</label><input id="scenario-manifest-path" value={value.scenarioManifestPath} onChange={event => value.updateScenarioManifestPath(event.target.value)} placeholder="~/skillmate-scenarios.json" /></div>
        <div className="form"><label htmlFor="scenario-manifest-mode">{t("settings.data.scenarioMode")}</label><select id="scenario-manifest-mode" value={value.scenarioManifestMode} onChange={event => value.updateScenarioManifestMode(event.target.value)}><option value="merge">{t("settings.data.merge")}</option><option value="replace">{t("settings.data.replaceScenarios")}</option></select></div>
        <div className="git-meta">{t("settings.data.scenarioHelp")}</div>
        {value.scenarioManifestPreview && (
          <div className="import-preview">
            <div className="import-preview-head"><strong>{t(value.scenarioManifestPreview.replace_existing ? "settings.data.replaceScenarioPreview" : "settings.data.mergeScenarioPreview")}</strong><span>{t(value.scenarioManifestMode === "replace" ? "settings.data.replaceScenarioHelp" : "settings.data.mergeScenarioHelp")}</span></div>
            <SummaryList lines={buildScenarioManifestPreviewSummary(value.scenarioManifestPreview, t)} />
          </div>
        )}
        <ActionRow>
          <button className="btn btn-secondary btn-sm" onClick={value.exportScenarioManifest}><Icon name="upload" size={14} />{t("settings.data.exportScenario")}</button>
          <button className="btn btn-secondary btn-sm" onClick={value.previewScenarioManifest} disabled={value.previewingScenarioManifest || value.applyingScenarioManifest}><Icon name="preview" size={14} />{t(value.previewingScenarioManifest ? "settings.data.previewing" : "settings.data.previewScenario")}</button>
          <button className="btn btn-primary btn-sm" onClick={value.importScenarioManifest} disabled={!value.scenarioManifestPreview || !value.scenarioManifestPreviewCurrent || value.applyingScenarioManifest}><Icon name="check" size={14} />{t(value.applyingScenarioManifest ? "settings.data.importing" : "settings.data.importScenario")}</button>
        </ActionRow>
      </div>
    </div>
  );
}

function SkillSetSettings({ value }) {
  const { t } = useI18n();
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="skills" size={20} /><h3>Skill Set</h3></div>
      <div className="settings-body">
        <div className="form"><label htmlFor="project-manifest-root">{t("settings.skillset.projectLock")}</label><input id="project-manifest-root" value={value.projectManifestRoot} onChange={event => value.setProjectManifestRoot(event.target.value)} placeholder="/path/to/project" /></div>
        <div className="git-meta">{t("settings.skillset.projectHelp")}</div>
        <ActionRow>
          <button className="btn btn-secondary btn-sm" onClick={value.exportProjectManifest} disabled={!value.projectManifestRoot.trim()}><Icon name="lock" size={14} />{t("settings.skillset.exportProject")}</button>
        </ActionRow>
        <div className="form"><label htmlFor="skillmate-manifest-path">SkillMate manifest</label><input id="skillmate-manifest-path" value={value.manifestPath} onChange={event => value.updateManifestPath(event.target.value)} placeholder="~/skillmate.toml" /></div>
        <div className="git-meta">{t("settings.skillset.manifestHelp")}</div>
        {value.manifestPreview && (
          <div className="import-preview">
            <div className="import-preview-head"><strong>{t("settings.skillset.manifestPreview")}</strong><span>{t(value.manifestPreview.can_apply ? "settings.skillset.ready" : "settings.skillset.conflict")}</span></div>
            <SummaryList lines={buildSkillMateManifestPreviewSummary(value.manifestPreview, t)} />
          </div>
        )}
        <ActionRow>
          <button className="btn btn-secondary btn-sm" onClick={value.exportManifest}><Icon name="upload" size={14} />{t("settings.skillset.export")}</button>
          <button className="btn btn-secondary btn-sm" onClick={value.previewManifest} disabled={value.previewingManifest || value.applyingManifest}><Icon name="preview" size={14} />{t(value.previewingManifest ? "settings.data.previewing" : "settings.skillset.preview")}</button>
          <button className="btn btn-primary btn-sm" onClick={value.applyManifest} disabled={!value.manifestPreview || !value.manifestPreviewCurrent || !value.manifestPreview.can_apply || value.applyingManifest}><Icon name="check" size={14} />{t(value.applyingManifest ? "settings.skillset.applying" : "settings.skillset.apply")}</button>
        </ActionRow>
        <div className="form settings-section"><label htmlFor="profile-name">Skill Set Profile</label><input id="profile-name" value={value.profileName} onChange={event => value.setProfileName(event.target.value)} placeholder={t("settings.skillset.profilePlaceholder")} /></div>
        <div className="form"><label htmlFor="profile-description">{t("settings.skillset.description")}</label><input id="profile-description" value={value.profileDescription} onChange={event => value.setProfileDescription(event.target.value)} placeholder={t("settings.skillset.descriptionPlaceholder")} /></div>
        <div className="git-meta">{t("settings.skillset.profileHelp")}</div>
        <ActionRow>
          <button className="btn btn-secondary btn-sm" onClick={value.saveProfile}><Icon name="check" size={14} />{t("settings.skillset.saveProfile")}</button>
          <button className="btn btn-secondary btn-sm" onClick={value.rollbackProfile} disabled={!value.profiles.previous_active_profile_id || value.applyingProfile}><Icon name="refresh" size={14} />{t("settings.skillset.rollback")}</button>
        </ActionRow>
        {value.profiles.profiles?.length > 0 && (
          <div className="scenario-detail settings-action-row">
            {value.profiles.profiles.map((profile) => (
              <div key={profile.id} className="scenario-path-row">
                <div><strong>{profile.name}{profile.active ? ` · ${t("settings.skillset.current")}` : ""}</strong><div className="card-path profile-description">{profile.description || t("settings.skillset.records", { count: profile.skills.length })}</div></div>
                <div className="card-actions">
                  <button className="btn btn-secondary btn-sm" onClick={() => value.previewProfile(profile.id)} disabled={value.previewingProfile || value.applyingProfile}><Icon name="preview" size={14} />{t("settings.skillset.previewProfile")}</button>
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
                  ><Icon name="check" size={14} />{t("settings.skillset.applyPreview")}</button>
                </div>
              </div>
            ))}
          </div>
        )}
        {value.profilePreview && (
          <div className="import-preview">
            <div className="import-preview-head"><strong>{t("settings.skillset.profilePreview")}</strong><span>{t(value.profilePreview.manifest_preview?.can_apply && !value.profilePreview.profile_issues?.length ? "settings.skillset.ready" : "settings.skillset.issue")}</span></div>
            <SummaryList lines={buildSkillProfilePreviewSummary(value.profilePreview, t)} />
          </div>
        )}
      </div>
    </div>
  );
}

function TagSettings({ value }) {
  const { t } = useI18n();
  return (
    <div className="settings-card">
      <div className="settings-head"><Icon name="tag" size={20} /><h3>{t("settings.tags.title")}</h3></div>
      <div className="settings-body">
        <div className="tag-form"><input aria-label={t("settings.tags.name")} value={value.name} onChange={event => value.setName(event.target.value)} placeholder={t("settings.tags.name")} /><input aria-label={t("settings.tags.color")} type="color" value={value.color} onChange={event => value.setColor(event.target.value)} /><button className="btn btn-primary btn-sm" onClick={value.add}><Icon name="plus" size={14} />{t("settings.tags.add")}</button></div>
        <div className="tag-list settings-action-row">{value.tags.map(tag => <div key={tag.id} className="tag-chip active" style={{ "--c": tag.color }}><span className="tag-dot" />{tag.name}</div>)}</div>
      </div>
    </div>
  );
}

export default function SettingsView({ activeTab, setActiveTab, backup, appUpdate, installPolicy, data, skillSet, tags }) {
  const { t, language, setLanguage } = useI18n();
  return (
    <div className="settings">
      <div className="content-head"><div><h2>{t("nav.settings")}</h2></div></div>
      <div className="settings-language">
        <div><Icon name="globe" size={18} /><span><strong>{t("settings.language")}</strong><small>{t("settings.languageHint")}</small></span></div>
        <select aria-label={t("settings.language")} value={language} onChange={(event) => setLanguage(event.target.value)}><option value="zh-CN">中文</option><option value="en">English</option></select>
      </div>
      <div className="sort-tabs settings-tabs" role="tablist" aria-label={t("settings.category")}>
        {SETTINGS_TABS.map(([key, labelKey]) => (
          <button key={key} role="tab" aria-selected={activeTab === key} className={`sort-tab ${activeTab === key ? "active" : ""}`} onClick={() => setActiveTab(key)}>{t(labelKey)}</button>
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
