// 定时检查更新到期的纯判定：off 永不到期；daily/weekly 按毫秒窗口比较。
// 抽成纯函数便于 node --test 直接覆盖，不依赖 localStorage 或 React。
export const SKILL_CHECK_INTERVALS = {
  daily: 24 * 60 * 60 * 1000,
  weekly: 7 * 24 * 60 * 60 * 1000,
};

export function isCheckDue(lastCheckAt, frequency, now = Date.now()) {
  const interval = SKILL_CHECK_INTERVALS[frequency];
  if (!interval) return false;
  const last = Number(lastCheckAt) || 0;
  if (!last) return true;
  return now - last >= interval;
}
