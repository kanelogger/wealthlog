import { useEffect, useRef } from "react";
import { readWriteSession, writeSessionKey } from "./write-session";

const guards = new Set<() => boolean>();
export function allowJournalNavigation() {
  return [...guards].every((guard) => guard());
}
export function useJournalNavigationGuard(
  dirty: boolean,
  messages: { pending: string; discard: string },
) {
  const current = useRef({ dirty, messages });
  current.current = { dirty, messages };
  useEffect(() => {
    const guard = () => {
      const state = readWriteSession(writeSessionKey());
      if (state.frozen || state.busy) {
        window.alert(current.current.messages.pending);
        return false;
      }
      return !current.current.dirty || window.confirm(current.current.messages.discard);
    };
    const click = (event: MouseEvent) => {
      const anchor = event.target instanceof Element ? event.target.closest("a[href]") : null;
      if (anchor && !guard()) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    };
    const unload = (event: BeforeUnloadEvent) => {
      const state = readWriteSession(writeSessionKey());
      if (current.current.dirty || state.frozen || state.busy) {
        event.preventDefault();
        event.returnValue = "";
      }
    };
    guards.add(guard);
    document.addEventListener("click", click, true);
    window.addEventListener("beforeunload", unload);
    return () => {
      guards.delete(guard);
      document.removeEventListener("click", click, true);
      window.removeEventListener("beforeunload", unload);
    };
  }, []);
}
