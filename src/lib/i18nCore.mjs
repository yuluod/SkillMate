export function formatTranslation(dictionary, fallback, key, values = {}) {
  const template = dictionary?.[key] ?? fallback?.[key] ?? key;
  return template.replace(/\{(\w+)\}/g, (_, name) => String(values[name] ?? `{${name}}`));
}
