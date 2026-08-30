import { invoke } from "@tauri-apps/api/core";

import type {
  AppState,
  ApplyResult,
  DnsRelayConfig,
  ServiceState,
  ValidationResult,
} from "./types";

export interface Backend {
  getAppState(): Promise<AppState>;
  validateDraft(draft: DnsRelayConfig): Promise<ValidationResult>;
  applyDraft(draft: DnsRelayConfig): Promise<ApplyResult>;
  serviceAction(
    action: "start" | "stop" | "restart" | "repair" | "uninstall",
  ): Promise<ServiceState>;
}

const fixtureConfig: DnsRelayConfig = {
  dns_target: "127.0.0.1:53",
  drop_list: [],
  redirect_list: [],
  resolvers: ["https://1.1.1.1/dns-query"],
  secure_only: true,
  resolver_searching: {
    enable: false,
    resolver_source: [],
    ipv4: true,
    doh: true,
  },
  hotreload_conf: { enable: true, poll_interval_ms: 1000 },
  relay_conf: {
    enable: false,
    resolve_manual: false,
    relay_timeout_sec: 5,
    relay_instances: [],
  },
  metric_conf: { enable: true, report_type: "http", report_interval: 30 },
  vpn_reassertion: false,
  init_tls: false,
  record_history: false,
  obfs_conf: { enable: false, bind_addr: "0.0.0.0:8853", keys: [] },
};

export class FixtureBackend implements Backend {
  #state: AppState = {
    service: "stopped",
    draft: structuredClone(fixtureConfig),
    warnings: [],
  };

  async getAppState(): Promise<AppState> {
    return structuredClone(this.#state);
  }

  async validateDraft(): Promise<ValidationResult> {
    return { valid: true, errors: [] };
  }

  async applyDraft(draft: DnsRelayConfig): Promise<ApplyResult> {
    await new Promise((resolve) => window.setTimeout(resolve, 50));
    this.#state.draft = structuredClone(draft);
    return { service: this.#state.service, message: "Configuration applied" };
  }

  async serviceAction(
    action: "start" | "stop" | "restart" | "repair" | "uninstall",
  ): Promise<ServiceState> {
    if (action === "start" || action === "restart") this.#state.service = "running";
    if (action === "stop") this.#state.service = "stopped";
    if (action === "uninstall") this.#state.service = "not_installed";
    return this.#state.service;
  }
}

export class TauriBackend implements Backend {
  getAppState(): Promise<AppState> {
    return invoke("get_app_state");
  }

  validateDraft(draft: DnsRelayConfig): Promise<ValidationResult> {
    return invoke("validate_draft", { draft });
  }

  applyDraft(draft: DnsRelayConfig): Promise<ApplyResult> {
    return invoke("apply_draft", { draft });
  }

  serviceAction(
    action: "start" | "stop" | "restart" | "repair" | "uninstall",
  ): Promise<ServiceState> {
    return invoke("service_action", { action });
  }
}

export function createBackend(): Backend {
  return "__TAURI_INTERNALS__" in window ? new TauriBackend() : new FixtureBackend();
}
