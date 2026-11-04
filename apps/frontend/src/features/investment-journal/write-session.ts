// Ledger-scoped volatile memory only; financial request payloads never enter browser storage.
import { selectedProfileId } from "@/features/profiles/session";
import type { InvestmentJournalCommand, InvestmentJournalError } from "@/adapters";

export interface FrozenJournalWrite {
  command: InvestmentJournalCommand;
  args: Record<string, unknown>;
  done: (data: unknown) => void;
  owner: { current: boolean };
}
export interface WriteSession {
  frozen: FrozenJournalWrite | null;
  busy: boolean;
  uncertain: boolean;
  error: InvestmentJournalError | null;
}
const empty: WriteSession = { frozen: null, busy: false, uncertain: false, error: null };
const sessions = new Map<string, WriteSession>();
const listeners = new Map<string, Set<() => void>>();
export function writeSessionKey() {
  return selectedProfileId() ?? "unbound";
}
export function readWriteSession(key: string) {
  return sessions.get(key) ?? empty;
}
export function updateWriteSession(key: string, state: WriteSession) {
  sessions.set(key, state);
  listeners.get(key)?.forEach((listener) => listener());
}
export function subscribeWriteSession(key: string, listener: () => void) {
  const set = listeners.get(key) ?? new Set<() => void>();
  set.add(listener);
  listeners.set(key, set);
  return () => {
    set.delete(listener);
    if (!set.size) listeners.delete(key);
  };
}
