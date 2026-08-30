import type { Backend } from "./backend";
import type { Store } from "./store";
import type { DnsRelayConfig, ViewId } from "./types";

type SecretKind = "relay" | "obfs";

export function bindEvents(root: HTMLElement, backend: Backend, store: Store) {
  let dialogTrigger: HTMLElement | null = null;
  let toastTimer: number | undefined;

  root.addEventListener("click", async (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-action]");
    if (!button) return;
    const action = button.dataset.action;
    if (action === "navigate") store.update((state) => { state.activeView = button.dataset.target as ViewId; });
    if (action === "fixture-state") store.update((state) => { state.fixtureState = button.dataset.state as typeof state.fixtureState; });
    if (action === "toggle-service") await toggleService();
    if (action === "service-action") await runServiceAction(button.dataset.serviceAction as "restart" | "repair" | "uninstall");
    if (action === "revert") await revert();
    if (action === "apply") await apply();
    if (action === "install") await install();
    if (action === "adopt") await adopt();
    if (action === "add-resolver") {
      const resolver = root.querySelector<HTMLSelectElement>("[data-resolver-template]")?.value;
      if (resolver) editDraft((draft) => { draft.resolvers.push(resolver); });
    }
    if (action === "move-resolver") moveResolver(Number(button.dataset.index), Number(button.dataset.direction));
    if (action === "delete-resolver") editDraft((draft) => { draft.resolvers.splice(Number(button.dataset.index), 1); });
    if (action === "test-resolver") await testResolver(Number(button.dataset.index));
    if (action === "open-dialog") openRuleDialog(button);
    if (action === "edit-rule") openRuleDialog(button, button.dataset.kind as "drop" | "redirect", Number(button.dataset.index));
    if (action === "save-rule") saveRule();
    if (action === "delete-rule") deleteRule(button.dataset.kind as "drop" | "redirect", Number(button.dataset.index));
    if (action === "close-dialog") closeDialog();
    if (action === "add-relay") editDraft((draft) => { draft.relay_conf.relay_instances.push({ relay_url: "https://", transport: "direct", relay_key: "" }); });
    if (action === "delete-relay") await deleteRelay(Number(button.dataset.index));
    if (action === "generate-relay-secret") await generateRelaySecret(Number(button.dataset.index));
    if (action === "reveal-relay-secret") await revealRelaySecret(Number(button.dataset.index));
    if (action === "test-relay") await testRelay(Number(button.dataset.index));
    if (action === "generate-obfs-secret") await generateObfsSecret();
    if (action === "reveal-obfs-secret") await revealObfsSecret(Number(button.dataset.index));
    if (action === "delete-obfs-secret") await deleteObfsSecret(Number(button.dataset.index));
    if (action === "pause-activity") store.update((state) => { state.activityPaused = !state.activityPaused; });
    if (action === "copy-activity") await copyActivity();
    if (action === "export-activity") download("dns-relay-activity.txt", activityText());
    if (action === "clear-activity") store.update((state) => { state.logs = { value: [] }; state.history = { value: [] }; });
    if (action === "load-raw") await loadRaw();
    if (action === "validate-raw") await useRaw(store.get().rawToml);
    if (action === "export-safe") await exportDraft(false);
    if (action === "export-plaintext" && window.confirm("Export secrets in plaintext? Anyone with the file can use them.")) await exportDraft(true);
  });

  root.addEventListener("change", async (event) => {
    const input = event.target as HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement;
    if (input.matches("[data-config-path]")) setConfigInput(input);
    if (input.matches("[data-rule-kind]")) showRuleTarget(input.value === "redirect");
    if (input.matches("[data-blocklist-import]")) await importBlocklist(input as HTMLInputElement);
    if (input.matches("[data-config-import]")) await importConfig(input as HTMLInputElement);
  });

  root.addEventListener("input", (event) => {
    const input = event.target as HTMLInputElement | HTMLTextAreaElement;
    if (input.matches("[data-activity-filter]")) store.update((state) => { state.activityFilter = input.value; });
    if (input.matches("[data-raw-toml]")) store.update((state) => { state.rawToml = input.value; state.rawError = undefined; });
  });

  root.addEventListener("close", (event) => {
    if ((event.target as HTMLElement).matches("[data-rule-dialog]")) dialogTrigger?.focus();
  }, true);

  async function toggleService() {
    await runServiceAction(store.get().app.service === "running" ? "stop" : "start");
  }

  async function runServiceAction(action: "start" | "stop" | "restart" | "repair" | "uninstall") {
    if (store.get().applying) return;
    store.update((state) => { state.applying = true; state.serviceRevision += 1; });
    try {
      const service = await backend.serviceAction(action);
      store.update((state) => {
        state.app.service = service;
        if (action === "repair" || action === "uninstall") {
          state.app.recoveryRequired = false;
          state.app.warnings = [];
        }
        state.applying = false;
      });
      notify(`DNS Relay ${service.replace(/_/g, " ")}`);
    } catch (error) {
      store.update((state) => { state.app.service = "error"; state.applying = false; });
      notify(message(error));
    }
  }

  async function apply() {
    if (store.get().applying || store.get().secretBusy) return;
    const draft = store.get().app.draft;
    if (!draft) return notify("Adopt the installed configuration before editing");
    const submitted = structuredClone(draft);
    store.update((state) => {
      state.applying = true;
      state.secretBusy = true;
      state.serviceRevision += 1;
    });
    try {
      const validation = await backend.validateDraft(submitted);
      if (!validation.valid) {
        store.update((state) => { state.applying = false; state.secretBusy = false; });
        return notify(validation.errors[0]?.message ?? "Configuration is invalid");
      }
      const result = await backend.applyDraft(submitted);
      const cleanup = await reconcileSecrets(submitted);
      store.update((state) => {
        state.app.service = result.service;
        state.savedDraft = submitted;
        state.dirty = cleanup.failed;
        state.generatedSecrets = cleanup.failedGenerated;
        state.pendingSecretDeletes = cleanup.failedPending;
        state.applying = false;
        state.secretBusy = false;
      });
      notify(cleanup.failed ? `${result.message}; secret cleanup is pending` : result.message);
    } catch (error) {
      store.update((state) => { state.applying = false; state.secretBusy = false; });
      notify(message(error));
    }
  }

  async function install() {
    if (store.get().applying) return;
    const draft = store.get().app.draft;
    if (!draft) return notify("Existing configuration must be adopted before editing");
    const submitted = structuredClone(draft);
    store.update((state) => { state.applying = true; state.serviceRevision += 1; });
    try {
      const result = await backend.installService(submitted);
      store.update((state) => {
        state.app.service = result.service;
        state.app.draft = submitted;
        state.savedDraft = structuredClone(submitted);
        state.app.warnings = [];
        state.app.recoveryRequired = false;
        state.generatedSecrets = [];
        state.pendingSecretDeletes = [];
        state.dirty = false;
        state.applying = false;
      });
      notify(result.message);
    } catch (error) {
      store.update((state) => { state.applying = false; });
      notify(message(error));
    }
  }

  async function adopt() {
    if (store.get().applying) return;
    store.update((state) => { state.applying = true; });
    try {
      const draft = await backend.adoptService();
      store.update((state) => {
        state.app.draft = draft;
        state.savedDraft = structuredClone(draft);
        state.app.warnings = [];
        state.app.recoveryRequired = false;
        state.generatedSecrets = [];
        state.pendingSecretDeletes = [];
        state.dirty = false;
        state.applying = false;
      });
      notify("Existing configuration adopted");
    } catch (error) {
      store.update((state) => { state.applying = false; });
      notify(message(error));
    }
  }

  function editDraft(change: (draft: DnsRelayConfig) => void) {
    if (store.get().applying) return;
    store.update((state) => {
      if (!state.app.draft) return;
      change(state.app.draft);
      state.dirty = true;
    });
  }

  function setConfigInput(input: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement) {
    const path = input.dataset.configPath;
    if (!path) return;
    let value: unknown = input.value;
    if (input.dataset.valueType === "boolean") value = (input as HTMLInputElement).checked;
    if (input.dataset.valueType === "number") value = Number(input.value);
    if (input.dataset.valueType === "optional-number") value = input.value ? Number(input.value) : undefined;
    if (input.dataset.valueType === "optional-string") value = input.value.trim() || undefined;
    if (input.dataset.valueType === "lines") value = input.value.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
    const state = store.get();
    if (!state.app.draft || state.applying) return;
    if (path.startsWith("record_history_conf.") && !state.app.draft.record_history_conf) {
      state.app.draft.record_history_conf = { matched_list: [], lines: 1000 };
    }
    setPath(state.app.draft as unknown as Record<string, unknown>, path, value);
    state.dirty = true;
    root.querySelector<HTMLElement>("[data-dirty-bar]")?.removeAttribute("hidden");
  }

  function moveResolver(index: number, direction: number) {
    editDraft((draft) => {
      const target = index + direction;
      if (!draft.resolvers[index] || target < 0 || target >= draft.resolvers.length) return;
      [draft.resolvers[index], draft.resolvers[target]] = [draft.resolvers[target], draft.resolvers[index]];
    });
  }

  async function testResolver(index: number) {
    const resolver = store.get().app.draft?.resolvers[index];
    if (!resolver) return;
    store.update((state) => { state.resolverProbes[resolver] = {}; });
    try {
      const result = await backend.testResolver(resolver);
      store.update((state) => { state.resolverProbes[resolver] = { value: result }; });
    } catch (error) {
      store.update((state) => { state.resolverProbes[resolver] = { error: message(error) }; });
    }
  }

  function openRuleDialog(trigger: HTMLElement, kind?: "drop" | "redirect", index?: number) {
    dialogTrigger = trigger;
    const dialog = root.querySelector<HTMLDialogElement>("[data-rule-dialog]");
    const draft = store.get().app.draft;
    const domain = dialog?.querySelector<HTMLInputElement>("[name='domain']");
    const target = dialog?.querySelector<HTMLInputElement>("[name='target']");
    const ruleKind = dialog?.querySelector<HTMLSelectElement>("[name='kind']");
    if (!dialog || !domain || !target || !ruleKind || !draft) return;
    dialog.dataset.kind = kind ?? "";
    dialog.dataset.index = index === undefined ? "" : String(index);
    if (kind === "drop" && index !== undefined) {
      domain.value = draft.drop_list[index] ?? "";
      ruleKind.value = "drop";
      target.value = "";
    } else if (kind === "redirect" && index !== undefined) {
      const entry = draft.redirect_list[index] ?? "";
      const split = entry.indexOf(":");
      domain.value = entry.slice(0, split);
      ruleKind.value = "redirect";
      target.value = entry.slice(split + 1);
    } else {
      domain.value = "";
      ruleKind.value = "drop";
      target.value = "";
    }
    showRuleTarget(ruleKind.value === "redirect");
    const error = dialog.querySelector<HTMLElement>("[data-rule-error]");
    if (error) error.textContent = "";
    dialog.showModal();
    domain.focus();
  }

  function saveRule() {
    const dialog = root.querySelector<HTMLDialogElement>("[data-rule-dialog]");
    const kind = dialog?.querySelector<HTMLSelectElement>("[name='kind']")?.value;
    const domain = dialog?.querySelector<HTMLInputElement>("[name='domain']")?.value.trim().toLowerCase() ?? "";
    const target = dialog?.querySelector<HTMLInputElement>("[name='target']")?.value.trim().toLowerCase() ?? "";
    const error = dialog?.querySelector<HTMLElement>("[data-rule-error]");
    if (!domain.includes(".") || (kind === "redirect" && !target.split(",").every(validIpv4))) {
      if (error) error.textContent = "Enter a valid domain and IPv4 address.";
      return;
    }
    const oldKind = dialog?.dataset.kind as "drop" | "redirect" | "";
    const oldIndex = Number(dialog?.dataset.index);
    editDraft((draft) => {
      if (oldKind === "drop") draft.drop_list.splice(oldIndex, 1);
      if (oldKind === "redirect") draft.redirect_list.splice(oldIndex, 1);
      if (kind === "drop") draft.drop_list.push(domain);
      else draft.redirect_list.push(`${domain}:${target}`);
    });
    dialog?.close();
  }

  function showRuleTarget(show: boolean) {
    const label = root.querySelector<HTMLElement>("[data-rule-target]");
    if (label) label.hidden = !show;
  }

  function deleteRule(kind: "drop" | "redirect", index: number) {
    editDraft((draft) => { (kind === "drop" ? draft.drop_list : draft.redirect_list).splice(index, 1); });
  }

  async function importBlocklist(input: HTMLInputElement) {
    const file = input.files?.[0];
    if (!file) return;
    const rules = await backend.parseBlocklist(await file.text());
    editDraft((draft) => { draft.drop_list.push(...rules.filter((rule) => !draft.drop_list.includes(rule))); });
    notify(`Imported ${rules.length} blocklist entries`);
  }

  async function generateRelaySecret(index: number) {
    const relay = store.get().app.draft?.relay_conf.relay_instances[index];
    if (!relay) return;
    await generateSecret("relay", relay.relay_key || undefined, (reference) => {
      editDraft((draft) => { draft.relay_conf.relay_instances[index].relay_key = reference; });
    });
  }

  async function generateObfsSecret() {
    await generateSecret("obfs", undefined, (reference) => {
      editDraft((draft) => { draft.obfs_conf.keys.push(reference); });
    });
  }

  async function generateSecret(kind: SecretKind, replaced: string | undefined, save: (reference: string) => void) {
    const stored = await withSecretLock(async () => {
      const reference = await backend.generateSecret(kind);
      const replacedWasGenerated = Boolean(replaced && store.get().generatedSecrets.includes(replaced));
      store.update((state) => {
        state.generatedSecrets.push(reference);
        if (replaced && !replacedWasGenerated && !state.pendingSecretDeletes.includes(replaced)) {
          state.pendingSecretDeletes.push(replaced);
        }
      });
      save(reference);
    });
    if (stored) notify(`${kind === "relay" ? "Relay" : "Obfuscation"} key stored in Keychain`);
  }

  async function revealRelaySecret(index: number) {
    const reference = store.get().app.draft?.relay_conf.relay_instances[index]?.relay_key;
    if (reference) await revealSecret(reference);
  }

  async function revealObfsSecret(index: number) {
    const reference = store.get().app.draft?.obfs_conf.keys[index];
    if (reference) await revealSecret(reference);
  }

  async function revealSecret(reference: string) {
    if (store.get().revealedSecrets[reference]) {
      store.update((state) => { delete state.revealedSecrets[reference]; });
      return;
    }
    if (!window.confirm("Reveal this secret on screen?")) return;
    await withSecretLock(async () => {
      const value = await backend.revealSecret(reference);
      store.update((state) => { state.revealedSecrets[reference] = value; });
    });
  }

  async function deleteRelay(index: number) {
    const reference = store.get().app.draft?.relay_conf.relay_instances[index]?.relay_key;
    await stageSecretRemoval(reference, () => {
      editDraft((draft) => { draft.relay_conf.relay_instances.splice(index, 1); });
    });
  }

  async function deleteObfsSecret(index: number) {
    const reference = store.get().app.draft?.obfs_conf.keys[index];
    if (!reference) return;
    await stageSecretRemoval(reference, () => {
      editDraft((draft) => { draft.obfs_conf.keys.splice(index, 1); });
    });
  }

  async function stageSecretRemoval(reference: string | undefined, remove: () => void) {
    if (!reference) return remove();
    await withSecretLock(async () => {
      if (!store.get().generatedSecrets.includes(reference)) {
        store.update((state) => {
          if (!state.pendingSecretDeletes.includes(reference)) state.pendingSecretDeletes.push(reference);
        });
      }
      remove();
    });
  }

  async function withSecretLock(operation: () => Promise<void>) {
    if (store.get().secretBusy) return false;
    store.update((state) => { state.secretBusy = true; });
    let failure: unknown;
    try {
      await operation();
    } catch (error) {
      failure = error;
    }
    store.update((state) => { state.secretBusy = false; });
    if (failure !== undefined) notify(message(failure));
    return failure === undefined;
  }

  async function reconcileSecrets(submitted: DnsRelayConfig) {
    const active = configSecretReferences(submitted);
    const failedPending: string[] = [];
    const failedGenerated: string[] = [];
    for (const reference of store.get().pendingSecretDeletes) {
      if (active.has(reference)) continue;
      try {
        await backend.deleteSecret(reference);
      } catch {
        failedPending.push(reference);
      }
    }
    for (const reference of store.get().generatedSecrets) {
      if (active.has(reference)) continue;
      try {
        await backend.deleteSecret(reference);
      } catch {
        failedGenerated.push(reference);
      }
    }
    return {
      failed: failedPending.length > 0 || failedGenerated.length > 0,
      failedPending,
      failedGenerated,
    };
  }

  async function testRelay(index: number) {
    const url = store.get().app.draft?.relay_conf.relay_instances[index]?.relay_url;
    if (!url) return;
    store.update((state) => { state.relayProbes[url] = {}; });
    try {
      const result = await backend.testRelay(url);
      store.update((state) => { state.relayProbes[url] = { value: result }; });
    } catch (error) {
      store.update((state) => { state.relayProbes[url] = { error: message(error) }; });
    }
  }

  async function copyActivity() {
    try {
      await navigator.clipboard.writeText(activityText());
      notify("Activity copied");
    } catch (error) {
      notify(message(error));
    }
  }

  function activityText() {
    const state = store.get();
    return [...(state.logs.value ?? []), ...(state.history.value ?? [])].join("\n");
  }

  async function loadRaw() {
    const draft = store.get().app.draft;
    if (!draft) return;
    try {
      const rawToml = await backend.exportConfig(draft, false);
      store.update((state) => { state.rawToml = rawToml; state.rawError = undefined; });
    } catch (error) {
      notify(message(error));
    }
  }

  async function useRaw(rawToml: string) {
    try {
      const draft = await backend.parseConfig(rawToml);
      store.update((state) => { state.app.draft = draft; state.rawError = undefined; state.dirty = true; });
      notify("Configuration is valid");
    } catch (error) {
      store.update((state) => { state.rawError = message(error); });
    }
  }

  async function importConfig(input: HTMLInputElement) {
    const file = input.files?.[0];
    if (file) await useRaw(await file.text());
  }

  async function exportDraft(plaintext: boolean) {
    const draft = store.get().app.draft;
    if (!draft) return;
    try {
      download(`dns-relay${plaintext ? "-plaintext" : ""}.toml`, await backend.exportConfig(draft, plaintext));
      notify(plaintext ? "Plaintext configuration exported" : "Secret-free configuration exported");
    } catch (error) {
      notify(message(error));
    }
  }

  async function revert() {
    if (store.get().applying || store.get().secretBusy) return;
    const failedGenerated: string[] = [];
    const failedPending: string[] = [];
    const reverted = await withSecretLock(async () => {
      const active = store.get().savedDraft
        ? configSecretReferences(store.get().savedDraft as DnsRelayConfig)
        : new Set<string>();
      for (const reference of store.get().generatedSecrets) {
        if (active.has(reference)) continue;
        try {
          await backend.deleteSecret(reference);
        } catch {
          failedGenerated.push(reference);
        }
      }
      for (const reference of store.get().pendingSecretDeletes) {
        if (active.has(reference)) continue;
        try {
          await backend.deleteSecret(reference);
        } catch {
          failedPending.push(reference);
        }
      }
      store.update((state) => {
        state.app.draft = structuredClone(state.savedDraft);
        state.revealedSecrets = {};
        state.generatedSecrets = failedGenerated;
        state.pendingSecretDeletes = failedPending;
        state.rawError = undefined;
        state.dirty = failedGenerated.length > 0 || failedPending.length > 0;
      });
    });
    if (reverted) notify(failedGenerated.length || failedPending.length ? "Changes reverted; staged secret cleanup failed" : "Changes reverted");
  }

  function closeDialog() {
    root.querySelector<HTMLDialogElement>("[data-rule-dialog]")?.close();
  }

  function notify(text: string) {
    window.clearTimeout(toastTimer);
    const toast = root.querySelector<HTMLElement>("[data-toast]");
    const live = root.querySelector<HTMLElement>("[data-live-region]");
    if (toast) { toast.textContent = text; toast.hidden = false; }
    if (live) live.textContent = text;
    toastTimer = window.setTimeout(() => { if (toast) toast.hidden = true; }, 2400);
  }
}

