const TECHNICAL_ERROR = /(?:TypeError|ReferenceError|SyntaxError|__TAURI|Cannot read properties|reading ['"]invoke['"]|\binvoke\b)/i;

export function toUserErrorMessage(error, fallback, separator = "; ") {
  const text = String(error ?? "").trim();
  if (!text) return fallback;

  if (TECHNICAL_ERROR.test(text)) {
    const marker = text.search(TECHNICAL_ERROR);
    const prefix = text.slice(0, marker).replace(/[\s:：;；,-]+$/, "");
    return prefix ? `${prefix}${separator}${fallback}` : fallback;
  }

  return text.replace(/^(?:Error|Exception):\s*/i, "").trim() || fallback;
}
