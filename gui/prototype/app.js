const state = {
  setupComplete: true,
  service: "running",
  activeView: "dashboard",
  dirty: false,
  secureOnly: true,
  metrics: { requests: 18429, cacheHit: 71, failed: 12, latency: 34 },
  resolvers: [
    { id: 1, name: "Cloudflare Security", endpoint: "https://security.cloudflare-dns.com/dns-query", type: "DoH", result: "Ready", latency: null },
    { id: 2, name: "Quad9 Secure", endpoint: "tls://9.9.9.9", type: "DoQ", result: "Ready", latency: null },
    { id: 3, name: "Local Unbound", endpoint: "127.0.0.1:5335", type: "UDP", result: "Ready", latency: null },
  ],
  dropRules: [
    { id: 1, domain: "*.telemetry.example", target: "drop", enabled: true },
    { id: 2, domain: "ads.vendor.test", target: "drop", enabled: false },
  ],
  redirectRules: [{ id: 3, domain: "router.home", target: "192.168.1.1", enabled: true }],
  relays: [{ id: 1, name: "Primary relay", url: "https://relay.example/dns-query", transport: "Direct", enabled: true }],
  logs: [
    "19:40:01 INFO resolver ready on 127.0.0.1:53",
    "19:40:03 INFO secure upstream Cloudflare Security: 31 ms",
    "19:41:18 WARN blocked tracker.example",
  ],
  relaySecretVisible: false,
  relayResult: "Not tested",
  activityPaused: false,
  ruleSearch: "",
  fixtureState: "normal",
};

let savedConfig = snapshotConfig();
let toastTimer;

const host = document.querySelector("[data-view-host]");
const dirtyBar = document.querySelector("[data-dirty-bar]");
const toast = document.querySelector("[data-toast]");
const liveRegion = document.querySelector("[data-live-region]");
const ruleDialog = document.querySelector("[data-rule-dialog]");
const ruleForm = document.querySelector("[data-rule-form]");

function snapshotConfig() {
  return structuredClone({
    secureOnly: state.secureOnly,
    dropRules: state.dropRules,
    redirectRules: state.redirectRules,
    relays: state.relays,
  });
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"]/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[char]);
}

function iconButton(icon, label, action, extra = "") {
  return `<button class="icon-button" data-icon-only data-action="${action}" ${extra} aria-label="${escapeHtml(label)}" title="${escapeHtml(label)}"><i data-lucide="${icon}"></i></button>`;
}

function announce(message) {
  liveRegion.textContent = "";
  requestAnimationFrame(() => { liveRegion.textContent = message; });
}

function showToast(message) {
  clearTimeout(toastTimer);
  toast.textContent = message;
  toast.hidden = false;
  toastTimer = setTimeout(() => { toast.hidden = true; }, 2400);
  announce(message);
}

function setDirty() {
  state.dirty = true;
  dirtyBar.hidden = false;
}

function render() {
  const header = document.querySelector("[data-header-state]");
  header.textContent = state.setupComplete ? titleCase(state.service) : "Setup";
  header.dataset.state = state.setupComplete ? state.service : "applying";
  document.querySelectorAll("[data-view-nav]").forEach((button) => {
    const active = state.setupComplete && button.dataset.target === state.activeView;
    if (active) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
    button.disabled = !state.setupComplete;
  });
  host.innerHTML = state.setupComplete ? renderView(state.activeView) : renderSetup();
  dirtyBar.hidden = !state.dirty || !state.setupComplete;
  window.lucide?.createIcons({ attrs: { "stroke-width": 1.8 } });
}

