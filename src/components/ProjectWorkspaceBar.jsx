import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../lib/i18n.jsx";

export default function ProjectWorkspaceBar({ workspace, onOpen }) {
  const { t } = useI18n();
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const select = path => { workspace.select(path); setError(""); onOpen(); };
  async function browse() {
    try {
      const path = await open({ directory: true, multiple: false, title: t("install.projectPickerTitle") });
      if (typeof path === "string") { setDraft(path); select(path); }
    } catch (reason) { setError(String(reason)); }
  }
  return <section className="workspace-bar view-shell" aria-label={t("workspace.title")}>
    <div className="workspace-toolbar">
      <strong>{t("workspace.title")}</strong>
      <form onSubmit={event => { event.preventDefault(); if (draft.trim()) select(draft); }}>
        <label className="visually-hidden" htmlFor="workspace-path">{t("projectInspection.path")}</label>
        <input id="workspace-path" value={draft} onChange={event => setDraft(event.target.value)} placeholder={workspace.path || t("projectInspection.placeholder")} />
        <button className="btn btn-secondary btn-sm" type="button" onClick={browse}>{t("workspace.browse")}</button>
        <button className="btn btn-primary btn-sm" disabled={!draft.trim()}>{t("workspace.open")}</button>
      </form>
    </div>
    {workspace.path && <p className="card-path">{t("workspace.current", { path: workspace.path })}</p>}
    {error && <p role="alert">{error}</p>}
    {!!workspace.projects.length && <details>
      <summary>{t("workspace.recent")}</summary>
      {[...workspace.projects].sort((a, b) => Number(b.pinned) - Number(a.pinned)).map(project => <div className="workspace-project" key={project.path}>
        <button className="btn btn-ghost" title={project.path} onClick={() => select(project.path)}>{project.path}</button>
        <button className="btn btn-secondary btn-sm" aria-pressed={project.pinned} aria-label={t(project.pinned ? "workspace.unpinPath" : "workspace.pinPath", { path: project.path })} onClick={() => workspace.togglePin(project.path)}>{t(project.pinned ? "workspace.unpin" : "workspace.pin")}</button>
        <button className="btn btn-ghost btn-sm" aria-label={t("workspace.forgetPath", { path: project.path })} onClick={() => workspace.forget(project.path)}>{t("workspace.forget")}</button>
      </div>)}
    </details>}
  </section>;
}
