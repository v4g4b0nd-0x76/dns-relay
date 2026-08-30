import {
  Activity,
  ArrowUpRight,
  ArrowDown,
  ArrowUp,
  CircleDotDashed,
  Copy,
  Database,
  Download,
  Eye,
  EyeOff,
  FlaskConical,
  Gauge,
  Network,
  Plus,
  Power,
  Route,
  Settings,
  ShieldBan,
  Timer,
  Trash2,
  TriangleAlert,
  X,
  createIcons,
} from "lucide";

import appMark from "../src-tauri/icons/icon.png";
import configFields from "./config-fields.json";
import type { ServiceState, ViewId } from "./types";
import type { ShellState } from "./store";

const views: Array<[ViewId, string, string]> = [
  ["dashboard", "gauge", "Home"],
  ["resolvers", "network", "Resolvers"],
  ["rules", "shield-ban", "Rules"],
  ["relay", "route", "Relay"],
  ["activity", "activity", "Activity"],
  ["settings", "settings", "Settings"],
];

const icons = {
  Activity,
  ArrowUpRight,
  ArrowDown,
  ArrowUp,
  CircleDotDashed,
  Copy,
  Database,
  Download,
  Eye,
  EyeOff,
  FlaskConical,
  Gauge,
  Network,
  Plus,
  Power,
  Route,
  Settings,
  ShieldBan,
  Timer,
  Trash2,
  TriangleAlert,
  X,
};

export function render(root: HTMLElement, state: ShellState) {
  const active = document.activeElement as HTMLElement | null;
  const focus = active && root.contains(active) ? active.dataset.focus : undefined;
  const setup = state.app.recoveryRequired || state.app.draft === null || state.app.service === "not_installed";
  const busy = state.applying || state.secretBusy;
  root.innerHTML = `<div class="app-shell" data-app-shell>
    <header class="app-header">
      <div class="brand"><img src="${appMark}" alt="" width="38" height="38"><div><strong>DNS Relay</strong><span>Local control plane</span></div></div>
      <span class="status-pill" data-header-state data-state="${setup ? "applying" : state.app.service}">${setup ? "Setup" : title(state.app.service)}</span>
    </header>
    <div class="app-layout">
      <nav class="primary-nav" aria-label="Primary">${views.map(([id, icon, label]) => navButton(id, icon, label, state, setup)).join("")}</nav>
      <main class="view-host" data-view-host data-config-field-count="${configFields.length}" ${busy ? "inert" : ""}>${setup ? renderSetup(state) : renderView(state)}</main>
    </div>
    <section class="dirty-bar" data-dirty-bar ${state.dirty && !setup ? "" : "hidden"} aria-label="Unsaved configuration">
      <span><i data-lucide="circle-dot-dashed"></i> Pending changes</span><div><button class="button ghost" data-action="revert" data-focus="revert" ${busy ? "disabled" : ""}>Revert changes</button><button class="button primary" data-action="apply" data-focus="apply" ${busy ? "disabled" : ""}>${state.applying ? "Applying…" : "Apply changes"}</button></div>
    </section>
    <dialog class="dialog" data-rule-dialog aria-labelledby="rule-dialog-title"><form><div class="section-heading"><div><p class="eyebrow">Rules</p><h2 id="rule-dialog-title">Add rule</h2></div><button type="button" class="icon-button" data-icon-only data-action="close-dialog" aria-label="Close rule editor" title="Close rule editor"><i data-lucide="x"></i></button></div><label>Domain pattern<input name="domain" autocomplete="off" placeholder="*.ads.example"></label><label>Rule target<input name="target" autocomplete="off" placeholder="drop or IP address"></label><p class="field-error" data-rule-error role="alert"></p><div class="dialog-actions"><button type="button" class="button ghost" data-action="close-dialog">Cancel</button><button type="button" class="button primary" data-action="save-rule">Save rule</button></div></form></dialog>
    <div class="toast" data-toast role="status" hidden></div><div class="sr-only" data-live-region aria-live="polite"></div>
  </div>`;
  createIcons({ icons, attrs: { "stroke-width": 1.8 } });
  if (focus) {
    Array.from(root.querySelectorAll<HTMLElement>("[data-focus]"))
      .find((element) => element.dataset.focus === focus)
      ?.focus();
  }
}

