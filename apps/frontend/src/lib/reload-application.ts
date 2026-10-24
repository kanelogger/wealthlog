let executed = false;

/** Clear every provider and query, once per document admission. */
export function reloadApplication(options: { dashboard?: boolean } = {}) {
  if (executed) return;
  executed = true;
  if (options.dashboard) window.location.replace("/");
  else window.location.reload();
}