function setPath(root: Record<string, unknown>, path: string, value: unknown) {
  const parts = path.split(".");
  let target: Record<string, unknown> | unknown[] = root;
  for (const [index, part] of parts.slice(0, -1).entries()) {
    const key: string | number = Array.isArray(target) ? Number(part) : part;
    const next = parts[index + 1];
    if ((target as Record<string | number, unknown>)[key] == null) {
      (target as Record<string | number, unknown>)[key] = /^\d+$/.test(next) ? [] : {};
    }
    target = (target as Record<string | number, unknown>)[key] as Record<string, unknown> | unknown[];
  }
  const tail = parts[parts.length - 1];
  const last: string | number = Array.isArray(target) ? Number(tail) : tail;
  (target as Record<string | number, unknown>)[last] = value;
}

function validIpv4(value: string) {
  const octets = value.trim().split(".");
  return octets.length === 4 && octets.every((octet) => /^\d{1,3}$/.test(octet) && Number(octet) <= 255);
}

function configSecretReferences(draft: DnsRelayConfig) {
  return new Set([
    ...draft.relay_conf.relay_instances.map((relay) => relay.relay_key),
    ...draft.obfs_conf.keys,
  ].filter((reference) => reference.startsWith("vault://")));
}

function download(name: string, content: string) {
  const url = URL.createObjectURL(new Blob([content], { type: "text/plain" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  URL.revokeObjectURL(url);
}

function message(error: unknown) {
  if (typeof error === "object" && error && "message" in error) return String(error.message);
  return String(error);
}