function navButton(id: ViewId, icon: string, label: string, state: ShellState, setup: boolean) {
  const current = !setup && state.activeView === id ? ' aria-current="page"' : "";
  return `<button data-view-nav data-no-overlap="${id}" data-action="navigate" data-target="${id}" data-focus="nav-${id}"${current}${setup ? " disabled" : ""}><i data-lucide="${icon}"></i><span>${label}</span></button>`;
}

function renderView(state: ShellState) {
  const renderers: Record<ViewId, (state: ShellState) => string> = {
    setup: renderSetup,
    dashboard: renderDashboard,
    resolvers: renderResolvers,
    rules: renderRules,
    relay: renderRelay,
    activity: renderActivity,
    settings: renderSettings,
  };
  return renderers[state.activeView](state);
}

function heading(kicker: string, name: string, detail: string) {
  return `<div class="page-heading"><div><p class="eyebrow">${kicker}</p><h1>${name}</h1><span class="muted">${detail}</span></div></div>`;
}

function renderDashboard(state: ShellState) {
  const service = state.applying ? "applying" : state.app.service;
  const running = service === "running";
  const stopped = service === "stopped" || service === "not_installed";
  const action = running ? "Stop" : "Start";
  const draft = state.app.draft;
  const metrics = state.observability.metrics.value;
  const health = !stopped && state.observability.health.value === true;
  const healthLabel = stopped ? "Service stopped" : (health ? "Healthy" : "Health unavailable");
  const cacheHit = metrics?.total_req
    ? `${Math.round((metrics.cached_count / metrics.total_req) * 100)}%`
    : "—";
  return `<section class="view" data-view="dashboard">${heading("Control", "Dashboard", "Service and resolver state at a glance")}
    ${renderWarnings(state)}
    ${state.observability.health.error && (!stopped || state.observability.health.errorKind !== "connection_refused") ? `<div class="notice warning">Health unavailable: ${escapeHtml(state.observability.health.error)}</div>` : ""}
    ${state.observability.metrics.error && (!stopped || state.observability.metrics.errorKind !== "connection_refused") ? `<div class="notice warning">Metrics unavailable: ${escapeHtml(state.observability.metrics.error)}</div>` : ""}
    <div class="card hero"><button class="power-button" data-power data-state="${service}" data-action="toggle-service" data-focus="power" aria-label="${action} DNS Relay" title="${action} DNS Relay" ${state.applying || service === "not_installed" ? "disabled" : ""}><i data-lucide="power"></i></button><div><p class="eyebrow">Service status</p><h2 class="service-title" data-service-state>${title(service)}</h2><span class="${health ? "healthy" : "muted"}">${healthLabel}</span><div class="detail-list"><div><span>Listener</span><strong>${escapeHtml(draft?.dns_target ?? "Unavailable")}</strong></div><div><span>Mode</span><strong>${draft?.secure_only ? "Secure only" : "Standard"}</strong></div><div><span>Transport</span><strong>Configured upstreams</strong></div><div><span>Changes</span><strong>${state.dirty ? "Pending" : "Saved"}</strong></div></div></div></div>
    <div class="metrics">${metric("Requests", metrics?.total_req.toLocaleString() ?? "—", "arrow-up-right")}${metric("Cache hit", cacheHit, "database")}${metric("Failures", metrics?.failed_count.toLocaleString() ?? "—", "triangle-alert")}${metric("Timeouts", metrics?.timeout_count.toLocaleString() ?? "—", "timer")}</div>
    <div class="card empty-state">Recent events connect with bounded platform logs in the configuration checkpoint.</div>
  </section>`;
}

function metric(label: string, value: string, icon: string) {
  return `<div class="metric"><div class="metric-head"><small>${label}</small><i data-lucide="${icon}"></i></div><strong>${value}</strong></div>`;
}

