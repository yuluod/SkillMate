import { useCallback, useEffect, useRef, useState } from "react";

export function useSearchFlow(delay = 200) {
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const timerRef = useRef(null);

  const cancelPending = useCallback(() => {
    clearTimeout(timerRef.current);
    timerRef.current = null;
  }, []);

  const update = useCallback((value) => {
    setInput(value);
    cancelPending();
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      setQuery(value);
    }, delay);
  }, [cancelPending, delay]);

  const clear = useCallback(() => {
    cancelPending();
    setInput("");
    setQuery("");
  }, [cancelPending]);

  useEffect(() => cancelPending, [cancelPending]);

  return { input, query, update, clear };
}
