import {
  Activity,
  ArrowUpRight,
  CircleDotDashed,
  Database,
  Gauge,
  Network,
  Plus,
  Power,
  Route,
  Settings,
  ShieldBan,
  Timer,
  TriangleAlert,
  X,
  createIcons,
} from "lucide";

import appMark from "../src-tauri/icons/icon.png";
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
  CircleDotDashed,
  Database,
  Gauge,
  Network,
  Plus,
  Power,
  Route,
  Settings,
  ShieldBan,
  Timer,
  TriangleAlert,
  X,
};

export function render(root: HTMLElement, state: ShellState) {
  const active = document.activeElement as HTMLElement | null;
  const focus = active && root.contains(active) ? active.dataset.focus : undefined;
  const setup = state.app.draft === null || state.app.service === "not_installed";
  root.innerHTML = `<div class="app-shell" data-app-shell>
    <header class="app-header">
      <div class="brand"><img src="${appMark}" alt="" width="38" height="38"><div><strong>DNS Relay</strong><span>Local control plane</span></div></div>
      <span class="status-pill" data-header-state data-state="${setup ? "applying" : state.app.service}">${setup ? "Setup" : title(state.app.service)}</span>
    </header>
    <div class="app-layout">
      <nav class="primary-nav" aria-label="Primary">${views.map(([id, icon, label]) => navButton(id, icon, label, state, setup)).join("")}</nav>
      <main class="view-host" data-view-host ${state.applying ? "inert" : ""}>${setup ? renderSetup(state) : renderView(state)}</main>
    </div>
    <section class="dirty-bar" data-dirty-bar ${state.dirty && !setup ? "" : "hidden"} aria-label="Unsaved configuration">
      <span><i data-lucide="circle-dot-dashed"></i> Pending changes</span><div><button class="button ghost" data-action="revert" data-focus="revert" ${state.applying ? "disabled" : ""}>Revert changes</button><button class="button primary" data-action="apply" data-focus="apply" ${state.applying ? "disabled" : ""}>${state.applying ? "Applying…" : "Apply changes"}</button></div>
    </section>
    <dialog class="dialog" data-rule-dialog aria-labelledby="rule-dialog-title"><form method="dialog"><div class="section-heading"><div><p class="eyebrow">Rules</p><h2 id="rule-dialog-title">Add rule</h2></div><button class="icon-button" data-icon-only data-action="close-dialog" aria-label="Close rule editor" title="Close rule editor"><i data-lucide="x"></i></button></div><label>Domain pattern<input name="domain" autocomplete="off" placeholder="ads.example"></label><label>Rule target<input name="target" autocomplete="off" placeholder="drop or IP address"></label><div class="dialog-actions"><button class="button ghost" data-action="close-dialog">Cancel</button><button class="button primary" value="save">Save rule</button></div></form></dialog>
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
  const action = running ? "Stop" : "Start";
  const draft = state.app.draft;
  return `<section class="view" data-view="dashboard">${heading("Control", "Dashboard", "Service and resolver state at a glance")}
    ${renderWarnings(state)}
    <div class="card hero"><button class="power-button" data-power data-state="${service}" data-action="toggle-service" data-focus="power" aria-label="${action} DNS Relay" title="${action} DNS Relay" ${state.applying || service === "not_installed" ? "disabled" : ""}><i data-lucide="power"></i></button><div><p class="eyebrow">Service status</p><h2 class="service-title" data-service-state>${title(service)}</h2><span class="muted">Native service control</span><div class="detail-list"><div><span>Listener</span><strong>${escapeHtml(draft?.dns_target ?? "Unavailable")}</strong></div><div><span>Mode</span><strong>${draft?.secure_only ? "Secure only" : "Standard"}</strong></div><div><span>Transport</span><strong>Configured upstreams</strong></div><div><span>Changes</span><strong>${state.dirty ? "Pending" : "Saved"}</strong></div></div></div></div>
    <div class="metrics">${metric("Requests", "—", "arrow-up-right")}${metric("Cache hit", "—", "database")}${metric("Failures", "—", "triangle-alert")}${metric("Latency", "—", "timer")}</div>
    <div class="card empty-state">Live metrics and recent events connect in the next checkpoint.</div>
  </section>`;
}

function metric(label: string, value: string, icon: string) {
  return `<div class="metric"><div class="metric-head"><small>${label}</small><i data-lucide="${icon}"></i></div><strong>${value}</strong></div>`;
}

function renderResolvers(state: ShellState) {
  const resolvers = state.app.draft?.resolvers ?? [];
  return `<section class="view" data-view="resolvers">${heading("Upstreams", "Resolvers", "Ordered DNS endpoints")}<div class="toolbar"><span class="badge">${state.app.draft?.secure_only ? "Secure only" : "Mixed security"}</span><button class="button" data-action="mark-dirty"><i data-lucide="plus"></i> Add resolver</button></div><div class="card">${resolvers.length ? resolvers.map((resolver) => `<div class="row"><span class="badge transport">DNS</span><div class="row-main"><strong>${escapeHtml(resolver)}</strong><span>Configured upstream</span></div></div>`).join("") : '<div class="empty-state">No resolvers configured</div>'}</div></section>`;
}

function renderRules(state: ShellState) {
  const count = (state.app.draft?.drop_list.length ?? 0) + (state.app.draft?.redirect_list.length ?? 0);
  return `<section class="view" data-view="rules">${heading("Policy", "Rules", `${count} configured rules`)}<div class="toolbar"><label><span class="sr-only">Search rules</span><input type="search" aria-label="Search rules" placeholder="Search domain rules"></label><button class="button primary" data-action="open-dialog"><i data-lucide="plus"></i> Add rule</button></div><div class="card empty-state">Structured rule editing connects in the configuration checkpoint.</div></section>`;
}

function renderRelay(state: ShellState) {
  const relay = state.app.draft?.relay_conf;
  return `<section class="view" data-view="relay">${heading("Transport", "Relay", "Direct and Google-chained HTTPS relays")}<div class="card"><div class="section-heading"><div><p class="eyebrow">Relay mode</p><h2>${relay?.enable ? "Enabled" : "Disabled"}</h2></div><span class="badge">${relay?.relay_instances.length ?? 0} configured</span></div><p class="muted">Secrets stay in the system credential vault and are materialized only during Apply.</p></div></section>`;
}

function renderActivity(_state: ShellState) {
  return `<section class="view" data-view="activity">${heading("Observability", "Activity", "Metrics, service logs, and query history")}<div class="card empty-state">Activity sources are loading independently.</div></section>`;
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
  return `<section class="view" data-view="settings">${heading("System", "Settings", "Listener, service, updates, and configuration")}${renderWarnings(state)}<div class="card form-grid"><label>Listener address<input value="${escapeHtml(draft?.dns_target ?? "")}" disabled></label><label class="check-row">Secure resolvers only<input type="checkbox" data-config="secure-only" data-focus="secure-only" ${draft?.secure_only ? "checked" : ""} ${state.applying ? "disabled" : ""}></label></div><div class="card"><p class="eyebrow">Shared states</p><div class="state-switches">${["normal", "loading", "empty", "warning", "error"].map((name) => `<button class="button ghost" data-action="fixture-state" data-state="${name}">${title(name as ServiceState)}</button>`).join("")}</div><div class="fixture" data-state="${state.fixtureState}">${fixture}</div></div></section>`;
}

function renderSetup(state: ShellState) {
  const existing = state.app.draft === null;
  return `<section class="view setup" data-view="setup"><img src="${appMark}" alt=""><p class="eyebrow">${existing ? "Existing installation" : "First launch"}</p><h1>${existing ? "Adopt" : "Set up"} DNS Relay</h1><p class="lead">${existing ? "The installed configuration is protected from default replacement." : "Review the fixed install paths, then authorize one native install prompt."}</p>${renderWarnings(state)}<button class="button primary" disabled>${existing ? "Adoption" : "Installation"} connects in the next checkpoint</button></section>`;
}

function renderWarnings(state: ShellState) {
  return state.app.warnings.map((warning) => `<div class="notice warning"><i data-lucide="triangle-alert"></i><span>${escapeHtml(warning)}</span></div>`).join("");
}

function title(value: string) {
  return value.replace(/_/g, " ").replace(/^./, (letter) => letter.toUpperCase());
}

function escapeHtml(value: string) {
  return value.replace(/[&<>\"]/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '\"': "&quot;" })[character] ?? character);
}