function renderResolvers(state: ShellState) {
  const draft = state.app.draft;
  const resolvers = draft?.resolvers ?? [];
  return `<section class="view" data-view="resolvers">${heading("Upstreams", "Resolvers", "Ordered UDP, DoH, and DoQ endpoints")}
    <div class="toolbar"><span class="badge">${draft?.secure_only ? "Secure only" : "Mixed security"}</span><label><span class="sr-only">New resolver transport</span><select aria-label="New resolver transport" data-resolver-template><option value="https://1.1.1.1/dns-query">DoH</option><option value="quic://9.9.9.9:853">DoQ</option><option value="1.1.1.1:53">UDP</option></select></label><button class="button" data-action="add-resolver"><i data-lucide="plus"></i> Add resolver</button></div>
    <div class="card">${resolvers.length ? resolvers.map((resolver, index) => {
      const probe = state.resolverProbes[resolver];
      return `<div class="row"><span class="badge transport">${resolverTransport(resolver)}</span><div class="row-main"><label><span class="sr-only">Resolver ${index + 1}</span><input data-config-path="resolvers.${index}" value="${escapeHtml(resolver)}"></label><span class="${probe?.value?.reachable ? "healthy" : "muted"}">${probe?.error ? escapeHtml(probe.error) : probe?.value ? `${escapeHtml(probe.value.message)} · ${probe.value.latencyMs} ms` : "Configured upstream"}</span></div><div class="row-actions">${iconAction("arrow-up", `Move resolver ${index + 1} up`, "move-resolver", `data-index="${index}" data-direction="-1" ${index === 0 ? "disabled" : ""}`)}${iconAction("arrow-down", `Move resolver ${index + 1} down`, "move-resolver", `data-index="${index}" data-direction="1" ${index === resolvers.length - 1 ? "disabled" : ""}`)}${iconAction("flask-conical", `Test resolver ${index + 1}`, "test-resolver", `data-index="${index}"`)}${iconAction("trash-2", `Delete resolver ${index + 1}`, "delete-resolver", `data-index="${index}"`)}</div></div>`;
    }).join("") : '<div class="empty-state">No resolvers configured</div>'}</div>
    <div class="card form-grid">
      ${checkField("Enable resolver discovery", "resolver_searching.enable", draft?.resolver_searching.enable)}
      <label>Discovery sources<textarea data-config-path="resolver_searching.resolver_source" data-value-type="lines">${escapeHtml(draft?.resolver_searching.resolver_source.join("\n") ?? "")}</textarea></label>
      <label>Refresh interval (seconds)<input type="number" min="1" data-config-path="resolver_searching.resfresh_interval" data-value-type="optional-number" value="${draft?.resolver_searching.resfresh_interval ?? ""}"></label>
      ${checkField("Discover IPv4 resolvers", "resolver_searching.ipv4", draft?.resolver_searching.ipv4)}
      ${checkField("Discover DoH resolvers", "resolver_searching.doh", draft?.resolver_searching.doh)}
      ${checkField("Secure resolvers only", "secure_only", draft?.secure_only)}
      <label>EDNS client subnet<input data-config-path="client_subnet" data-value-type="optional-string" value="${escapeHtml(draft?.client_subnet ?? "")}" placeholder="Auto or public IPv4 /24"></label>
    </div>
  </section>`;
}

function renderRules(state: ShellState) {
  const draft = state.app.draft;
  const drops = draft?.drop_list ?? [];
  const redirects = draft?.redirect_list ?? [];
  const rows = [
    ...drops.map((domain, index) => ({ kind: "drop", domain, target: "drop", index })),
    ...redirects.map((entry, index) => {
      const split = entry.indexOf(":");
      return { kind: "redirect", domain: entry.slice(0, split), target: entry.slice(split + 1), index };
    }),
  ];
  return `<section class="view" data-view="rules">${heading("Policy", "Rules", `${drops.length} drop · ${redirects.length} redirect`)}
    <div class="toolbar"><button class="button primary" data-action="open-dialog"><i data-lucide="plus"></i> Add rule</button><label class="button">Import blocklist<input class="sr-only" type="file" accept="text/plain" data-blocklist-import></label></div>
    <div class="card">${rows.length ? rows.map((rule) => `<div class="row"><span class="badge transport">${rule.kind === "drop" ? "DROP" : "DNS"}</span><div class="row-main"><strong>${escapeHtml(rule.domain)}</strong><span>${escapeHtml(rule.target)}</span></div><div class="row-actions">${iconAction("settings", `Edit ${rule.domain}`, "edit-rule", `data-kind="${rule.kind}" data-index="${rule.index}"`)}${iconAction("trash-2", `Delete ${rule.domain}`, "delete-rule", `data-kind="${rule.kind}" data-index="${rule.index}"`)}</div></div>`).join("") : '<div class="empty-state">No rules configured</div>'}</div>
    <p class="muted">Drop lists may be imported. Redirect rules remain inline-only.</p>
  </section>`;
}

