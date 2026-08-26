import { useEffect, useId, useMemo, useState } from "react";
import Icon from "./Icon.jsx";
import ModalShell from "./ModalShell.jsx";
import {
  SUPPORTED_INSTALL_SOURCES,
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
  const {
    source: {
      kind: src,
      setKind: setSrc,
      package: pkg,
      setPackage: setPkg,
      detectionView: installDetectionView,
    },
    target: {
      assistant: installAssistant,
      setAssistant: setInstallAssistant,
      mode: installMode,
      setMode: setInstallMode,
      projectPath,
      setProjectPath,
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
    <ModalShell title={t("install.title")} icon="plus" className="install-modal" onClose={onClose}>
      <div className="form">
        <label htmlFor="install-source">{t("install.source")}</label>
        <input id="install-source" value={pkg} onChange={e => setPkg(e.target.value)} placeholder={t("install.sourcePlaceholder")} />
      </div>
      {installDetectionView && (
        <div className={`install-compact ${installDetectionView.tone}`}>
          <span>{installDetectionView.sourceLabel}</span>
          <strong>{installDetectionView.summary}</strong>
          {installDetectionView.warningSummary && <p>{installDetectionView.warningSummary}</p>}
        </div>
      )}
      <div className="install-target">
        <div className="form">
          <label htmlFor="install-assistant">{t("install.target")}</label>
          <select id="install-assistant" value={installAssistant} onChange={e => setInstallAssistant(e.target.value)}>
            {assistants.map((assistant) => (
              <option key={assistant.name} value={assistant.name}>{assistant.name}</option>
            ))}
          </select>
        </div>
        {showProjectLinkOption && (
          <label className="install-switch">
            <input type="checkbox" checked={installMode === "symlink"} onChange={e => setInstallMode(e.target.checked ? "symlink" : "copy")} />
            <span>{t("install.linkProject")}</span>
          </label>
        )}
      </div>
      {showProjectLinkOption && installMode === "symlink" && (
        <div className="install-project">
          <div className="form">
            <label htmlFor="install-project-path">{t("install.projectPath")}</label>
            <input id="install-project-path" value={projectPath} onChange={e => setProjectPath(e.target.value)} placeholder="/path/to/project" />
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
      {(showInstallAdvancedOptions || installAdvancedOpen) && (
        <div className="install-advanced">
          <div className="form">
            <label htmlFor="install-source-kind">{t("install.sourceType")}</label>
            <select id="install-source-kind" value={src} onChange={e => setSrc(e.target.value)}>
              {SUPPORTED_INSTALL_SOURCES.map((source) => (
                <option key={source} value={source}>{t(source === "git" ? "install.git" : "install.local")}</option>
              ))}
            </select>
          </div>
        </div>
      )}
      {installStructurePreview && (
        <div className={`structure-preview install-preview-card ${installPreviewView?.tone || (installStructurePreview.can_install === false ? "error" : getStructureStatusTone(installStructurePreview.structure_status))}`}>
          <div className="structure-preview-head">
            <span>{t("install.plan")}</span>
            <strong>{t(installPreviewView?.canApply && installPreviewCurrent ? "install.ready" : "install.needsCheck")}</strong>
          </div>
          <ul className="install-summary-list">
            {buildInstallPreviewSummary(installStructurePreview, t).slice(0, 4).map((line, index) => (
              <li key={`${line}-${index}`}>{line}</li>
            ))}
          </ul>
          {!installPreviewCurrent && <p>{t("install.stale")}</p>}
        </div>
      )}
      <button className="btn btn-primary full install-primary" onClick={runInstallPrimaryAction} disabled={installPrimaryAction.disabled || loading}>
        <Icon name={installPrimaryAction.icon} size={16} />{t(installPrimaryAction.action === "install" ? "install.install" : "install.review")}
      </button>
      <div className="install-secondary-actions">
        <button className="btn btn-ghost btn-sm" onClick={() => setInstallDetailsOpen(!installDetailsOpen)}>
          <Icon name="preview" size={14} />{t(installDetailsOpen ? "install.hideDetails" : "install.showDetails")}
        </button>
        <button className="btn btn-ghost btn-sm" onClick={() => setInstallAdvancedOpen(!installAdvancedOpen)}>
          <Icon name="settings" size={14} />{t(installAdvancedOpen ? "install.hideAdvanced" : "install.showAdvanced")}
        </button>
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

export function PreviewModal({ preview, onClose, onCheckUpdate, onOpenDrift, driftGroup }) {
  const { t } = useI18n();
  const skill = preview.skill || {};
  const card = buildSkillCardView(skill, t);
  const sourceLocator = skill.origin_locator || skill.upstream_url || "—";
  const resolvedLocator = skill.resolved_locator || skill.upstream_url || "—";
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
          <p>{skill.can_sync ? t("preview.updatable") : t("preview.notUpdatable")}</p>
          <div className="skill-detail-actions">
            {skill.path && <button className="btn btn-secondary btn-sm" onClick={() => onCheckUpdate?.(skill.path)} disabled={!skill.can_sync}><Icon name="refresh" size={14} />{t("preview.checkUpdate")}</button>}
            {driftGroup && <button className="btn btn-secondary btn-sm" onClick={() => onOpenDrift?.(driftGroup)}><Icon name="branch" size={14} />{t("preview.openDrift")}</button>}
          </div>
        </section>
      </div>
      <section className="skill-detail-section skill-provenance">
        <div className="skill-detail-title"><Icon name="branch" size={17} /><strong>{t("preview.provenance")}</strong><span className={`structure-badge ${skill.managed_by_app ? "success" : ""}`}>{skill.managed_by_app ? t("common.managed") : t("common.unmanaged")}</span></div>
        <dl>
          <div><dt>{t("preview.sourceKind")}</dt><dd>{skill.source_type || skill.origin_kind || "—"}</dd></div>
          <div><dt>{t("preview.origin")}</dt><dd title={sourceLocator}>{sourceLocator}</dd></div>
          <div><dt>{t("preview.resolved")}</dt><dd title={resolvedLocator}>{resolvedLocator}</dd></div>
          <div><dt>{t("preview.tracking")}</dt><dd>{skill.tracking_ref || "—"}</dd></div>
          <div><dt>{t("preview.installed")}</dt><dd>{skill.installed_ref || "—"}</dd></div>
          <div><dt>{t("preview.latest")}</dt><dd>{skill.latest_ref || "—"}</dd></div>
          <div><dt>{t("preview.hash")}</dt><dd>{skill.content_hash || "—"}</dd></div>
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
