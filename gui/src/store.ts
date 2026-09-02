import type { AppState, DataState, DnsRelayConfig, ObservabilitySnapshot, ProbeResult, ViewId } from "./types";

export interface ShellState {
  app: AppState;
  observability: ObservabilitySnapshot;
  savedDraft: DnsRelayConfig | null;
  activeView: ViewId;
  dirty: boolean;
  applying: boolean;
  secretBusy: boolean;
  serviceRevision: number;
  logs: DataState<string[]>;
  history: DataState<string[]>;
  resolverProbes: Record<string, DataState<ProbeResult>>;
  relayProbes: Record<string, DataState<ProbeResult>>;
  revealedSecrets: Record<string, string>;
  generatedSecrets: string[];
  pendingSecretDeletes: string[];
  activityFilter: string;
  activityPaused: boolean;
  rawToml: string;
  rawError?: string;
  advancedOpen: boolean;
}

export function createStore(initial: ShellState) {
  let state = structuredClone(initial);
  const listeners = new Set<(state: ShellState) => void>();
  return {
    get: () => state,
    update(change: (draft: ShellState) => void) {
      const next = structuredClone(state);
      change(next);
      if (JSON.stringify(next) === JSON.stringify(state)) return;
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