function renderRelay(state: ShellState) {
  const draft = state.app.draft;
  const relay = draft?.relay_conf;
  return `<section class="view" data-view="relay">${heading("Transport", "Relay", "Direct and Google-chained HTTPS relays")}
    <div class="card form-grid">${checkField("Enable relay", "relay_conf.enable", relay?.enable)}${checkField("Manual bootstrap", "relay_conf.resolve_manual", relay?.resolve_manual)}<label>Timeout (seconds)<input type="number" min="1" data-config-path="relay_conf.relay_timeout_sec" data-value-type="number" value="${relay?.relay_timeout_sec ?? 5}"></label><label>Client subnet<input data-config-path="client_subnet" data-value-type="optional-string" value="${escapeHtml(draft?.client_subnet ?? "")}" placeholder="Auto"></label></div>
    ${(relay?.relay_instances ?? []).map((item, index) => {
      const probe = state.relayProbes[item.relay_url];
      const revealed = state.revealedSecrets[item.relay_key];
      return `<div class="card"><div class="section-heading"><div><p class="eyebrow">Relay ${index + 1}</p><h2>${escapeHtml(item.relay_url || "New relay")}</h2></div>${iconAction("trash-2", `Delete relay ${index + 1}`, "delete-relay", `data-index="${index}"`)}</div><div class="form-grid"><label>HTTPS URL<input data-config-path="relay_conf.relay_instances.${index}.relay_url" value="${escapeHtml(item.relay_url)}"></label><label>Transport<select data-config-path="relay_conf.relay_instances.${index}.transport"><option value="direct" ${item.transport === "direct" ? "selected" : ""}>Direct</option><option value="google_chained" ${item.transport === "google_chained" ? "selected" : ""}>Google chained</option></select></label><label>Vault<select disabled><option>System keychain</option></select></label></div><div class="secret-line"><span class="secret-value">${revealed ? escapeHtml(revealed) : item.relay_key ? "••••••••••••" : "Not configured"}</span><div class="row-actions">${iconAction(revealed ? "eye-off" : "eye", revealed ? "Mask relay key" : "Reveal relay key", "reveal-relay-secret", `data-index="${index}" ${item.relay_key ? "" : "disabled"}`)}<button class="button" data-action="generate-relay-secret" data-index="${index}">${item.relay_key ? "Replace key" : "Generate key"}</button><button class="button" data-action="test-relay" data-index="${index}">Test relay</button></div></div><p class="${probe?.value?.reachable ? "healthy" : "muted"}">${probe?.error ? escapeHtml(probe.error) : probe?.value ? `${escapeHtml(probe.value.message)} · ${probe.value.latencyMs} ms` : "Not tested"}</p></div>`;
    }).join("")}
    <button class="button" data-action="add-relay"><i data-lucide="plus"></i> Add relay</button><p class="muted">Secrets stay in Keychain and are materialized only during Apply.</p>
  </section>`;
}

function renderActivity(state: ShellState) {
  const query = state.activityFilter.toLowerCase();
  const logs = (state.logs.value ?? []).filter((line) => line.toLowerCase().includes(query));
  const history = (state.history.value ?? []).filter((line) => line.toLowerCase().includes(query));
  return `<section class="view" data-view="activity">${heading("Observability", "Activity", "Bounded service logs and query history")}
    ${state.logs.error ? `<div class="notice warning">Logs unavailable: ${escapeHtml(state.logs.error)}</div>` : ""}${state.history.error ? `<div class="notice warning">History unavailable: ${escapeHtml(state.history.error)}</div>` : ""}
    <div class="card"><div class="toolbar"><label><span class="sr-only">Filter activity</span><input type="search" aria-label="Filter activity" data-activity-filter data-focus="activity-filter" value="${escapeHtml(state.activityFilter)}" placeholder="Filter activity"></label><button class="button" data-action="pause-activity">${state.activityPaused ? "Resume" : "Pause"}</button><button class="button" data-action="copy-activity"><i data-lucide="copy"></i> Copy</button><button class="button" data-action="export-activity"><i data-lucide="download"></i> Export</button><button class="button danger-button" data-action="clear-activity">Clear view</button></div><p class="eyebrow">Service logs</p>${logs.length ? `<ul class="log-list">${logs.map((line) => `<li>${escapeHtml(line)}</li>`).join("")}</ul>` : '<div class="empty-state">No matching logs</div>'}</div>
    <div class="card"><p class="eyebrow">Query history</p>${history.length ? `<ul class="log-list">${history.map((line) => `<li>${escapeHtml(line)}</li>`).join("")}</ul>` : '<div class="empty-state">No matching history</div>'}</div>
  </section>`;
}

