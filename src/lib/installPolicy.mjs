export const DEFAULT_INSTALL_POLICY = Object.freeze({
  mode: "off",
  block_risky_content: false,
  trusted_git_hosts: [],
  trusted_local_roots: [],
});

export function normalizeInstallPolicy(value) {
  return {
    mode: value?.mode || "off",
    block_risky_content: Boolean(value?.block_risky_content),
    trusted_git_hosts: Array.isArray(value?.trusted_git_hosts) ? value.trusted_git_hosts : [],
    trusted_local_roots: Array.isArray(value?.trusted_local_roots) ? value.trusted_local_roots : [],
  };
}

export function splitPolicyEntries(value) {
  return [...new Set(String(value || "")
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean))];
}
