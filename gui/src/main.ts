import { createBackend } from "./backend";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("Missing application root");

const state = await createBackend().getAppState();
root.textContent = `DNS Relay — ${state.service.replace("_", " ")}`;
