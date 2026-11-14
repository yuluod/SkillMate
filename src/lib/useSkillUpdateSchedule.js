import { useCallback, useEffect, useState } from "react";
import { isCheckDue } from "./updateSchedule.mjs";

export const SKILL_CHECK_STORAGE_KEY = "skillmate-skill-check-frequency";
export const SKILL_CHECK_LAST_KEY = "skillmate-skill-check-last";
const STARTUP_DELAY_MS = 3000;
const RECHECK_INTERVAL_MS = 60 * 60 * 1000;

export function readSkillCheckFrequency() {
  try {
    return window.localStorage.getItem(SKILL_CHECK_STORAGE_KEY) || "off";
  } catch {
    return "off";
  }
}

function readLastCheckAt() {
  try {
    return window.localStorage.getItem(SKILL_CHECK_LAST_KEY);
  } catch {
    return null;
  }
}

// 可选的定时 Skill 更新检查：只发现、不写入。到期后静默调用与手动检查
// 同一引擎的静默变体（不弹提示），结果只更新「可更新」计数。
// 检查会访问上游 Git 远端；失败静默保留上次结果。检查时间戳只在检查
// 真正启动后记录——批量引擎因已有任务直接跳过时，不消耗整个检查周期。
// checkPaths 使用完整可检查库存，不受列表页当前筛选影响。
export function useSkillUpdateSchedule({ checkAllSilent, checkPaths = [], checkableCount = 0 }) {
  const [frequency, setFrequencyState] = useState(readSkillCheckFrequency);

  const setFrequency = useCallback((next) => {
    setFrequencyState(next);
    try {
      window.localStorage.setItem(SKILL_CHECK_STORAGE_KEY, next);
    } catch {
      // 持久化失败时仅本次会话生效
    }
  }, []);

  useEffect(() => {
    if (frequency === "off" || !checkableCount) return undefined;
    let cancelled = false;
    const maybeCheck = () => {
      if (cancelled) return;
      if (!isCheckDue(readLastCheckAt(), frequency, Date.now())) return;
      Promise.resolve(checkAllSilent(checkPaths))
        .then((started) => {
          if (cancelled || !started) return;
          try {
            window.localStorage.setItem(SKILL_CHECK_LAST_KEY, String(Date.now()));
          } catch {
            // 时间戳写失败只会导致下次提前重查，不阻塞本次
          }
        })
        .catch(() => {
          // 引擎内部已消化检查失败；此处兜底，不让调度循环中断
        });
    };
    const startupTimer = setTimeout(maybeCheck, STARTUP_DELAY_MS);
    const interval = setInterval(maybeCheck, RECHECK_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearTimeout(startupTimer);
      clearInterval(interval);
    };
  }, [frequency, checkAllSilent, checkPaths, checkableCount]);

  return { frequency, setFrequency };
}
