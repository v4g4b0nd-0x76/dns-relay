export type ServiceState =
  | "not_installed"
  | "stopped"
  | "running"
  | "applying"
  | "error";

export type ViewId =
  | "setup"
  | "dashboard"
  | "resolvers"
  | "rules"
  | "relay"
  | "activity"
  | "settings";

export interface CommandError {
  code: string;
  message: string;
  field?: string;
}

export interface ResolverRow {
  id: string;
  address: string;
  transport: "udp" | "doh" | "doq";
  latencyMs?: number;
  healthy?: boolean;
}

export interface RuleRow {
  id: string;
  kind: "drop" | "redirect";
  domain: string;
  target?: string;
  enabled: boolean;
}

export interface RelayConfig {
  relay_url: string;
  transport: "direct" | "google_chained";
  relay_key: string;
}

export interface RelayRow extends RelayConfig {
  id: string;
}

export interface Metrics {
  total_req: number;
  resolved_count: number;
  failed_count: number;
  timeout_count: number;
  redirect_count: number;
  drop_count: number;
  cached_count: number;
  relay_resolved_count: number;
}

export interface DataState<T> {
  value?: T;
  error?: string;
}

export interface ObservabilitySnapshot {
  health: DataState<boolean>;
  metrics: DataState<Metrics>;
}

export interface DnsRelayConfig {
  dns_target: string;
  drop_list: string[];
  redirect_list: string[];
  resolvers: string[];
  secure_only: boolean;
  client_subnet?: string;
  resolver_searching: {
    enable: boolean;
    resolver_source: string[];
    resfresh_interval?: number;
    ipv4: boolean;
    doh: boolean;
  };
  hotreload_conf: { enable: boolean; poll_interval_ms: number };
  relay_conf: {
    enable: boolean;
    resolve_manual: boolean;
    relay_timeout_sec: number;
    relay_instances: RelayConfig[];
  };
  metric_conf: {
    enable: boolean;
    report_type: "log" | "http";
    report_interval: number;
  };
  vpn_reassertion: boolean;
  init_tls: boolean;
  record_history: boolean;
  record_history_conf?: { matched_list: string[]; lines: number };
  obfs_conf: { enable: boolean; bind_addr: string; keys: string[] };
}

export interface AppState {
  service: ServiceState;
  draft: DnsRelayConfig | null;
  warnings: string[];
  recoveryRequired: boolean;
}

export interface ValidationResult {
  valid: boolean;
  errors: CommandError[];
}

export interface ApplyResult {
  service: ServiceState;
  message: string;
}
