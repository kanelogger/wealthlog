/**
 * A transport failure leaves the write in an unknown state: the server may
 * have committed it. Callers must retry the frozen payload unchanged so the
 * request key can return the original result.
 */
export function isUnknownResultError(error: unknown): boolean {
  if (typeof TypeError !== "undefined" && error instanceof TypeError) return true;
  const text = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  return /failed to fetch|networkerror|network error|timeout|timed out|econn|load failed/i.test(
    text,
  );
}
