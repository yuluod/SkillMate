import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import zhCN from "../locales/zh-CN.js";
import en from "../locales/en.js";
import { formatTranslation } from "./i18nCore.mjs";

const STORAGE_KEY = "skillmate-language";
const dictionaries = { "zh-CN": zhCN, en };
const defaultI18n = {
  language: "zh-CN",
  setLanguage: () => {},
  t: (key, values) => formatTranslation(zhCN, zhCN, key, values),
};
const I18nContext = createContext(defaultI18n);

function initialLanguage() {
  if (typeof window === "undefined") return "zh-CN";
  const saved = window.localStorage.getItem(STORAGE_KEY);
  if (saved && dictionaries[saved]) return saved;
  return window.navigator.language?.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

export function I18nProvider({ children }) {
  const [language, setLanguage] = useState(initialLanguage);
  useEffect(() => {
    window.localStorage.setItem(STORAGE_KEY, language);
    document.documentElement.lang = language;
  }, [language]);
  const t = useCallback(
    (key, values) => formatTranslation(dictionaries[language], zhCN, key, values),
    [language],
  );
  const value = useMemo(() => ({ language, setLanguage, t }), [language, t]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  return useContext(I18nContext);
}
