import { useCallback, useMemo, useState } from "react";
import {
  formatScenarioCopyText,
  normalizeScenarioSkillPaths,
  resolveScenarioSkills,
} from "./skillmate.mjs";
import { skillmateApi } from "./skillmateApi.js";

export function useScenarioFlow({ scenarios, allSkills, selectableSkills, showToast, loadData, setView }) {
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
    setName(`${scenario.name} 副本`);
    setDescription(scenario.description || "");
    setSelectedPaths([...scenario.skill_ids]);
    setManualInput("");
    showToast("已回填到场景编辑器", "success");
  }, [showToast]);

  const apply = useCallback((scenario) => {
    setActiveId(scenario.id);
    setView("skills");
    showToast(`已按场景筛选：${scenario.name}`, "success");
  }, [setView, showToast]);

  const create = useCallback(async () => {
    const skillIds = normalizeScenarioSkillPaths({
      selectedPaths,
      manualInput,
      skills: selectableSkills,
    });
    try {
      await skillmateApi.scenarios.create({
        name: name || `场景 ${new Date().toLocaleDateString()}`,
        description: description || "自动生成场景",
        skillIds,
      });
      showToast("场景已创建", "success");
      clearEditor();
      await loadData();
      setView("scenarios");
    } catch (e) {
      showToast(`创建失败: ${e}`, "error");
    }
  }, [clearEditor, description, loadData, manualInput, name, selectableSkills, selectedPaths, setView, showToast]);

  const remove = useCallback(async (id) => {
    try {
      await skillmateApi.scenarios.delete(id);
      if (activeId === id) setActiveId("");
      showToast("场景已删除", "success");
      await loadData();
    } catch (e) {
      showToast(`删除失败: ${e}`, "error");
    }
  }, [activeId, loadData, showToast]);

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
      showToast("路径已复制", "success");
    } catch (e) {
      showToast(`复制失败: ${e}`, "error");
    }
  }, [showToast]);

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