function renderSettings(state: ShellState) {
  const draft = state.app.draft;
  const fixture = {
    normal: "All independent data sources available",
    loading: '<span class="spinner" aria-label="Loading"></span>',
    empty: "No installed service detected",
    warning: "Metrics unavailable; service status remains available",
    error: "Service failed. Open logs or run repair.",
  }[state.fixtureState];
  const history = draft?.record_history_conf;
  const obfs = draft?.obfs_conf;
  return `<section class="view" data-view="settings">${heading("System", "Settings", "Listener, service, updates, and complete configuration")}${renderWarnings(state)}
    <div class="card form-grid"><label>Listener address<input data-config-path="dns_target" value="${escapeHtml(draft?.dns_target ?? "")}"></label>${checkField("Secure resolvers only", "secure_only", draft?.secure_only, state.applying)}${checkField("Hot reload", "hotreload_conf.enable", draft?.hotreload_conf.enable)}<label>Hot reload interval (ms)<input type="number" min="1" data-config-path="hotreload_conf.poll_interval_ms" data-value-type="number" value="${draft?.hotreload_conf.poll_interval_ms ?? 1000}"></label>${checkField("VPN DNS reassertion", "vpn_reassertion", draft?.vpn_reassertion)}${checkField("Initialize TLS roots", "init_tls", draft?.init_tls)}</div>
    <div class="card form-grid"><p class="eyebrow">Metrics and history</p>${checkField("Metrics endpoint", "metric_conf.enable", draft?.metric_conf.enable)}<label>Metrics output<select data-config-path="metric_conf.report_type"><option value="log" ${draft?.metric_conf.report_type === "log" ? "selected" : ""}>Log</option><option value="http" ${draft?.metric_conf.report_type === "http" ? "selected" : ""}>HTTP</option></select></label><label>Metrics interval (seconds)<input type="number" min="1" data-config-path="metric_conf.report_interval" data-value-type="number" value="${draft?.metric_conf.report_interval ?? 30}"></label>${checkField("Record query history", "record_history", draft?.record_history)}<label>History matched domains<textarea data-config-path="record_history_conf.matched_list" data-value-type="lines">${escapeHtml(history?.matched_list.join("\n") ?? "")}</textarea></label><label>History line retention<input type="number" min="1" data-config-path="record_history_conf.lines" data-value-type="number" value="${history?.lines ?? 1000}"></label></div>
    <div class="card form-grid"><p class="eyebrow">Obfuscated listener</p>${checkField("Enable obfuscation", "obfs_conf.enable", obfs?.enable)}<label>Bind address<input data-config-path="obfs_conf.bind_addr" value="${escapeHtml(obfs?.bind_addr ?? "")}"></label><div><span class="muted">Keys</span>${(obfs?.keys ?? []).map((key, index) => `<div class="secret-line"><span>${state.revealedSecrets[key] ? escapeHtml(state.revealedSecrets[key]) : "••••••••••••"}</span><div class="row-actions">${iconAction(state.revealedSecrets[key] ? "eye-off" : "eye", `Reveal obfuscation key ${index + 1}`, "reveal-obfs-secret", `data-index="${index}"`)}${iconAction("trash-2", `Delete obfuscation key ${index + 1}`, "delete-obfs-secret", `data-index="${index}"`)}</div></div>`).join("")}<button class="button" data-action="generate-obfs-secret">Generate obfuscation key</button></div></div>
    <div class="card"><div class="section-heading"><div><p class="eyebrow">Service</p><h2>${title(state.app.service)}</h2></div></div><div class="toolbar"><button class="button" data-action="service-action" data-service-action="restart">Restart</button><button class="button" data-action="service-action" data-service-action="repair">Repair</button><button class="button danger-button" data-action="service-action" data-service-action="uninstall">Uninstall</button></div></div>
    <div class="card"><div class="section-heading"><div><p class="eyebrow">Advanced</p><h2>Raw TOML</h2></div><span class="badge">${state.rawError ? "Invalid" : "Validated on import"}</span></div><textarea aria-label="Raw TOML" data-raw-toml data-focus="raw-toml" placeholder="Load or paste TOML">${escapeHtml(state.rawToml)}</textarea>${state.rawError ? `<p class="field-error" role="alert">${escapeHtml(state.rawError)}</p>` : ""}<div class="toolbar"><button class="button" data-action="load-raw">Load draft</button><button class="button primary" data-action="validate-raw">Validate and use</button><label class="button">Import<input class="sr-only" type="file" accept=".toml,text/plain" data-config-import></label><button class="button" data-action="export-safe">Export safely</button><button class="button danger-button" data-action="export-plaintext">Export plaintext…</button></div></div>
    <div class="card"><p class="eyebrow">Shared states</p><div class="state-switches">${["normal", "loading", "empty", "warning", "error"].map((name) => `<button class="button ghost" data-action="fixture-state" data-state="${name}">${title(name as ServiceState)}</button>`).join("")}</div><div class="fixture" data-state="${state.fixtureState}">${fixture}</div></div>
  </section>`;
}

