import { useEffect, useId, useMemo, useState } from "react";
import Icon from "./Icon.jsx";
import ModalShell from "./ModalShell.jsx";
import {
  SUPPORTED_INSTALL_SOURCES,
  buildInstallPreviewView,
  buildInstallPreviewSummary,
  buildProjectTargetPreviewSummary,
  buildSkillCardView,
  buildStructureWarningSummary,
  buildValidationSummary,
  getStructureStatusLabel,
  getStructureStatusTone,
} from "../lib/skillmate.mjs";
import { skillmateApi } from "../lib/skillmateApi.js";
import { useI18n } from "../lib/i18n.jsx";
import { toUserErrorMessage } from "../lib/errorMessage.mjs";

function shortHash(value = "") {
  return value.replace(/^sha256:/, "").slice(0, 10) || "—";
}

export function DriftSyncModal({ group, onClose, onComplete }) {
  const { t } = useI18n();
  const sourceCopies = useMemo(
    () => group.copies.filter((copy) => copy.structureStatus === "complete" && copy.sourceType !== "symlink"),
    [group],
  );
  const [sourcePath, setSourcePath] = useState(sourceCopies[0]?.path || group.copies[0]?.path || "");
  const eligibleTargets = useMemo(
    () => group.copies.filter((copy) => copy.path !== sourcePath && copy.managed && copy.sourceType !== "symlink"),
    [group, sourcePath],
  );
  const [targetPaths, setTargetPaths] = useState([]);
  const [preview, setPreview] = useState(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    setTargetPaths(eligibleTargets.map((copy) => copy.path));
    setPreview(null);
    setError("");
  }, [eligibleTargets]);

  function toggleTarget(path) {
    setTargetPaths((current) => current.includes(path) ? current.filter((item) => item !== path) : [...current, path]);
    setPreview(null);
  }

  async function previewPlan() {
    setBusy("preview");
    setError("");
    try {
      setPreview(await skillmateApi.drift.preview(sourcePath, targetPaths));
    } catch (reason) {
      setError(t("drift.error", { message: toUserErrorMessage(reason, t("error.safeRetry")) }));
    } finally {
      setBusy("");
    }
  }

  async function applyPlan() {
    if (!preview?.planToken && !preview?.plan_token) return;
    setBusy("apply");
    setError("");
    try {
      const message = await skillmateApi.drift.apply(sourcePath, targetPaths, preview.planToken || preview.plan_token);
      await onComplete(message || t("drift.complete"));
      onClose();
    } catch (reason) {
      setError(t("drift.error", { message: toUserErrorMessage(reason, t("error.safeRetry")) }));
    } finally {
      setBusy("");
    }
  }

  const actions = preview?.actions || [];
  const conflicts = preview?.conflicts || [];
  const canApply = Boolean(preview?.canApply ?? preview?.can_apply) && actions.length > 0;
  return (
    <ModalShell title={`${t("drift.title")} · ${group.name}`} icon="branch" className="large drift-modal" onClose={onClose}>
      <p className="modal-intro">{t("drift.subtitle")}</p>
      <div className="drift-section">
        <label htmlFor="drift-source">{t("drift.baseline")}</label>
        <select id="drift-source" value={sourcePath} onChange={(event) => setSourcePath(event.target.value)}>
          {(sourceCopies.length ? sourceCopies : group.copies).map((copy) => <option key={copy.path} value={copy.path}>{copy.availableIn.map((item) => item.name).join(" / ")} · {shortHash(copy.contentHash)}</option>)}
        </select>
      </div>
      <fieldset className="drift-section"><legend>{t("drift.targets")}</legend>
        <div className="drift-copy-list">
          {eligibleTargets.map((copy) => <label key={copy.path} className="drift-copy"><input type="checkbox" checked={targetPaths.includes(copy.path)} onChange={() => toggleTarget(copy.path)} /><span><strong>{copy.availableIn.map((item) => item.name).join(" / ")}</strong><small>{copy.path}</small></span><code>{shortHash(copy.contentHash)}</code></label>)}
          {eligibleTargets.length === 0 && <p className="empty-hint">{t("drift.noTargets")}</p>}
        </div>
      </fieldset>
      {preview && <div className={`install-compact ${canApply ? "success" : "error"}`}><strong>{canApply ? t("drift.planReady", { count: actions.length }) : t("drift.planBlocked", { count: conflicts.length })}</strong>
        {actions.map((action) => <span key={action.targetPath || action.target_path}>{t("drift.fromTo", { assistant: action.assistant, before: shortHash(action.beforeHash || action.before_hash), after: shortHash(action.afterHash || action.after_hash) })}</span>)}
        {conflicts.map((conflict) => <span key={conflict.targetPath || conflict.target_path}>{conflict.message}</span>)}
      </div>}
      {error && <div className="install-compact error" role="alert"><strong>{error}</strong></div>}
      <div className="modal-actions"><button className="btn btn-secondary" onClick={previewPlan} disabled={!targetPaths.length || Boolean(busy)}><Icon name="preview" size={15} />{busy === "preview" ? t("drift.previewing") : t("drift.preview")}</button><button className="btn btn-primary" onClick={applyPlan} disabled={!canApply || Boolean(busy)}><Icon name="check" size={15} />{busy === "apply" ? t("drift.applying") : t("drift.apply")}</button></div>
    </ModalShell>
  );
}

