import { createBackend } from "./backend";
import { bindEvents } from "./events";
import { render } from "./render";
import { createStore } from "./store";
import "./styles.css";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("Missing application root");

const backend = createBackend();

try {
  const app = await backend.getAppState();
  const store = createStore({
    savedDraft: structuredClone(app.draft),
    app,
    activeView: "dashboard",
    dirty: false,
    applying: false,
    fixtureState: "normal",
  });
  store.subscribe((state) => render(root, state));
  bindEvents(root, backend, store);
  render(root, store.get());
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