function renderSetup(state: ShellState) {
  if (state.app.recoveryRequired) {
    return `<section class="view setup" data-view="setup"><img src="${appMark}" alt=""><p class="eyebrow">Incomplete installation</p><h1>Repair DNS Relay</h1><p class="lead">Restore the fixed service assets while preserving an existing configuration.</p>${renderWarnings(state)}<button class="button primary" data-action="service-action" data-service-action="repair" ${state.applying ? "disabled" : ""}>${state.applying ? "Authorizing…" : "Repair installation"}</button></section>`;
  }
  const existing = state.app.draft === null;
  return `<section class="view setup" data-view="setup"><img src="${appMark}" alt=""><p class="eyebrow">${existing ? "Existing installation" : "First launch"}</p><h1>${existing ? "Adopt" : "Set up"} DNS Relay</h1><p class="lead">${existing ? "The installed configuration is protected from default replacement. Legacy secrets move directly into Keychain." : "Review the fixed install paths, then authorize one native install prompt."}</p>${renderWarnings(state)}<button class="button primary" data-action="${existing ? "adopt" : "install"}" ${state.applying ? "disabled" : ""}>${state.applying ? "Authorizing…" : existing ? "Adopt existing configuration" : "Install and start"}</button></section>`;
}

function renderWarnings(state: ShellState) {
  return state.app.warnings.map((warning) => `<div class="notice warning"><i data-lucide="triangle-alert"></i><span>${escapeHtml(warning)}</span></div>`).join("");
}

function iconAction(icon: string, label: string, action: string, extra = "") {
  return `<button class="icon-button" data-icon-only data-action="${action}" ${extra} aria-label="${escapeHtml(label)}" title="${escapeHtml(label)}"><i data-lucide="${icon}"></i></button>`;
}

function resolverTransport(resolver: string) {
  if (resolver.startsWith("https://")) return "DoH";
  if (resolver.startsWith("quic://")) return "DoQ";
  return "UDP";
}

function checkField(label: string, path: string, checked?: boolean, disabled = false) {
  return `<label class="check-row">${label}<input type="checkbox" data-config-path="${path}" data-focus="${path}" data-value-type="boolean" ${checked ? "checked" : ""} ${disabled ? "disabled" : ""}></label>`;
}

function title(value: string) {
  return value.replace(/_/g, " ").replace(/^./, (letter) => letter.toUpperCase());
}

function escapeHtml(value: string) {
  return value.replace(/[&<>\"]/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '\"': "&quot;" })[character] ?? character);
}