export function InstallModal({
  flow,
  assistants,
  loading,
  onClose,
}) {
  const { t } = useI18n();
  const workflow = flow.workflow || "add";
  const {
    source: {
      kind: src,
      setKind: setSrc,
      package: pkg,
      paths = [pkg],
      setPackage: setPkg,
      detectionView: installDetectionView,
    },
    target: {
      assistants: installAssistants = [],
      toggleAssistant,
      mode: installMode,
      setMode: setInstallMode,
      projectPath,
      setProjectPath,
      pickProjectDirectory,
      projectPreview: projectTargetPreview,
      previewingProject: previewingProjectTargets,
      showProjectLinkOption,
    },
    preview: {
      structure: installStructurePreview,
      view: installPreviewView,
      current: installPreviewCurrent,
      primaryAction: installPrimaryAction,
      runPrimaryAction: runInstallPrimaryAction,
    },
    selection = {
      availableSkills: [],
      selectedPaths: [],
      required: false,
      toggle: () => {},
    },
    disclosure: {
      detailsOpen: installDetailsOpen,
      setDetailsOpen: setInstallDetailsOpen,
      advancedOpen: installAdvancedOpen,
      setAdvancedOpen: setInstallAdvancedOpen,
      showAdvancedOptions: showInstallAdvancedOptions,
    },
    commandPreview: cmd,
  } = flow;
  return (
    <ModalShell title={t(workflow === "enable" ? "enable.title" : "install.title")} icon={workflow === "enable" ? "check" : "plus"} className="install-modal" onClose={onClose}>
      {workflow === "add" ? (
        <div className="form">
          <label htmlFor="install-source">{t("install.source")}</label>
          <input id="install-source" value={pkg} onChange={e => setPkg(e.target.value)} placeholder={t(src === "claude_marketplace" ? "install.claudeMarketplacePlaceholder" : "install.sourcePlaceholder")} />
        </div>
      ) : (
        <div className="install-compact success">
          <span>{t("enable.librarySource")}</span>
          <div className="install-skill-options">
            {paths.map((path) => (
              <div key={path}>
                <strong>{path.split(/[\\/]/).filter(Boolean).pop() || path}</strong>
                <p className="registry-path" title={path}>{path}</p>
              </div>
            ))}
          </div>
        </div>
      )}
      {workflow === "add" && installDetectionView && (
        <div className={`install-compact ${installDetectionView.tone}`}>
          <span>{installDetectionView.sourceLabel}</span>
          <strong>{installDetectionView.summary}</strong>
          {installDetectionView.warningSummary && <p>{installDetectionView.warningSummary}</p>}
        </div>
      )}
      {workflow === "add" && (selection.required || selection.availableSkills.length > 1) && selection.availableSkills.length > 0 && (
        <fieldset className="install-skill-selection">
          <legend>{t("install.selectSkills")}</legend>
          <p>{t("install.selectSkillsHint", { count: selection.availableSkills.length })}</p>
          <div className="install-skill-options">
            {selection.availableSkills.map((skill) => {
              const title = skill.title || skill.relative_path.split("/").pop();
              return (
                <label className="install-skill-option" key={skill.relative_path}>
                  <input
                    type="checkbox"
                    checked={selection.selectedPaths.includes(skill.relative_path)}
                    onChange={() => selection.toggle(skill.relative_path)}
                  />
                  <span>
                    <strong>{title}</strong>
                    <small>{skill.relative_path}</small>
                    {skill.description && <small>{skill.description}</small>}
                  </span>
                </label>
              );
            })}
          </div>
          {selection.required && selection.selectedPaths.length === 0 && (
            <p className="install-selection-required" role="status">{t("install.selectionRequired")}</p>
          )}
        </fieldset>
      )}
      {workflow === "enable" && (
        <div className="install-target">
          <fieldset className="install-assistant-field">
            <legend>{t("install.target")}</legend>
            <div className="install-assistant-options">
              {assistants.map((assistant) => {
                const unavailable = installMode === "symlink" && !assistant.supports_project_skills;
                return (
                  <label className={`install-assistant-option ${unavailable ? "disabled" : ""}`} key={assistant.name}>
                    <input
                      type="checkbox"
                      checked={installAssistants.includes(assistant.name)}
                      disabled={unavailable}
                      onChange={() => toggleAssistant(assistant.name)}
                    />
                    <span>{assistant.name}</span>
                  </label>
                );
              })}
            </div>
            <small className="form-help">{t("enable.targetHint.multiple")}</small>
          </fieldset>
          <div className="form install-scope-field">
            <label htmlFor="install-scope">{t("install.scope")}</label>
            <select id="install-scope" value={installMode} onChange={e => setInstallMode(e.target.value)}>
              <option value="copy">{t("install.scope.global")}</option>
              {showProjectLinkOption && <option value="symlink">{t("install.scope.project")}</option>}
            </select>
            <small className="form-help">{t(showProjectLinkOption ? (installMode === "symlink" ? "enable.targetHint.project" : "enable.targetHint.global") : "install.targetHint.globalOnly")}</small>
          </div>
        </div>
      )}
      {workflow === "enable" && showProjectLinkOption && installMode === "symlink" && (
        <div className="install-project">
          <div className="form">
            <label htmlFor="install-project-path">{t("install.projectPath")}</label>
            <div className="project-path-control">
              <input id="install-project-path" value={projectPath} onChange={e => setProjectPath(e.target.value)} placeholder="/path/to/project" />
              <button className="btn btn-secondary" type="button" onClick={pickProjectDirectory}><Icon name="folder" size={15} />{t("install.chooseProject")}</button>
            </div>
          </div>
          {previewingProjectTargets && <div className="git-meta">{t("install.detectingTargets")}</div>}
          {projectTargetPreview.length > 0 && (
            <ul className="import-preview-list">
              {buildProjectTargetPreviewSummary(projectTargetPreview, t).map((line, index) => (
                <li key={`${line}-${index}`}>{line}</li>
              ))}
            </ul>
          )}
        </div>
      )}
      {workflow === "add" && (showInstallAdvancedOptions || installAdvancedOpen) && (
        <div className="install-advanced">
          <div className="form">
            <label htmlFor="install-source-kind">{t("install.sourceType")}</label>
            <select id="install-source-kind" value={src} onChange={e => setSrc(e.target.value)}>
              {SUPPORTED_INSTALL_SOURCES.map((source) => (
                <option key={source} value={source}>{t(source === "git" ? "install.git" : source === "local" ? "install.local" : "install.claudeMarketplace")}</option>
              ))}
            </select>
          </div>
        </div>
      )}
      {installStructurePreview && (
        <div className={`structure-preview install-preview-card ${installPreviewView?.tone || (installStructurePreview.can_install === false ? "error" : getStructureStatusTone(installStructurePreview.structure_status))}`}>
          <div className="structure-preview-head">
            <span>{t(workflow === "enable" ? "enable.plan" : "install.plan")}</span>
            <strong>{t(installPreviewView?.canApply && installPreviewCurrent
              ? (workflow === "enable" ? "enable.ready" : "install.ready")
              : "install.needsCheck")}</strong>
          </div>
          <ul className="install-summary-list">
            {buildInstallPreviewSummary(installStructurePreview, t).map((line, index) => (
              <li key={`${line}-${index}`}>{line}</li>
            ))}
          </ul>
          {installPreviewView?.actions?.length > 0 && (
            <div className="install-plan-actions" aria-label={t("install.planWrites")}>
              {installPreviewView.actions.map((action) => {
                const reason = installPreviewView.conflicts.find((conflict) => conflict.target === action.target)?.reason
                  || action.reason;
                return (
                  <div key={`${action.action}-${action.target}`}>
                    <span className="stamp muted">{action.label}</span>
                    <span>
                      <strong>{reason}</strong>
                      <small title={action.target}>{action.target}</small>
                    </span>
                  </div>
                );
              })}
            </div>
          )}
          {!installPreviewCurrent && <p>{t("install.stale")}</p>}
        </div>
      )}
      <button className="btn btn-primary full install-primary" onClick={runInstallPrimaryAction} disabled={installPrimaryAction.disabled || loading}>
        <Icon name={installPrimaryAction.icon} size={16} />{installPrimaryAction.label}
      </button>
      <div className="install-secondary-actions">
        <button className="btn btn-ghost btn-sm" onClick={() => setInstallDetailsOpen(!installDetailsOpen)}>
          <Icon name="preview" size={14} />{t(installDetailsOpen ? "install.hideDetails" : "install.showDetails")}
        </button>
        {workflow === "add" && (
          <button className="btn btn-ghost btn-sm" onClick={() => setInstallAdvancedOpen(!installAdvancedOpen)}>
            <Icon name="settings" size={14} />{t(installAdvancedOpen ? "install.hideAdvanced" : "install.showAdvanced")}
          </button>
        )}
      </div>
      {installDetailsOpen && (
        <div className="install-details">
          <div className="form"><span className="form-label">{t("install.execution")}</span><div className="cmd">{cmd}</div></div>
          {installStructurePreview && (
            <>
              <p>{buildStructureWarningSummary(installStructurePreview, t)}</p>
              {installPreviewView?.packageWarnings && <p>{installPreviewView.packageWarnings}</p>}
              {installPreviewView?.needsModel && <p>{t("install.modelHint")}</p>}
              {installPreviewView?.policy?.message && (
                <div className={`install-compact ${installPreviewView.policy.allowed ? "success" : "error"}`}>
                  <span>{t("install.policy")}</span>
                  <strong>{installPreviewView.policy.message}</strong>
                </div>
              )}
              {installPreviewView?.policy?.findings?.length > 0 && (
                <ul className={`import-preview-list ${installPreviewView.policy.allowed ? "" : "danger"}`}>
                  {installPreviewView.policy.findings.map((finding, index) => (
                    <li key={`${finding.code}-${index}`}>{finding.label}：{finding.message}</li>
                  ))}
                </ul>
              )}
              {installPreviewView?.skills?.length > 0 && (
                <ul className="import-preview-list">
                  {installPreviewView.skills.map((skill) => (
                    <li key={skill.relative_path}>{skill.relative_path} · {getStructureStatusLabel(skill.structure_status, t)}</li>
                  ))}
                </ul>
              )}
              {installPreviewView?.actions?.length > 0 && (
                <ul className="import-preview-list">
                  {installPreviewView.actions.map((action) => (
                    <li key={`${action.action}-${action.target}`}>{action.label}：{action.source} → {action.target}</li>
                  ))}
                </ul>
              )}
              {installPreviewView?.conflicts?.length > 0 && (
                <ul className="import-preview-list danger">
                  {installPreviewView.conflicts.map((conflict) => (
                    <li key={`${conflict.reason}-${conflict.target}`}>{conflict.target}：{conflict.reason}</li>
                  ))}
                </ul>
              )}
            </>
          )}
        </div>
      )}
    </ModalShell>
  );
}

