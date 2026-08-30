import type { AppState, DnsRelayConfig, ViewId } from "./types";

export interface ShellState {
  app: AppState;
  savedDraft: DnsRelayConfig | null;
  activeView: ViewId;
  dirty: boolean;
  applying: boolean;
  fixtureState: "normal" | "loading" | "empty" | "warning" | "error";
}

export function createStore(initial: ShellState) {
  let state = structuredClone(initial);
  const listeners = new Set<(state: ShellState) => void>();
  return {
    get: () => state,
    update(change: (draft: ShellState) => void) {
      const next = structuredClone(state);
      change(next);
      state = next;
      listeners.forEach((listener) => listener(state));
    },
    subscribe(listener: (state: ShellState) => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

export type Store = ReturnType<typeof createStore>;
