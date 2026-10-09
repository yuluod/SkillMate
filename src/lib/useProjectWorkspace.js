import { useCallback, useState } from "react";

const STORAGE_KEY = "skillmate-projects-v1";
function readProjects() {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || "[]");
    return Array.isArray(saved) ? saved.filter(item => typeof item?.path === "string" && item.path.trim()).slice(0, 20) : [];
  } catch { return []; }
}

export function useProjectWorkspace() {
  const [projects, setProjects] = useState(readProjects);
  const [path, setPath] = useState("");
  const [request, setRequest] = useState(0);
  const save = useCallback(transform => setProjects(previous => {
    const next = transform(previous);
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify(next)); } catch { /* Session remains usable. */ }
    return next;
  }), []);
  const select = useCallback(value => {
    setPath(value.trim());
    setRequest(value => value + 1);
  }, []);
  const remember = useCallback(value => {
    setPath(value);
    save(previous => {
      const known = previous.find(item => item.path === value);
      const next = [{ path: value, pinned: Boolean(known?.pinned) }, ...previous.filter(item => item.path !== value)];
      return [...next.filter(item => item.pinned), ...next.filter(item => !item.pinned).slice(0, 10)].slice(0, 20);
    });
  }, [save]);
  const togglePin = useCallback(value => save(previous => previous.map(item => item.path === value ? { ...item, pinned: !item.pinned } : item)), [save]);
  const forget = useCallback(value => save(previous => previous.filter(item => item.path !== value)), [save]);
  return { path, request, select, remember, projects, togglePin, forget };
}