export function AdoptionModal({ candidate, onClose, onComplete }) {
  const { t } = useI18n();
  const [preview, setPreview] = useState(null);
  const [busy, setBusy] = useState("preview");
  const [error, setError] = useState("");
  const previewView = useMemo(() => buildInstallPreviewView(preview, t), [preview, t]);

  useEffect(() => {
    let cancelled = false;
    setBusy("preview");
    skillmateApi.adoption.preview({
      path: candidate.skill.path,
      assistantName: candidate.assistant,
      projectPath: candidate.projectPath || undefined,
    }).then((result) => {
      if (!cancelled) setPreview(result);
    }).catch((reason) => {
      if (!cancelled) setError(String(reason));
    }).finally(() => {
      if (!cancelled) setBusy("");
    });
    return () => { cancelled = true; };
  }, [candidate]);

  async function apply() {
    if (!preview?.plan_token || !previewView?.canApply) return;
    setBusy("apply");
    setError("");
    try {
      const result = await skillmateApi.adoption.apply({
        path: candidate.skill.path,
        assistantName: candidate.assistant,
        projectPath: candidate.projectPath || undefined,
        planToken: preview.plan_token,
      });
      if (!result.success) throw new Error(result.message || result.output || t("adoption.failed"));
      await onComplete(result.message);
      onClose();
    } catch (reason) {
      setError(toUserErrorMessage(reason, t("error.safeRetry")));
    } finally {
      setBusy("");
    }
  }

  return (
    <ModalShell title={t("adoption.title")} icon="branch" className="large adoption-modal" onClose={onClose}>
      <p className="modal-intro">{t("adoption.hint")}</p>
      <div className="install-compact warn">
        <span>{candidate.assistant} · {candidate.projectPath ? t("projectInspection.scope.project") : t("projectInspection.scope.global")}</span>
        <strong>{candidate.skill.manifest_title || candidate.skill.name}</strong>
        <p className="registry-path">{candidate.skill.path}</p>
      </div>
      {busy === "preview" && <div className="install-compact"><strong>{t("adoption.previewing")}</strong></div>}
      {preview && <div className={`structure-preview install-preview-card ${previewView?.canApply ? "success" : "error"}`}>
        <div className="structure-preview-head"><span>{t("adoption.plan")}</span><strong>{preview.message}</strong></div>
        {previewView?.actions?.length > 0 && <div className="install-plan-actions">
          {previewView.actions.map((action) => <div key={`${action.action}-${action.target}`}><span className="stamp muted">{action.label}</span><span><strong>{action.source}</strong><small>{action.target}</small></span></div>)}
        </div>}
        {previewView?.conflicts?.length > 0 && <ul className="import-preview-list danger">{previewView.conflicts.map((conflict) => <li key={`${conflict.reason}-${conflict.target}`}>{conflict.target}：{conflict.reason}</li>)}</ul>}
      </div>}
      {error && <div className="install-compact error" role="alert"><strong>{error}</strong></div>}
      <div className="modal-actions">
        <button className="btn btn-secondary" onClick={onClose} disabled={busy === "apply"}>{t("common.cancel")}</button>
        <button className="btn btn-primary" onClick={apply} disabled={!previewView?.canApply || Boolean(busy)}><Icon name="check" size={15} />{busy === "apply" ? t("adoption.applying") : t("adoption.confirm")}</button>
      </div>
    </ModalShell>
  );
}

