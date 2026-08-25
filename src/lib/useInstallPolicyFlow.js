import { useCallback, useEffect, useMemo, useState } from "react";
import { DEFAULT_INSTALL_POLICY, normalizeInstallPolicy } from "./installPolicy.mjs";
import { skillmateApi } from "./skillmateApi.js";
import { useI18n } from "./i18n.jsx";

export function useInstallPolicyFlow({ showToast }) {
  const { t } = useI18n();
  const [policy, setPolicy] = useState(DEFAULT_INSTALL_POLICY);
  const [savedPolicy, setSavedPolicy] = useState(DEFAULT_INSTALL_POLICY);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const result = normalizeInstallPolicy(await skillmateApi.policy.get());
      setPolicy(result);
      setSavedPolicy(result);
      setError("");
    } catch (e) {
      setError(String(e));
      showToast(t("policy.toast.loadFailed", { message: String(e) }), "error");
    } finally {
      setLoading(false);
    }
  }, [showToast, t]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    skillmateApi.policy.get()
      .then((result) => {
        if (cancelled) return;
        const normalized = normalizeInstallPolicy(result);
        setPolicy(normalized);
        setSavedPolicy(normalized);
        setError("");
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        showToast(t("policy.toast.loadFailed", { message: String(e) }), "error");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [showToast, t]);

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
      setError(String(e));
      showToast(t("policy.toast.saveFailed", { message: String(e) }), "error");
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
