import { useState } from "react";
import Icon from "./Icon.jsx";
import { useI18n } from "../lib/i18n.jsx";
import { skillmateApi } from "../lib/skillmateApi.js";

// 低置信提示注入启发式的展示与「已审阅」登记，安装预览与更新预览共用。
export default function SuspiciousFindings({ findings, onReviewed }) {
  const { t } = useI18n();
  const [busy, setBusy] = useState("");
  if (!findings?.length) return null;
  const pending = findings.filter((finding) => !finding.reviewed);
  async function review(finding) {
    if (busy) return;
    setBusy(finding.fingerprint);
    try {
      await skillmateApi.findings.review(finding);
      onReviewed?.(finding);
    } finally {
      setBusy("");
    }
  }
  return (
    <div className="suspicious-findings">
      <div className="skill-detail-title"><Icon name="shield" size={15} /><strong>{t("suspiciousFindings.title", { count: pending.length })}</strong></div>
      <p className="form-help">{t("suspiciousFindings.hint")}</p>
      <ul>
        {findings.map((finding) => (
          <li key={finding.fingerprint} className={finding.reviewed ? "reviewed" : ""}>
            <span className="stamp warn">{t(`suspiciousFindings.kind.${finding.kind}`)}</span>
            <code>{finding.file}:{finding.line}</code>
            <span className="suspicious-excerpt" title={finding.excerpt}>{finding.excerpt}</span>
            {finding.reviewed
              ? <span className="stamp muted">{t("suspiciousFindings.reviewed")}</span>
              : <button className="btn btn-ghost btn-sm" disabled={Boolean(busy)} onClick={() => review(finding)}>{t("suspiciousFindings.review")}</button>}
          </li>
        ))}
      </ul>
    </div>
  );
}
