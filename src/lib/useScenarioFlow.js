import { useCallback, useMemo, useState } from "react";
import {
  formatScenarioCopyText,
  normalizeScenarioSkillPaths,
  resolveScenarioSkills,
} from "./skillmate.mjs";
import { skillmateApi } from "./skillmateApi.js";
import { useI18n } from "./i18n.jsx";

export function useScenarioFlow({ scenarios, allSkills, selectableSkills, showToast, loadData, setView }) {
  const { t, language } = useI18n();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [selectedPaths, setSelectedPaths] = useState([]);
  const [manualInput, setManualInput] = useState("");
  const [expandedId, setExpandedId] = useState("");
  const [activeId, setActiveId] = useState("");

  const active = useMemo(
    () => scenarios.find((scenario) => scenario.id === activeId) || null,
    [activeId, scenarios]
  );
  const details = useMemo(() => scenarios.reduce((result, scenario) => {
    result[scenario.id] = resolveScenarioSkills({ scenario, allSkills });
    return result;
  }, {}), [allSkills, scenarios]);

  const clearEditor = useCallback(() => {
    setName("");
    setDescription("");
    setSelectedPaths([]);
    setManualInput("");
  }, []);

  const togglePath = useCallback((path) => {
    setSelectedPaths((current) => (
      current.includes(path) ? current.filter((item) => item !== path) : [...current, path]
    ));
  }, []);

  const loadIntoEditor = useCallback((scenario) => {
    setName(t("scenario.copyName", { name: scenario.name }));
    setDescription(scenario.description || "");
    setSelectedPaths([...scenario.skill_ids]);
    setManualInput("");
    showToast(t("scenario.toast.loaded"), "success");
  }, [showToast, t]);

  const apply = useCallback((scenario) => {
    setActiveId(scenario.id);
    setView("skills");
    showToast(t("scenario.toast.applied", { name: scenario.name }), "success");
  }, [setView, showToast, t]);

  const create = useCallback(async () => {
    const skillIds = normalizeScenarioSkillPaths({
      selectedPaths,
      manualInput,
      skills: selectableSkills,
    });
    try {
      await skillmateApi.scenarios.create({
        name: name || t("scenario.defaultName", { date: new Date().toLocaleDateString(language === "en" ? "en-US" : "zh-CN") }),
        description: description || t("scenario.defaultDescription"),
        skillIds,
      });
      showToast(t("scenario.toast.created"), "success");
      clearEditor();
      await loadData();
      setView("scenarios");
    } catch (e) {
      showToast(t("scenario.toast.createFailed", { message: String(e) }), "error");
    }
  }, [clearEditor, description, language, loadData, manualInput, name, selectableSkills, selectedPaths, setView, showToast, t]);

  const remove = useCallback(async (id) => {
    try {
      await skillmateApi.scenarios.delete(id);
      if (activeId === id) setActiveId("");
      showToast(t("scenario.toast.deleted"), "success");
      await loadData();
    } catch (e) {
      showToast(t("scenario.toast.deleteFailed", { message: String(e) }), "error");
    }
  }, [activeId, loadData, showToast, t]);

  const copyPaths = useCallback(async (paths) => {
    const text = formatScenarioCopyText(paths);
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
      } else {
        const textarea = document.createElement("textarea");
        textarea.value = text;
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand("copy");
        textarea.remove();
      }
      showToast(t("scenario.toast.copied"), "success");
    } catch (e) {
      showToast(t("scenario.toast.copyFailed", { message: String(e) }), "error");
    }
  }, [showToast, t]);

  return {
    active,
    activeId,
    setActiveId,
    details,
    editor: {
      name,
      setName,
      description,
      setDescription,
      selectedPaths,
      manualInput,
      setManualInput,
      togglePath,
      clear: clearEditor,
      create,
    },
    expandedId,
    setExpandedId,
    apply,
    loadIntoEditor,
    copyPaths,
    remove,
  };
}
