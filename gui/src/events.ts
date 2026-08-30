import type { Backend } from "./backend";
import type { Store } from "./store";
import type { ViewId } from "./types";

export function bindEvents(root: HTMLElement, backend: Backend, store: Store) {
  let dialogTrigger: HTMLElement | null = null;
  let toastTimer: number | undefined;

  root.addEventListener("click", async (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-action]");
    if (!button) return;
    const action = button.dataset.action;
    if (action === "navigate") store.update((state) => { state.activeView = button.dataset.target as ViewId; });
    if (action === "mark-dirty" && !store.get().applying) store.update((state) => { state.dirty = true; });
    if (action === "fixture-state") store.update((state) => { state.fixtureState = button.dataset.state as typeof state.fixtureState; });
    if (action === "toggle-service") await serviceAction();
    if (action === "service-action") await runServiceAction(button.dataset.serviceAction as "restart" | "repair" | "uninstall");
    if (action === "revert") revert();
    if (action === "apply") await apply();
    if (action === "install") await install();
    if (action === "adopt") await adopt();
    if (action === "open-dialog") openDialog(button);
    if (action === "close-dialog") closeDialog();
  });

  root.addEventListener("change", (event) => {
    const input = event.target as HTMLInputElement;
    if (input.matches("[data-config='secure-only']") && !store.get().applying) {
      store.update((state) => {
        if (state.app.draft) state.app.draft.secure_only = input.checked;
        state.dirty = true;
      });
    }
  });

  root.addEventListener("close", (event) => {
    if ((event.target as HTMLElement).matches("[data-rule-dialog]")) dialogTrigger?.focus();
  }, true);

  async function serviceAction() {
    const current = store.get().app.service;
    const action = current === "running" ? "stop" : "start";
    await runServiceAction(action);
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
    if (store.get().applying) return;
    const draft = store.get().app.draft;
    if (!draft) return notify("Adopt the installed configuration before editing");
    const submitted = structuredClone(draft);
    store.update((state) => { state.applying = true; state.serviceRevision += 1; });
    try {
      const result = await backend.applyDraft(submitted);
      store.update((state) => {
        state.app.service = result.service;
        state.savedDraft = submitted;
        state.dirty = JSON.stringify(state.app.draft) !== JSON.stringify(submitted);
        state.applying = false;
      });
      notify(result.message);
    } catch (error) {
      store.update((state) => { state.applying = false; });
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
        state.dirty = false;
        state.applying = false;
      });
      notify("Existing configuration adopted");
    } catch (error) {
      store.update((state) => { state.applying = false; });
      notify(message(error));
    }
  }

  function revert() {
    if (store.get().applying) return;
    store.update((state) => {
      state.app.draft = structuredClone(state.savedDraft);
      state.dirty = false;
    });
    notify("Changes reverted");
  }

  function openDialog(trigger: HTMLElement) {
    dialogTrigger = trigger;
    const dialog = root.querySelector<HTMLDialogElement>("[data-rule-dialog]");
    dialog?.showModal();
    dialog?.querySelector<HTMLInputElement>("input")?.focus();
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

function message(error: unknown) {
  if (typeof error === "object" && error && "message" in error) return String(error.message);
  return String(error);
}
