import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DEFAULT_INSTALL_POLICY, normalizeInstallPolicy } from "./installPolicy.mjs";
import { skillmateApi } from "./skillmateApi.js";
import { useI18n } from "./i18n.jsx";
import { toUserErrorMessage } from "./errorMessage.mjs";

export function useInstallPolicyFlow({ showToast }) {
  const { t } = useI18n();
  const [policy, setPolicy] = useState(DEFAULT_INSTALL_POLICY);
  const [savedPolicy, setSavedPolicy] = useState(DEFAULT_INSTALL_POLICY);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const mountedRef = useRef(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const result = normalizeInstallPolicy(await skillmateApi.policy.get());
      if (!mountedRef.current) return;
      setPolicy(result);
      setSavedPolicy(result);
      setError("");
    } catch (e) {
      if (!mountedRef.current) return;
      const message = toUserErrorMessage(e, t("error.safeRetry"));
      setError(message);
      showToast(t("policy.toast.loadFailed", { message }), "error");
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  }, [showToast, t]);

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
    };
  }, [load]);

  const dirty = useMemo(
    () => JSON.stringify(policy) !== JSON.stringify(savedPolicy),
    [policy, savedPolicy]
  );

  const update = useCallback((field, value) => {
    setPolicy((current) => ({ ...current, [field]: value }));
  }, []);

  const save = useCallback(async () => {
    if (saving) return;
    setSaving(true);
    try {
      const result = normalizeInstallPolicy(await skillmateApi.policy.set(policy));
      setPolicy(result);
      setSavedPolicy(result);
      setError("");
      showToast(t("policy.toast.saved"), "success");
    } catch (e) {
      const message = toUserErrorMessage(e, t("error.safeRetry"));
      setError(message);
      showToast(t("policy.toast.saveFailed", { message }), "error");
    } finally {
      setSaving(false);
    }
  }, [policy, saving, showToast, t]);

  return {
    policy,
    loading,
    saving,
    dirty,
    error,
    update,
    save,
    reload: load,
  };
}
