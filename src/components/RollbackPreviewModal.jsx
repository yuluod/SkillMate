import { useEffect, useRef, useState } from "react";
import ModalShell from "./ModalShell.jsx";
import { useI18n } from "../lib/i18n.jsx";
import { toUserErrorMessage } from "../lib/errorMessage.mjs";
import { skillmateApi } from "../lib/skillmateApi.js";

// 回滚预览复用更新预览的结构与差异展示：当前版本 → 快照目标版本，
// 影响启用位置与文件前后内容一致；确认后经计划令牌执行。
export default function RollbackPreviewModal({ path, onClose, onConfirm }) {
  const { t } = useI18n();
  const [preview, setPreview] = useState(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [applying, setApplying] = useState(false);
  const submitting = useRef(false);
  useEffect(() => {
    let cancelled = false;
    setLoading(true); setPreview(null); setError("");
    skillmateApi.updates.previewRollback(path).then(value => {
      if (!cancelled) setPreview(value);
    }).catch(reason => { if (!cancelled) setError(toUserErrorMessage(reason, t("error.safeRetry"))); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [path]);
  async function apply() {
    if (submitting.current || !preview?.canApply || !preview.planToken) return;
    submitting.current = true; setApplying(true);
    try {
      const success = await onConfirm(path, preview.planToken);
      if (success === undefined) return;
      if (success) onClose();
      else { setPreview(null); setError(t("updatePreview.retry")); }
    } catch (reason) { setPreview(null); setError(toUserErrorMessage(reason, t("error.safeRetry"))); }
    finally { submitting.current = false; setApplying(false); }
  }
  return <ModalShell title={t("rollbackPreview.title")} icon="undo" className="update-preview-modal" onClose={() => { if (!submitting.current) onClose(); }}>
    <div className="modal-body">
      {loading && <p role="status">{t("rollbackPreview.loading")}</p>}
      {error && <p className="install-compact error" role="alert">{error}</p>}
      {preview && <>
        <p className="card-path">{preview.libraryPath}</p>
        <p>{t("updates.current")}: {preview.installedRef || "—"} → {t("rollbackPreview.target")}: {preview.latestRef}</p>
        <h4>{t("updatePreview.impacts", { count: preview.impacts.length })}</h4>
        <p className="empty-hint">{t("rollbackPreview.impactHint")}</p>
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
        {!preview.canApply && <p role="status">{t(preview.blockedReason ? `updatePreview.blocked.${preview.blockedReason}` : "updatePreview.blocked")}</p>}
      </>}
    </div>
    <div className="modal-actions">
      <button className="btn btn-secondary" onClick={onClose} disabled={applying}>{t("common.cancel")}</button>
      <button className="btn btn-primary" disabled={loading || applying || !preview?.canApply || !preview?.planToken} onClick={apply}>{applying ? t("rollbackPreview.rollingBack") : t("rollbackPreview.confirm")}</button>
    </div>
  </ModalShell>;
}