function titleCase(value) {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

function renderView(view) {
  const renderers = { dashboard: renderDashboard, resolvers: renderResolvers, rules: renderRules, relay: renderRelay, activity: renderActivity, settings: renderSettings };
  return renderers[view]();
}

function pageHeading(kicker, title, detail = "") {
  return `<div class="page-heading"><div><p class="eyebrow">${kicker}</p><h1>${title}</h1>${detail ? `<span class="muted">${detail}</span>` : ""}</div></div>`;
}

function renderDashboard() {
  const running = state.service === "running";
  const action = running ? "Stop" : "Start";
  return `<section class="view" data-view="dashboard">
    ${pageHeading("Control", "Dashboard", "Service, resolver, and traffic at a glance")}
    <div class="card hero">
      <button class="power-button" data-power data-state="${state.service}" data-action="toggle-service" aria-label="${action} DNS Relay" title="${action} DNS Relay" ${state.service === "applying" ? "disabled" : ""}><i data-lucide="power"></i></button>
      <div><p class="eyebrow">Service status</p><h2 class="service-title" data-service-state>${titleCase(state.service)}</h2><span class="muted">Healthy · uptime 4h 17m</span>
        <div class="detail-list"><div><span>Listener</span><strong>127.0.0.1:53</strong></div><div><span>Upstream</span><strong>Cloudflare Security</strong></div><div><span>Mode</span><strong>${state.secureOnly ? "Secure only" : "Standard"}</strong></div><div><span>Transport</span><strong>DoH</strong></div></div>
      </div>
    </div>
    <div class="metrics">
      ${metric("Requests", state.metrics.requests.toLocaleString(), "arrow-up-right")}
      ${metric("Cache hit", `${state.metrics.cacheHit}%`, "database")}
      ${metric("Failures", state.metrics.failed, "triangle-alert")}
      ${metric("Latency", `${state.metrics.latency} ms`, "timer")}
    </div>
    <div class="card"><div class="section-heading"><div><p class="eyebrow">Recent events</p><h2>Resolver activity</h2></div><span class="badge">Live</span></div><ul class="log-list">${state.logs.slice(-3).map((log) => `<li>${escapeHtml(log)}</li>`).join("")}</ul></div>
    <div class="card row"><div class="row-main"><strong>Version 1.6.10</strong><span>Bundled service matches installed version</span></div><span class="healthy">Up to date</span></div>
  </section>`;
}

function metric(label, value, icon) {
  return `<div class="metric"><div class="metric-head"><small>${label}</small><i data-lucide="${icon}"></i></div><strong>${value}</strong></div>`;
}

function renderResolvers() {
  return `<section class="view" data-view="resolvers">
    ${pageHeading("Upstreams", "Resolvers", "Latency-ranked secure DNS endpoints")}
    <div class="toolbar"><span class="badge">${state.secureOnly ? "Secure only" : "Mixed security"}</span><button class="button" data-action="mark-dirty"><i data-lucide="plus"></i> Add resolver</button></div>
    <div class="card">${state.resolvers.map((resolver) => `<div class="row"><span class="badge transport">${resolver.type}</span><div class="row-main"><strong>${escapeHtml(resolver.name)}</strong><span>${escapeHtml(resolver.endpoint)}</span></div><span data-resolver-result class="${resolver.result === "Healthy" ? "healthy" : "muted"}">${resolver.result}${resolver.latency ? ` · ${resolver.latency} ms` : ""}</span>${iconButton("flask-conical", `Test ${resolver.name}`, "test-resolver", `data-id="${resolver.id}"`)}</div>`).join("")}</div>
    <div class="card form-grid"><label>Discovery sources<input value="public-dns.info, built-in"></label><label>Refresh interval<input value="30 min"></label><label>EDNS client subnet<input value="Auto"></label><label class="check-row">Secure resolvers only<input type="checkbox" data-config="secure-only" ${state.secureOnly ? "checked" : ""}></label></div>
  </section>`;
}

function allRules() {
  return [...state.dropRules, ...state.redirectRules];
}

function renderRules() {
  const query = state.ruleSearch.toLowerCase();
  const rules = allRules().filter((rule) => rule.domain.toLowerCase().includes(query));
  return `<section class="view" data-view="rules">
    ${pageHeading("Policy", "Rules", `${state.dropRules.length} drop · ${state.redirectRules.length} redirect`)}
    <div class="toolbar"><label><span class="sr-only">Search rules</span><input type="search" aria-label="Search rules" data-rule-search value="${escapeHtml(state.ruleSearch)}" placeholder="Search domain rules"></label><button class="button primary" data-action="add-rule"><i data-lucide="plus"></i> Add rule</button></div>
    <div class="card">${rules.length ? rules.map(renderRuleRow).join("") : `<div class="empty-state"><i data-lucide="search-x"></i><p>No matching rules</p></div>`}</div>
    <p class="muted">Redirect rules are inline-only. Drop lists may also be imported from local files.</p>
  </section>`;
}

function renderRuleRow(rule) {
  const enableLabel = `${rule.enabled ? "Disable" : "Enable"} ${rule.domain}`;
  return `<div class="row" data-rule-row data-enabled="${rule.enabled}" data-id="${rule.id}"><span class="badge transport">${rule.target === "drop" ? "DROP" : "DNS"}</span><div class="row-main"><strong>${escapeHtml(rule.domain)}</strong><span>${escapeHtml(rule.target)}</span></div><div class="row-actions">${iconButton(rule.enabled ? "toggle-right" : "toggle-left", enableLabel, "toggle-rule", `data-id="${rule.id}"`)}${iconButton("pencil", `Edit ${rule.domain}`, "edit-rule", `data-id="${rule.id}"`)}${iconButton("trash-2", `Delete ${rule.domain}`, "delete-rule", `data-id="${rule.id}"`)}</div></div>`;
}

function renderRelay() {
  const relay = state.relays[0];
  return `<section class="view" data-view="relay">
    ${pageHeading("Transport", "Relay", "Direct and Google-chained HTTPS relays")}
    <div class="card"><div class="section-heading"><div><p class="eyebrow">Primary</p><h2>${relay.name}</h2></div><span class="badge">Enabled</span></div><div class="detail-list"><div><span>Transport</span><strong>${relay.transport}</strong></div><div><span>Timeout</span><strong>2 seconds</strong></div><div><span>URL</span><strong>${relay.url}</strong></div><div><span>Bootstrap</span><strong>Manual</strong></div></div></div>
    <div class="card"><p class="eyebrow">Relay key</p><div class="secret-line"><span class="secret-value" data-relay-secret>${state.relaySecretVisible ? "rk_demo_7f4c21" : "••••••••••••"}</span>${iconButton(state.relaySecretVisible ? "eye-off" : "eye", state.relaySecretVisible ? "Mask relay key" : "Reveal relay key", "toggle-secret")}</div><div class="toolbar"><button class="button" data-action="mark-dirty">Generate replacement</button><button class="button primary" data-action="test-relay">Test relay</button></div><p data-relay-result class="${state.relayResult === "Reachable" ? "healthy" : "muted"}">${state.relayResult}</p></div>
    <div class="card form-grid"><label>Client subnet<input value="Auto"></label><label>Manual bootstrap<input value="1.1.1.1"></label><label>Google chain<input value="Disabled"></label><label>Vault<input value="System keychain"></label></div>
  </section>`;
}

function renderActivity() {
  const logs = state.logs.filter((line) => !state.activityFilter || line.toLowerCase().includes(state.activityFilter.toLowerCase()));
  return `<section class="view" data-view="activity">
    ${pageHeading("Observability", "Activity", "Metrics, service logs, and query history")}
    <div class="card"><div class="metric-head"><div><p class="eyebrow">Requests</p><h2>${state.metrics.requests.toLocaleString()}</h2></div><span class="badge">5 min</span></div><div class="sparkline" aria-label="Request activity chart">${[28, 44, 37, 65, 52, 78, 62, 88, 70, 94, 76, 86].map((height) => `<i style="height:${height}%"></i>`).join("")}</div></div>
    <div class="card"><div class="toolbar"><label><span class="sr-only">Filter logs</span><input aria-label="Filter logs" data-log-filter value="${escapeHtml(state.activityFilter || "")}" placeholder="Filter logs"></label><button class="button" data-action="pause-activity">${state.activityPaused ? "Resume" : "Pause"}</button><button class="button danger-button" data-action="clear-activity">Clear</button></div>${logs.length ? `<ul class="log-list">${logs.map((log) => `<li>${escapeHtml(log)}</li>`).join("")}</ul>` : `<div class="empty-state">No activity</div>`}</div>
    <div class="card row"><div class="row-main"><strong>Query history</strong><span>Retained for 24 hours · 4,218 rows</span></div><button class="button">Export</button></div>
  </section>`;
}

function renderSettings() {
  return `<section class="view" data-view="settings">
    ${pageHeading("System", "Settings", "Listener, service, updates, and configuration")}
    <div class="card form-grid"><label>Listener address<input value="127.0.0.1:53"></label><label>History retention<input value="24 hours"></label><label class="check-row">Secure resolvers only<input type="checkbox" data-config="secure-only" ${state.secureOnly ? "checked" : ""}></label><label class="check-row">Hot reload<input type="checkbox" checked></label><label class="check-row">VPN DNS reassertion<input type="checkbox" checked></label><label class="check-row">Metrics endpoint<input type="checkbox" checked></label></div>
    <div class="card"><div class="section-heading"><div><p class="eyebrow">Service</p><h2>Installed · 1.6.10</h2></div><span class="healthy">Healthy</span></div><div class="toolbar"><button class="button">Restart</button><button class="button">Repair</button><button class="button danger-button">Uninstall</button><button class="button ghost" data-action="restart-setup">Restart setup</button></div></div>
    <div class="card"><p class="eyebrow">Prototype states</p><div class="state-switches">${["normal", "loading", "empty", "warning", "error"].map((fixture) => `<button class="button ghost" data-action="fixture-state" data-state="${fixture}">${titleCase(fixture)}</button>`).join("")}</div>${renderFixture()}</div>
    <div class="card"><div class="section-heading"><div><p class="eyebrow">Advanced</p><h2>Raw TOML</h2></div><span class="badge">Valid</span></div><textarea aria-label="Raw TOML">listen = "127.0.0.1:53"\nsecure_only = ${state.secureOnly}</textarea><div class="toolbar"><button class="button">Import</button><button class="button">Export safely</button><button class="button" data-action="mark-dirty">Validate draft</button></div></div>
  </section>`;
}

function renderFixture() {
  const content = {
    normal: "All independent data sources available",
    loading: '<span class="spinner" aria-label="Loading"></span>',
    empty: "No installed service detected",
    warning: "Metrics unavailable; service status remains available",
    error: "Service failed. Open logs or run repair.",
  }[state.fixtureState];
  return `<div class="fixture" data-state="${state.fixtureState}">${content}</div>`;
}

function renderSetup() {
  return `<section class="view setup" data-view="setup"><img src="assets/app-mark.png" alt=""><p class="eyebrow">First launch</p><h1>Set up DNS Relay</h1><p class="lead">Review the fixed install paths, choose a secure resolver, then authorize one native install prompt.</p><div class="setup-steps"><div class="setup-step"><div><strong>Confirm locations</strong><span>Application files and system-only configuration</span></div></div><div class="setup-step"><div><strong>Choose resolver</strong><span>Cloudflare Security · DoH · secure only</span></div></div><div class="setup-step"><div><strong>Install service</strong><span>One administrator prompt; the GUI stays unprivileged</span></div></div></div><button class="button primary" data-action="complete-setup">Install and start</button><p class="muted">Existing configuration can be imported and validated after setup.</p></section>`;
}

function findRule(id) {
  return allRules().find((rule) => rule.id === Number(id));
}

function openRuleEditor(rule) {
  ruleForm.reset();
  ruleForm.elements.ruleId.value = rule?.id || "";
  ruleForm.elements.domain.value = rule?.domain || "";
  ruleForm.elements.target.value = rule?.target || "drop";
  document.querySelector("[data-rule-title]").textContent = rule ? "Edit rule" : "Add rule";
  document.querySelector("[data-rule-error]").textContent = "";
  ruleDialog.showModal();
  ruleForm.elements.domain.focus();
}

document.addEventListener("click", (event) => {
  const button = event.target.closest("[data-action]");
  if (!button) return;
  const { action } = button.dataset;
  if (action === "navigate") { state.activeView = button.dataset.target; render(); }
  if (action === "toggle-service") {
    const next = state.service === "running" ? "stopped" : "running";
    state.service = "applying"; render();
    setTimeout(() => { state.service = next; render(); announce(`DNS Relay ${next}`); }, 140);
  }
  if (action === "test-resolver") {
    const resolver = state.resolvers.find((item) => item.id === Number(button.dataset.id));
    resolver.result = "Testing"; resolver.latency = null; render();
    setTimeout(() => { resolver.result = "Healthy"; resolver.latency = 28 + resolver.id * 3; render(); announce(`${resolver.name} is healthy`); }, 120);
  }
  if (action === "add-rule") openRuleEditor();
  if (action === "edit-rule") openRuleEditor(findRule(button.dataset.id));
  if (action === "toggle-rule") { const rule = findRule(button.dataset.id); rule.enabled = !rule.enabled; setDirty(); render(); }
  if (action === "delete-rule") {
    const id = Number(button.dataset.id);
    state.dropRules = state.dropRules.filter((rule) => rule.id !== id);
    state.redirectRules = state.redirectRules.filter((rule) => rule.id !== id);
    setDirty(); render();
  }
  if (action === "close-dialog") ruleDialog.close();
  if (action === "toggle-secret") { state.relaySecretVisible = !state.relaySecretVisible; render(); }
  if (action === "test-relay") { state.relayResult = "Testing"; render(); setTimeout(() => { state.relayResult = "Reachable · 82 ms"; render(); }, 120); }
  if (action === "mark-dirty") { setDirty(); render(); }
  if (action === "revert") { Object.assign(state, structuredClone(savedConfig)); state.dirty = false; render(); showToast("Changes reverted"); }
  if (action === "apply") { savedConfig = snapshotConfig(); state.dirty = false; render(); showToast("Configuration applied"); }
  if (action === "pause-activity") { state.activityPaused = !state.activityPaused; render(); }
  if (action === "clear-activity") { state.logs = []; render(); announce("Activity cleared"); }
  if (action === "fixture-state") { state.fixtureState = button.dataset.state; render(); }
  if (action === "restart-setup") { state.setupComplete = false; state.dirty = false; render(); }
  if (action === "complete-setup") { state.setupComplete = true; state.service = "running"; state.activeView = "dashboard"; render(); announce("DNS Relay installed and running"); }
});

document.addEventListener("change", (event) => {
  if (event.target.matches("[data-config='secure-only']")) {
    state.secureOnly = event.target.checked;
    setDirty();
    render();
  }
});

document.addEventListener("input", (event) => {
  if (event.target.matches("[data-rule-search]")) { state.ruleSearch = event.target.value; render(); }
  if (event.target.matches("[data-log-filter]")) { state.activityFilter = event.target.value; render(); }
});

ruleForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const domain = ruleForm.elements.domain.value.trim().toLowerCase();
  const target = ruleForm.elements.target.value.trim().toLowerCase();
  const error = document.querySelector("[data-rule-error]");
  if (!domain || !domain.includes(".") || (!target || (target !== "drop" && !/^\d{1,3}(\.\d{1,3}){3}$/.test(target)))) {
    error.textContent = "Enter a domain and use drop or an IPv4 target.";
    return;
  }
  const id = Number(ruleForm.elements.ruleId.value);
  const existing = id ? findRule(id) : null;
  if (existing) { existing.domain = domain; existing.target = target; }
  else {
    const rule = { id: Math.max(0, ...allRules().map((item) => item.id)) + 1, domain, target, enabled: true };
    (target === "drop" ? state.dropRules : state.redirectRules).push(rule);
  }
  setDirty();
  ruleDialog.close();
  render();
});

render();
