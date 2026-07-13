export function readPlanToken(preview) {
  return typeof preview?.plan_token === "string" ? preview.plan_token.trim() : "";
}

export function buildPlanInvocation(command, args, planToken) {
  const token = typeof planToken === "string" ? planToken.trim() : "";
  if (!token) {
    throw new Error("操作计划缺失，请重新预览");
  }
  return {
    command,
    args: { ...args, planToken: token },
  };
}

export function createSingleFlightPlanExecutor(invokeFn) {
  const active = new Map();
  return {
    run(key, command, args, planToken) {
      if (active.has(key)) {
        return { started: false, promise: active.get(key), error: null };
      }
      let invocation;
      try {
        invocation = buildPlanInvocation(command, args, planToken);
      } catch (error) {
        return { started: false, promise: null, error };
      }
      const promise = Promise.resolve()
        .then(() => invokeFn(invocation.command, invocation.args))
        .finally(() => active.delete(key));
      active.set(key, promise);
      return { started: true, promise, error: null };
    },
  };
}