export function PreviewModal({ preview, getSyncInfo, onClose, onCheckUpdate, onOpenDrift, driftGroup }) {
  const { t } = useI18n();
  const skill = preview.skill || {};
  const syncInfo = getSyncInfo?.(skill);
  const liveSkill = syncInfo ? {
    ...skill,
    origin_kind: syncInfo.originKind ?? skill.origin_kind,
    origin_locator: syncInfo.originLocator ?? skill.origin_locator,
    resolved_locator: syncInfo.resolvedLocator ?? skill.resolved_locator,
    tracking_ref: syncInfo.trackingRef ?? skill.tracking_ref,
    installed_ref: syncInfo.installedRef ?? skill.installed_ref,
    latest_ref: syncInfo.latestRef ?? skill.latest_ref,
    sync_state: syncInfo.syncState ?? skill.sync_state,
    sync_message: syncInfo.message ?? skill.sync_message,
    lag_count: syncInfo.lagCount ?? skill.lag_count,
    last_probe_at: syncInfo.lastProbeAt ?? skill.last_probe_at,
    last_sync_at: syncInfo.lastSyncAt ?? skill.last_sync_at,
    managed_by_app: syncInfo.managedByApp ?? skill.managed_by_app,
    can_check: syncInfo.canCheck ?? skill.can_check,
    can_sync: syncInfo.canSync ?? skill.can_sync,
  } : skill;
  const card = buildSkillCardView(liveSkill, t);
  const sourceLocator = liveSkill.origin_locator || liveSkill.upstream_url || "—";
  const resolvedLocator = liveSkill.resolved_locator || liveSkill.upstream_url || "—";
  return (
    <ModalShell title={preview.title} className="large" onClose={onClose}>
      {preview.diagnostics?.length > 0 && (
        <div className="install-compact warn" role="status">
          <span>{t("install.partialPreview")}</span>
          <strong>{preview.diagnostics.map((item) => `${t(`diagnostic.${item.section}`)}: ${item.message}`).join(t("common.messageSeparator"))}</strong>
        </div>
      )}
      {preview.validation && (
        <div className="skill-detail-section">
          <div className="import-preview-head">
            <strong>{t("preview.structure")}</strong>
            <span>{getStructureStatusLabel(preview.validation.structure_status, t)}</span>
          </div>
          <ul className="import-preview-list">
            {buildValidationSummary(preview.validation, t).map((check) => (
              <li key={check.code}>{check.code}：{check.label} · {check.message}</li>
            ))}
          </ul>
        </div>
      )}
      <div className="skill-detail-grid">
        <section className={`skill-detail-section ${card.securityWarningCount || card.hasManagedDrift ? "danger" : "success"}`}>
          <div className="skill-detail-title"><Icon name="shield" size={17} /><strong>{t("preview.security")}</strong></div>
          <p>{card.securityWarningCount > 0 ? t("preview.staticRisks", { count: card.securityWarningCount }) : t("preview.staticSafe")}</p>
          {card.securityWarningSummary && <small>{card.securityWarningSummary}</small>}
          {card.hasManagedDrift && <small className="danger-text">{t("preview.managedChanged")}</small>}
        </section>
        <section className="skill-detail-section">
          <div className="skill-detail-title"><Icon name="updates" size={17} /><strong>{t("preview.update")}</strong></div>
          <p>{card.updateSummary}</p>
          <div className="skill-detail-actions">
            {liveSkill.path && <button className="btn btn-secondary btn-sm" onClick={() => onCheckUpdate?.(liveSkill.path)} disabled={!card.canCheck || syncInfo?.checking}><Icon name="refresh" size={14} />{t("preview.checkUpdate")}</button>}
            {driftGroup && <button className="btn btn-secondary btn-sm" onClick={() => onOpenDrift?.(driftGroup)}><Icon name="branch" size={14} />{t("preview.openDrift")}</button>}
          </div>
        </section>
      </div>
      <section className="skill-detail-section skill-provenance">
        <div className="skill-detail-title"><Icon name="branch" size={17} /><strong>{t("preview.provenance")}</strong><span className={`structure-badge ${liveSkill.managed_by_app ? "success" : ""}`}>{liveSkill.managed_by_app ? t("common.managed") : t("common.unmanaged")}</span></div>
        <dl>
          <div><dt>{t("provenance.sourceLabel")}</dt><dd>{card.contentSourceLabel}</dd></div>
          <div><dt>{t("provenance.managerLabel")}</dt><dd>{card.managerLabel}</dd></div>
          <div><dt>{t("provenance.updateLabel")}</dt><dd>{card.updateStrategyLabel}</dd></div>
          <div><dt>{t("preview.sourceKind")}</dt><dd>{liveSkill.origin_kind || liveSkill.source_type || "—"}</dd></div>
          <div><dt>{t("preview.origin")}</dt><dd title={sourceLocator}>{sourceLocator}</dd></div>
          <div><dt>{t("preview.resolved")}</dt><dd title={resolvedLocator}>{resolvedLocator}</dd></div>
          <div><dt>{t("preview.tracking")}</dt><dd>{liveSkill.tracking_ref || "—"}</dd></div>
          <div><dt>{t("preview.installed")}</dt><dd>{liveSkill.installed_ref || "—"}</dd></div>
          <div><dt>{t("preview.latest")}</dt><dd>{liveSkill.latest_ref || "—"}</dd></div>
          <div><dt>{t("preview.hash")}</dt><dd>{liveSkill.content_hash || "—"}</dd></div>
        </dl>
      </section>
      <pre className="readme">{preview.content}</pre>
    </ModalShell>
  );
}

