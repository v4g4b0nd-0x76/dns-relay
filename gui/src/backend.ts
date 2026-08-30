import { invoke } from "@tauri-apps/api/core";

import type {
  AppState,
  ApplyResult,
  DnsRelayConfig,
  ObservabilitySnapshot,
  ProbeResult,
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
  testResolver(resolver: string): Promise<ProbeResult>;
  testRelay(relayUrl: string): Promise<ProbeResult>;
  readLogs(limit: number): Promise<string[]>;
  readHistory(limit: number): Promise<string[]>;
  parseConfig(configToml: string): Promise<DnsRelayConfig>;
  parseBlocklist(content: string): Promise<string[]>;
  exportConfig(draft: DnsRelayConfig, plaintext: boolean): Promise<string>;
  generateSecret(kind: "relay" | "obfs"): Promise<string>;
  revealSecret(reference: string): Promise<string>;
  deleteSecret(reference: string): Promise<void>;
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
  #validationDelay = false;
  #secrets = new Map<string, string>();
  #secretCounter = 0;

  constructor(mode: "default" | "first-launch" | "existing" | "partial-install" | "partial-existing" | "install-error" | "metrics-error" | "restart-error" | "service-error" | "validation-delay" = "default") {
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
    this.#validationDelay = mode === "validation-delay";
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

  async validateDraft(draft: DnsRelayConfig): Promise<ValidationResult> {
    if (this.#validationDelay) await new Promise((resolve) => window.setTimeout(resolve, 300));
    if (draft.secure_only && !draft.resolvers.some((resolver) => resolver.startsWith("https://") || resolver.startsWith("quic://")) && !draft.relay_conf.enable) {
      return { valid: false, errors: [{ code: "invalid_config", message: "secure_only requires an authenticated resolver or relay" }] };
    }
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

  async testResolver(resolver: string): Promise<ProbeResult> {
    if (!resolver || resolver.startsWith("http://")) throw new Error("invalid resolver");
    return { reachable: true, message: "Resolver is reachable", latencyMs: 31 };
  }

  async testRelay(relayUrl: string): Promise<ProbeResult> {
    if (!relayUrl.startsWith("https://")) throw new Error("Relay URL must use HTTPS");
    return { reachable: true, message: "Relay responded with 200 OK", latencyMs: 82 };
  }

  async readLogs(limit: number): Promise<string[]> {
    return [
      "19:40:01 INFO resolver ready on 127.0.0.1:53",
      "19:41:18 WARN blocked tracker.example",
    ].slice(-limit);
  }

  async readHistory(limit: number): Promise<string[]> {
    return ["example.com 93.184.216.34", "tracker.example 0.0.0.0"].slice(-limit);
  }

  async parseConfig(configToml: string): Promise<DnsRelayConfig> {
    if (configToml.includes("invalid")) throw new Error("invalid config");
    const draft = structuredClone(this.#state.draft ?? fixtureConfig);
    if (/secure_only\s*=\s*false/.test(configToml)) draft.secure_only = false;
    if (configToml.includes("fixture_duplicate_generated_secret") && this.#secretCounter) {
      const relay_key = `vault://relay.fixture.${this.#secretCounter}`;
      draft.relay_conf.relay_instances = [
        { relay_url: "https://one.example/dns-query", transport: "direct", relay_key },
        { relay_url: "https://two.example/dns-query", transport: "direct", relay_key },
      ];
    }
    return draft;
  }

  async parseBlocklist(content: string): Promise<string[]> {
    return content.split(/\r?\n/).map((line) => line.trim().split(/\s+/).pop() ?? "").filter((line) => line.includes(".") && !line.startsWith("#"));
  }

  async exportConfig(draft: DnsRelayConfig, plaintext: boolean): Promise<string> {
    const secrets = draft.relay_conf.relay_instances.map((relay) =>
      plaintext ? "demo-plaintext-secret" : relay.relay_key,
    );
    return `dns_target = "${draft.dns_target}"\nsecure_only = ${draft.secure_only}\nrelay_keys = ${JSON.stringify(secrets)}`;
  }

  async generateSecret(kind: "relay" | "obfs"): Promise<string> {
    const reference = `vault://${kind}.fixture.${++this.#secretCounter}`;
    this.#secrets.set(reference, `fixture-secret-${this.#secretCounter}`);
    return reference;
  }

  async revealSecret(reference: string): Promise<string> {
    const value = this.#secrets.get(reference);
    if (!value) throw new Error("secret is missing");
    return value;
  }

  async deleteSecret(reference: string): Promise<void> {
    this.#secrets.delete(reference);
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

  testResolver(resolver: string): Promise<ProbeResult> {
    return invoke("test_resolver", { resolver });
  }

  testRelay(relayUrl: string): Promise<ProbeResult> {
    return invoke("test_relay", { relayUrl });
  }

  readLogs(limit: number): Promise<string[]> {
    return invoke("read_logs", { limit });
  }

  readHistory(limit: number): Promise<string[]> {
    return invoke("read_history", { limit });
  }

  parseConfig(configToml: string): Promise<DnsRelayConfig> {
    return invoke("parse_config", { configToml });
  }

  parseBlocklist(content: string): Promise<string[]> {
    return invoke("parse_blocklist", { content });
  }

  exportConfig(draft: DnsRelayConfig, plaintext: boolean): Promise<string> {
    return invoke("export_config", { draft, plaintext });
  }

  generateSecret(kind: "relay" | "obfs"): Promise<string> {
    return invoke("generate_secret", { kind });
  }

  revealSecret(reference: string): Promise<string> {
    return invoke("reveal_secret", { reference });
  }

  deleteSecret(reference: string): Promise<void> {
    return invoke("delete_secret", { reference });
  }
}

export function createBackend(): Backend {
  if ("__TAURI_INTERNALS__" in window) return new TauriBackend();
  const mode = new URLSearchParams(window.location.search).get("fixture");
  if (mode === "first-launch" || mode === "existing" || mode === "partial-install" || mode === "partial-existing" || mode === "install-error" || mode === "metrics-error" || mode === "restart-error" || mode === "service-error" || mode === "validation-delay") {
    return new FixtureBackend(mode);
  }
  return new FixtureBackend();
}
