import { useEffect, useRef, useState } from "react";
import ModalShell from "./ModalShell.jsx";
import { useI18n } from "../lib/i18n.jsx";
import { skillmateApi } from "../lib/skillmateApi.js";

export default function UpdatePreviewModal({ path, onClose, onConfirm }) {
  const { t } = useI18n();
  const [preview, setPreview] = useState(null);
  const [error, setError] = useState("");
  const [checking, setChecking] = useState(true);
  const [applying, setApplying] = useState(false);
  const [revision, setRevision] = useState(0);
  const submitting = useRef(false);
  useEffect(() => {
    let cancelled = false;
    setChecking(true); setPreview(null); setError("");
    skillmateApi.updates.preview(path).then(value => {
      if (!cancelled) setPreview(value);
    }).catch(reason => { if (!cancelled) setError(String(reason)); })
      .finally(() => { if (!cancelled) setChecking(false); });
    return () => { cancelled = true; };
  }, [path, revision]);
  async function apply() {
    if (submitting.current || !preview?.canApply || !preview.planToken) return;
    submitting.current = true; setApplying(true);
    try {
      const success = await onConfirm(path, preview.planToken);
      if (success) onClose();
      else { setPreview(null); setError(t("updatePreview.retry")); }
    } catch (reason) { setPreview(null); setError(String(reason)); }
    finally { submitting.current = false; setApplying(false); }
  }
  return <ModalShell title={t("updatePreview.title")} icon="preview" className="update-preview-modal" onClose={() => { if (!submitting.current) onClose(); }}>
    <div className="modal-body">
      {checking && <p role="status">{t("updatePreview.loading")}</p>}
      {error && <p className="install-compact error" role="alert">{error}</p>}
      {preview && <>
        <p className="card-path">{preview.libraryPath}</p>
        <p>{t("updates.current")}: {preview.installedRef || "—"} → {preview.latestRef}</p>
        <h4>{t("updatePreview.impacts", { count: preview.impacts.length })}</h4>
        <p className="empty-hint">{t("updatePreview.impactHint")}</p>
        {preview.impacts.map(item => <div className="update-impact" key={item.path}>
          <strong>{item.assistant} · {t(`projectInspection.scope.${item.scope}`)}</strong>
          {item.projectPath && <span className="card-path">{item.projectPath}</span>}
          <span className="card-path">{item.path}</span>
          {!item.connected && <span className="stamp warn">{t("updatePreview.disconnected")}</span>}
        </div>)}
        <h4>{t("updatePreview.files", { count: preview.files.length + preview.omittedFiles })}</h4>
        {preview.files.map(file => <details className="update-file" key={file.path}>
          <summary><span className="stamp">{t(`updatePreview.${file.kind}`)}</span> {file.path}</summary>
          {file.textOmitted && <p>{t("updatePreview.textOmitted")}</p>}
          <div className="update-file-content">
            <div><strong>{t("updatePreview.before")}</strong><pre>{file.before ?? "—"}</pre></div>
            <div><strong>{t("updatePreview.after")}</strong><pre>{file.after ?? "—"}</pre></div>
          </div>
        </details>)}
        {preview.omittedFiles > 0 && <p>{t("updatePreview.omitted", { count: preview.omittedFiles })}</p>}
        {!!preview.warnings.length && <p className="stamp warn">{t("updatePreview.warnings")}: {preview.warnings.map(code => t(`structure.warning.${code}`)).join(t("common.listSeparator"))}</p>}
        {!preview.canApply && <p role="status">{t(preview.blockedReason ? `updatePreview.blocked.${preview.blockedReason}` : "updatePreview.blocked")}</p>}
      </>}
    </div>
    <div className="modal-actions">
      <button className="btn btn-secondary" disabled={applying || checking} onClick={() => setRevision(value => value + 1)}>{t("common.refresh")}</button>
      <button className="btn btn-primary" disabled={checking || applying || !preview?.canApply || !preview?.planToken} onClick={apply}>{t(applying ? "updates.updating" : "updatePreview.confirm")}</button>
    </div>
  </ModalShell>;
}
