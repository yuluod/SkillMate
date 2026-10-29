import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import Icon from "./Icon.jsx";
import { useI18n } from "../lib/i18n.jsx";
import { toUserErrorMessage } from "../lib/errorMessage.mjs";

function shortPath(path = "") {
  const normalized = path.replace(/\/+$/, "");
  const segments = normalized.split("/").filter(Boolean);
  if (segments.length <= 2) return normalized;
  return `…/${segments.slice(-2).join("/")}`;
}

export default function ProjectWorkspaceBar({ workspace, onOpen }) {
  const { t } = useI18n();
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const pickerRef = useRef(null);
  const closePicker = () => { if (pickerRef.current) pickerRef.current.open = false; };
  const select = path => { workspace.select(path); setError(""); onOpen(); closePicker(); };
  async function browse() {
    try {
      const path = await open({ directory: true, multiple: false, title: t("install.projectPickerTitle") });
      if (typeof path === "string") { setDraft(path); select(path); }
    } catch (reason) { setError(toUserErrorMessage(reason, t("error.safeRetry"))); }
  }
  useEffect(() => {
    const onPointerDown = (event) => {
      if (pickerRef.current?.open && !pickerRef.current.contains(event.target)) closePicker();
    };
    const onKeyDown = (event) => { if (event.key === "Escape") closePicker(); };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, []);
  return (
    <details className="ws-picker" ref={pickerRef}>
      <summary className="ws-chip" title={workspace.path || t("workspace.pick")}>
        <Icon name="folder" size={13} />
        <span className="ws-chip-name">{t("workspace.project")}</span>
        {workspace.path
          ? <b>{shortPath(workspace.path)}</b>
          : <span className="ws-chip-empty">{t("workspace.pick")}</span>}
        <Icon name="arrow" size={12} />
      </summary>
      <div className="ws-popover">
        <form className="ws-popover-form" onSubmit={event => { event.preventDefault(); if (draft.trim()) select(draft.trim()); }}>
          <label className="visually-hidden" htmlFor="workspace-path">{t("projectInspection.path")}</label>
          <input id="workspace-path" value={draft} onChange={event => setDraft(event.target.value)} placeholder={workspace.path || t("projectInspection.placeholder")} />
          <button className="btn btn-secondary btn-sm" type="button" onClick={browse}>{t("workspace.browse")}</button>
          <button className="btn btn-primary btn-sm" disabled={!draft.trim()}>{t("workspace.open")}</button>
        </form>
        {error && <p role="alert" className="ws-popover-error">{error}</p>}
        {!!workspace.projects.length && (
          <div className="ws-recent">
            <span className="ws-recent-label">{t("workspace.recent")}</span>
            {[...workspace.projects].sort((a, b) => Number(b.pinned) - Number(a.pinned)).map(project => (
              <div className="ws-project" key={project.path}>
                <button className="btn btn-ghost btn-sm" title={project.path} onClick={() => select(project.path)}>{shortPath(project.path)}</button>
                <button className="btn btn-ghost btn-sm ws-project-pin" aria-pressed={project.pinned} aria-label={t(project.pinned ? "workspace.unpinPath" : "workspace.pinPath", { path: project.path })} onClick={() => workspace.togglePin(project.path)}>{t(project.pinned ? "workspace.unpin" : "workspace.pin")}</button>
                <button className="btn btn-ghost btn-sm" aria-label={t("workspace.forgetPath", { path: project.path })} onClick={() => workspace.forget(project.path)}>{t("workspace.forget")}</button>
              </div>
            ))}
          </div>
        )}
      </div>
    </details>
  );
}
