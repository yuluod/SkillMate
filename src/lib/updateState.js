const fields = {
  originKind: "origin_kind", originLocator: "origin_locator",
  resolvedLocator: "resolved_locator", trackingRef: "tracking_ref",
  installedRef: "installed_ref", latestRef: "latest_ref",
  syncState: "sync_state", message: "sync_message", lagCount: "lag_count",
  lastProbeAt: "last_probe_at", lastSyncAt: "last_sync_at",
  managedByApp: "managed_by_app", canCheck: "can_check", canSync: "can_sync",
};

export function getUpdateInfo(skill, state = {}, unknownMessage = "") {
  const info = Object.fromEntries(Object.entries(fields).map(([key, source]) =>
    [key, state[key] ?? skill[source]]));
  return { ...info, message: info.message ?? unknownMessage,
    lagCount: info.lagCount ?? 0, canCheck: info.canCheck ?? false,
    canSync: info.canSync ?? false,
    checking: Boolean(state.checking), updating: Boolean(state.updating) };
}

// A failed IPC response carries placeholder identity/capability values, not
// evidence that ownership or the source has changed.
export function adaptUpdateResult(previous = {}, result, noResultMessage) {
  const syncState = result?.syncState || "failed";
  if (syncState === "failed") {
    return { ...previous, syncState, hasUpdate: false, lagCount: 0,
      message: result?.message || noResultMessage };
  }
  const patch = Object.fromEntries(Object.keys(fields)
    .filter(key => result[key] !== undefined).map(key => [key, result[key]]));
  return { ...previous, ...patch, syncState, hasUpdate: syncState === "behind" };
}

export function updateCheckNotice(result) {
  const state = result.syncState;
  if (state === "current") return { key: "updates.toast.current", tone: "success" };
  if (state === "behind") return { key: "updates.toast.available", tone: "warn" };
  const known = ["failed", "source_missing", "unsupported", "diverged", "ahead_local", "local_fixed"];
  return { key: `updates.state.${known.includes(state) ? state : "unknown"}`,
    tone: state === "failed" ? "error" : "warn" };
}

export function isIncompleteUpdateCheck(result) {
  return !["current", "behind", "ahead_local", "local_fixed"].includes(result.syncState);
}
