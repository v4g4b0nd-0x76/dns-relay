import { invoke } from "@tauri-apps/api/core";

import type {
  AppState,
  ApplyResult,
  DnsRelayConfig,
  ObservabilitySnapshot,
  ServiceState,
  ValidationResult,
} from "./types";

export interface Backend {
  getAppState(): Promise<AppState>;
  getServiceState(): Promise<ServiceState>;
  getObservability(): Promise<ObservabilitySnapshot>;
  installService(draft: DnsRelayConfig): Promise<ApplyResult>;
  adoptService(): Promise<DnsRelayConfig>;
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
    recoveryRequired: false,
  };
  #installFails = false;

  #metricsFail = false;
  #restartFails = false;

  constructor(mode: "default" | "first-launch" | "existing" | "partial-install" | "partial-existing" | "install-error" | "metrics-error" | "restart-error" | "service-error" = "default") {
    if (mode === "first-launch" || mode === "install-error") this.#state.service = "not_installed";
    if (mode === "existing") {
      this.#state.service = "running";
      this.#state.draft = null;
      this.#state.warnings = ["Existing configuration must be adopted before editing"];
    }
    if (mode === "partial-install") {
      this.#state.recoveryRequired = true;
      this.#state.warnings = ["Installation is incomplete; run Repair to restore fixed assets"];
    }
    if (mode === "partial-existing") {
      this.#state.draft = null;
      this.#state.recoveryRequired = true;
      this.#state.warnings = ["Installation is incomplete; run Repair to restore fixed assets"];
    }
    this.#installFails = mode === "install-error";
    this.#metricsFail = mode === "metrics-error";
    this.#restartFails = mode === "restart-error";
    if (mode === "restart-error") this.#state.service = "running";
    if (mode === "service-error") {
      this.#state.service = "error";
      this.#state.warnings = ["Service status unavailable: launchctl failed"];
    }
  }

  async getAppState(): Promise<AppState> {
    return structuredClone(this.#state);
  }

  async getServiceState(): Promise<ServiceState> {
    return this.#state.service;
  }

  async getObservability(): Promise<ObservabilitySnapshot> {
    return {
      health: { value: true },
      metrics: this.#metricsFail
        ? { error: "Metrics endpoint is unavailable" }
        : {
            value: {
              total_req: 18429,
              resolved_count: 18002,
              failed_count: 12,
              timeout_count: 3,
              redirect_count: 92,
              drop_count: 420,
              cached_count: 13084,
              relay_resolved_count: 128,
            },
          },
    };
  }

  async validateDraft(): Promise<ValidationResult> {
    return { valid: true, errors: [] };
  }

  async installService(draft: DnsRelayConfig): Promise<ApplyResult> {
    await new Promise((resolve) => window.setTimeout(resolve, 50));
    if (this.#installFails) throw new Error("Administrator authorization was cancelled");
    this.#state = { service: "running", draft: structuredClone(draft), warnings: [], recoveryRequired: false };
    return { service: "running", message: "DNS Relay installed" };
  }

  async adoptService(): Promise<DnsRelayConfig> {
    await new Promise((resolve) => window.setTimeout(resolve, 50));
    const draft = structuredClone(fixtureConfig);
    this.#state.draft = draft;
    this.#state.warnings = [];
    this.#state.recoveryRequired = false;
    return structuredClone(draft);
  }

  async applyDraft(draft: DnsRelayConfig): Promise<ApplyResult> {
    await new Promise((resolve) => window.setTimeout(resolve, 50));
    this.#state.draft = structuredClone(draft);
    return { service: this.#state.service, message: "Configuration applied" };
  }

  async serviceAction(
    action: "start" | "stop" | "restart" | "repair" | "uninstall",
  ): Promise<ServiceState> {
    if (action === "restart" && this.#restartFails) throw new Error("Service restart failed");
    if (action === "start" || action === "restart" || action === "repair") this.#state.service = "running";
    if (action === "stop") this.#state.service = "stopped";
    if (action === "uninstall") this.#state.service = "not_installed";
    if (action === "repair" || action === "uninstall") this.#state.recoveryRequired = false;
    return this.#state.service;
  }
}

export class TauriBackend implements Backend {
  getAppState(): Promise<AppState> {
    return invoke("get_app_state");
  }

  getServiceState(): Promise<ServiceState> {
    return invoke("get_service_state");
  }

  getObservability(): Promise<ObservabilitySnapshot> {
    return invoke("get_observability");
  }

  validateDraft(draft: DnsRelayConfig): Promise<ValidationResult> {
    return invoke("validate_draft", { draft });
  }

  installService(draft: DnsRelayConfig): Promise<ApplyResult> {
    return invoke("install_service", { draft });
  }

  adoptService(): Promise<DnsRelayConfig> {
    return invoke("adopt_service");
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
  if ("__TAURI_INTERNALS__" in window) return new TauriBackend();
  const mode = new URLSearchParams(window.location.search).get("fixture");
  if (mode === "first-launch" || mode === "existing" || mode === "partial-install" || mode === "partial-existing" || mode === "install-error" || mode === "metrics-error" || mode === "restart-error" || mode === "service-error") {
    return new FixtureBackend(mode);
  }
  return new FixtureBackend();
}
