export function createResettableTimer({ schedule = setTimeout, cancel = clearTimeout } = {}) {
  let timer = null;

  function clear() {
    if (timer !== null) {
      cancel(timer);
      timer = null;
    }
  }

  return {
    start(delay, callback) {
      clear();
      timer = schedule(() => {
        timer = null;
        callback();
      }, delay);
    },
    clear,
    dispose: clear,
  };
}