export function TagEditorModal({
  tagEditor,
  tags,
  toggleSkillTag,
  saveSkillTags,
  onClose,
}) {
  const { t } = useI18n();
  const batch = tagEditor.mode === "add";
  return (
    <ModalShell title={t(batch ? "tags.addMany" : "tags.edit")} onClose={onClose}>
      <p className="modal-intro">{batch
        ? t("tags.addManyHint", { count: tagEditor.skills.length })
        : tagEditor.skills[0]?.name}</p>
      <div className="tag-list">
        {tags.map((tag) => (
          <button
            key={tag.id}
            className={`tag-chip ${tagEditor.selected.includes(tag.id) ? "active" : ""}`}
            style={{ "--c": tag.color }}
            onClick={() => toggleSkillTag(tag.id)}
          >
            <span className="tag-dot" />
            {tag.name}
          </button>
        ))}
        {tags.length === 0 && <p className="empty-hint">{t("tags.createFirst")}</p>}
      </div>
      <div className="card-actions" style={{ justifyContent: "flex-end", marginTop: 20 }}>
        <button className="btn btn-secondary btn-sm" onClick={onClose}>{t("common.cancel")}</button>
        <button className="btn btn-primary btn-sm" onClick={saveSkillTags} disabled={batch && tagEditor.selected.length === 0}>{t(batch ? "tags.addAction" : "common.save")}</button>
      </div>
    </ModalShell>
  );
}

