import { useId } from "react";
import Icon from "./Icon.jsx";
import ModalShell from "./ModalShell.jsx";
import {
  SUPPORTED_INSTALL_SOURCES,
  buildInstallPreviewSummary,
  buildProjectTargetPreviewSummary,
  buildStructureWarningSummary,
  buildValidationSummary,
  getStructureStatusLabel,
  getStructureStatusTone,
} from "../lib/skillmate.mjs";

export function InstallModal({
  flow,
  assistants,
  loading,
  onClose,
}) {
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
    <ModalShell title="安装 Skill" icon="plus" className="install-modal" onClose={onClose}>
      <div className="form">
        <label htmlFor="install-source">Skill 来源</label>
        <input id="install-source" value={pkg} onChange={e => setPkg(e.target.value)} placeholder="Git URL、owner/repo、GitHub tree URL 或本地目录" />
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
          <label htmlFor="install-assistant">安装到</label>
          <select id="install-assistant" value={installAssistant} onChange={e => setInstallAssistant(e.target.value)}>
            {assistants.map((assistant) => (
              <option key={assistant.name} value={assistant.name}>{assistant.name}</option>
            ))}
          </select>
        </div>
        {showProjectLinkOption && (
          <label className="install-switch">
            <input type="checkbox" checked={installMode === "symlink"} onChange={e => setInstallMode(e.target.checked ? "symlink" : "copy")} />
            <span>链接到项目</span>
          </label>
        )}
      </div>
      {showProjectLinkOption && installMode === "symlink" && (
        <div className="install-project">
          <div className="form">
            <label htmlFor="install-project-path">项目路径</label>
            <input id="install-project-path" value={projectPath} onChange={e => setProjectPath(e.target.value)} placeholder="/path/to/project" />
          </div>
          {previewingProjectTargets && <div className="git-meta">正在识别项目目标目录...</div>}
          {projectTargetPreview.length > 0 && (
            <ul className="import-preview-list">
              {buildProjectTargetPreviewSummary(projectTargetPreview).map((line, index) => (
                <li key={`${line}-${index}`}>{line}</li>
              ))}
            </ul>
          )}
        </div>
      )}
      {(showInstallAdvancedOptions || installAdvancedOpen) && (
        <div className="install-advanced">
          <div className="form">
            <label htmlFor="install-source-kind">来源类型</label>
            <select id="install-source-kind" value={src} onChange={e => setSrc(e.target.value)}>
              {SUPPORTED_INSTALL_SOURCES.map((source) => (
                <option key={source} value={source}>{source === "git" ? "Git 仓库" : "本地目录"}</option>
              ))}
            </select>
          </div>
        </div>
      )}
      {installStructurePreview && (
        <div className={`structure-preview install-preview-card ${installPreviewView?.tone || (installStructurePreview.can_install === false ? "error" : getStructureStatusTone(installStructurePreview.structure_status))}`}>
          <div className="structure-preview-head">
            <span>安装计划</span>
            <strong>{installPreviewView?.canApply && installPreviewCurrent ? "可安装" : "需要检查"}</strong>
          </div>
          <ul className="install-summary-list">
            {buildInstallPreviewSummary(installStructurePreview).slice(0, 4).map((line, index) => (
              <li key={`${line}-${index}`}>{line}</li>
            ))}
          </ul>
          {!installPreviewCurrent && <p>输入已变化，请重新检查结构。</p>}
        </div>
      )}
      <button className="btn btn-primary full install-primary" onClick={runInstallPrimaryAction} disabled={installPrimaryAction.disabled || loading}>
        <Icon name={installPrimaryAction.icon} size={16} />{installPrimaryAction.label}
      </button>
      <div className="install-secondary-actions">
        <button className="btn btn-ghost btn-sm" onClick={() => setInstallDetailsOpen(!installDetailsOpen)}>
          <Icon name="preview" size={14} />{installDetailsOpen ? "收起执行信息" : "查看执行信息"}
        </button>
        <button className="btn btn-ghost btn-sm" onClick={() => setInstallAdvancedOpen(!installAdvancedOpen)}>
          <Icon name="settings" size={14} />{installAdvancedOpen ? "收起高级选项" : "高级选项"}
        </button>
      </div>
      {installDetailsOpen && (
        <div className="install-details">
          <div className="form"><span className="form-label">执行方式</span><div className="cmd">{cmd}</div></div>
          {installStructurePreview && (
            <>
              <p>{buildStructureWarningSummary(installStructurePreview)}</p>
              {installPreviewView?.packageWarnings && <p>{installPreviewView.packageWarnings}</p>}
              {installPreviewView?.needsModel && <p>本地规则置信度不足，可后续启用模型辅助识别。</p>}
              {installPreviewView?.policy?.message && (
                <div className={`install-compact ${installPreviewView.policy.allowed ? "success" : "error"}`}>
                  <span>安装策略</span>
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
                    <li key={skill.relative_path}>{skill.relative_path} · {getStructureStatusLabel(skill.structure_status)}</li>
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

export function PreviewModal({ preview, onClose }) {
  return (
    <ModalShell title={preview.title} className="large" onClose={onClose}>
      {preview.validation && (
        <div className="import-preview" style={{ marginBottom: 14 }}>
          <div className="import-preview-head">
            <strong>结构验证</strong>
            <span>{getStructureStatusLabel(preview.validation.structure_status)}</span>
          </div>
          <ul className="import-preview-list">
            {buildValidationSummary(preview.validation).map((check) => (
              <li key={check.code}>{check.code}：{check.label} · {check.message}</li>
            ))}
          </ul>
        </div>
      )}
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
  return (
    <ModalShell title="编辑标签" onClose={onClose}>
      <p style={{ color: "var(--text2)", fontSize: "0.9rem", marginBottom: 16 }}>{tagEditor.skill?.name}</p>
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
        {tags.length === 0 && <p className="empty-hint">请先在设置页创建标签</p>}
      </div>
      <div className="card-actions" style={{ justifyContent: "flex-end", marginTop: 20 }}>
        <button className="btn btn-secondary btn-sm" onClick={onClose}>取消</button>
        <button className="btn btn-primary btn-sm" onClick={saveSkillTags}>保存</button>
      </div>
    </ModalShell>
  );
}

export function ConfirmModal({ confirmState, onClose, onConfirm }) {
  const descriptionId = useId();
  return (
    <ModalShell title={confirmState.title} onClose={onClose} role="alertdialog" descriptionId={descriptionId}>
      <p id={descriptionId} style={{ color: "var(--text2)", fontSize: "0.9rem", marginBottom: 20 }}>{confirmState.message}</p>
      <div className="card-actions" style={{ justifyContent: "flex-end" }}>
        <button className="btn btn-secondary btn-sm" onClick={onClose}>取消</button>
        <button
          className={`btn btn-${confirmState.tone === "primary" ? "primary" : "danger"} btn-sm`}
          onClick={onConfirm}
        >
          {confirmState.confirmLabel || "确认"}
        </button>
      </div>
    </ModalShell>
  );
}
