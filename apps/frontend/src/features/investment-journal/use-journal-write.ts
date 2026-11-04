import { useEffect, useRef, useSyncExternalStore } from "react";
import {
  executeInvestmentJournal,
  InvestmentJournalError,
  type InvestmentJournalCommand,
} from "@/adapters";
import {
  readWriteSession,
  subscribeWriteSession,
  updateWriteSession,
  writeSessionKey,
  type FrozenJournalWrite,
} from "./write-session";

/** Unknown writes survive route unmounts in ledger-scoped memory, with the same key/body. */
export function useJournalWrite() {
  const key = writeSessionKey();
  const owner = useRef(true);
  useEffect(() => {
    owner.current = true;
    return () => {
      owner.current = false;
    };
  }, []);
  const state = useSyncExternalStore(
    (listener) => subscribeWriteSession(key, listener),
    () => readWriteSession(key),
  );
  async function execute(frozen: FrozenJournalWrite) {
    if (readWriteSession(key).busy) return;
    updateWriteSession(key, { frozen, busy: true, uncertain: false, error: null });
    let result: unknown;
    try {
      result = await executeInvestmentJournal(frozen.command, frozen.args);
    } catch (cause) {
      const error =
        cause instanceof InvestmentJournalError
          ? cause
          : new InvestmentJournalError("unknown_result", String(cause));
      const uncertain = error.code === "unknown_result" || error.retryable;
      updateWriteSession(key, { frozen: uncertain ? frozen : null, busy: false, uncertain, error });
      return;
    }
    updateWriteSession(key, { frozen: null, busy: false, uncertain: false, error: null });
    if (frozen.owner.current) frozen.done(result);
    else
      window.dispatchEvent(
        new CustomEvent("wealthlog:investment-recovered", {
          detail: { profile: key, command: frozen.command, result },
        }),
      );
  }
  return {
    busy: state.busy,
    error: state.error,
    uncertain: state.uncertain,
    run<T>(
      command: InvestmentJournalCommand,
      args: Record<string, unknown>,
      done: (data: T) => void,
    ) {
      const current = readWriteSession(key);
      if (current.frozen || current.busy) return Promise.resolve();
      return execute({
        command,
        args: structuredClone(args),
        done: (data) => done(data as T),
        owner,
      });
    },
    retry() {
      const frozen = readWriteSession(key).frozen;
      return frozen ? execute(frozen) : Promise.resolve();
    },
    clearError() {
      const current = readWriteSession(key);
      if (!current.frozen) updateWriteSession(key, { ...current, error: null });
    },
  };
}
