import { createBackend } from "./backend";
import { bindEvents } from "./events";
import { render } from "./render";
import { createStore } from "./store";
import "./styles.css";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("Missing application root");

const backend = createBackend();

try {
  const [app, observability] = await Promise.all([
    backend.getAppState(),
    backend.getObservability(),
  ]);
  const store = createStore({
    savedDraft: structuredClone(app.draft),
    app,
    observability,
    activeView: "dashboard",
    dirty: false,
    applying: false,
    serviceRevision: 0,
    fixtureState: "normal",
  });
  store.subscribe((state) => render(root, state));
  bindEvents(root, backend, store);
  render(root, store.get());
  let servicePoll = 0;
  let observabilityPoll = 0;
  const interactionActive = () => {
    const active = document.activeElement;
    return store.get().applying
      || root.querySelector("dialog[open]") !== null
      || active?.matches("input, textarea, select") === true;
  };
  const serviceTimer = window.setInterval(async () => {
    if (interactionActive()) return;
    const poll = ++servicePoll;
    const revision = store.get().serviceRevision;
    try {
      const service = await backend.getServiceState();
      if (poll !== servicePoll || revision !== store.get().serviceRevision) return;
      store.update((state) => { state.app.service = service; });
    } catch {
      if (poll !== servicePoll || revision !== store.get().serviceRevision) return;
      store.update((state) => { state.app.service = "error"; });
    }
  }, 2000);
  const observabilityTimer = window.setInterval(async () => {
    if (interactionActive()) return;
    const poll = ++observabilityPoll;
    try {
      const observability = await backend.getObservability();
      if (poll !== observabilityPoll || interactionActive()) return;
      store.update((state) => { state.observability = observability; });
    } catch (error) {
      if (poll !== observabilityPoll || interactionActive()) return;
      const unavailable = { error: String(error) };
      store.update((state) => {
        state.observability = { health: unavailable, metrics: unavailable };
      });
    }
  }, 5000);
  window.addEventListener("beforeunload", () => {
    window.clearInterval(serviceTimer);
    window.clearInterval(observabilityTimer);
  });
} catch (error) {
  const main = document.createElement("main");
  main.className = "fatal-error";
  const heading = document.createElement("h1");
  heading.textContent = "DNS Relay unavailable";
  const detail = document.createElement("p");
  detail.textContent = String(error);
  main.append(heading, detail);
  root.replaceChildren(main);
}