export function TagManagerModal({
  tags,
  name,
  color,
  setName,
  setColor,
  onAdd,
  onUpdate,
  onDelete,
  onClose,
}) {
  const { t } = useI18n();
  return (
    <ModalShell title={t("tags.manageTitle")} onClose={onClose}>
      <p className="modal-intro">{t("tags.manageHint")}</p>
      <form
        className="tag-form"
        onSubmit={(event) => {
          event.preventDefault();
          void onAdd();
        }}
      >
        <input aria-label={t("settings.tags.name")} value={name} onChange={(event) => setName(event.target.value)} placeholder={t("settings.tags.name")} />
        <input aria-label={t("settings.tags.color")} type="color" value={color} onChange={(event) => setColor(event.target.value)} />
        <button className="btn btn-primary btn-sm" type="submit"><Icon name="plus" size={14} />{t("settings.tags.add")}</button>
      </form>
      <div className="tag-manager-list">
        {tags.map((tag) => (
          <form
            className="tag-manager-row"
            key={tag.id}
            onSubmit={(event) => {
              event.preventDefault();
              const values = new FormData(event.currentTarget);
              void onUpdate(tag.id, values.get("name"), values.get("color"));
            }}
          >
            <input name="name" aria-label={t("tags.nameFor", { name: tag.name })} defaultValue={tag.name} required />
            <input name="color" aria-label={t("tags.colorFor", { name: tag.name })} type="color" defaultValue={tag.color} />
            <div className="tag-manager-actions">
              <button className="btn btn-secondary btn-sm" type="submit">{t("tags.save")}</button>
              <button className="btn btn-danger btn-sm" type="button" onClick={() => onDelete(tag)}>{t("tags.delete")}</button>
            </div>
          </form>
        ))}
        {tags.length === 0 && <p className="empty-hint">{t("tags.empty")}</p>}
      </div>
    </ModalShell>
  );
}

export function ConfirmModal({ confirmState, onClose, onConfirm }) {
  const { t } = useI18n();
  const descriptionId = useId();
  return (
    <ModalShell title={confirmState.title} onClose={onClose} role="alertdialog" descriptionId={descriptionId}>
      <p id={descriptionId} style={{ color: "var(--text2)", fontSize: "0.9rem", marginBottom: 20 }}>{confirmState.message}</p>
      <div className="card-actions" style={{ justifyContent: "flex-end" }}>
        <button className="btn btn-secondary btn-sm" onClick={onClose}>{t("common.cancel")}</button>
        <button
          className={`btn btn-${confirmState.tone === "primary" ? "primary" : "danger"} btn-sm`}
          onClick={onConfirm}
        >
          {confirmState.confirmLabel || t("common.confirm")}
        </button>
      </div>
    </ModalShell>
  );
}
