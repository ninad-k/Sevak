import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import Settings from "./Settings.svelte";
import SevakUiWindow from "./lib/sevak-ui/SevakUiWindow.svelte";
import { setLanguage } from "./lib/i18n.svelte";

// The OS language until the config arrives with the shell status (or, in
// Settings, the saved form), so the first frame is already in the right language.
setLanguage("auto");

// One frontend serves all windows: the settings window loads `index.html#settings`,
// and a workflow's UI window `index.html#sevak-ui-<id>`.
const isSettings = window.location.hash === "#settings";
if (isSettings) document.documentElement.classList.add("settings");
const uiWindowId = window.location.hash.startsWith("#sevak-ui-")
  ? window.location.hash.slice("#sevak-ui-".length)
  : null;

const target = document.getElementById("app")!;
const app = uiWindowId
  ? mount(SevakUiWindow, { target, props: { id: uiWindowId } })
  : mount(isSettings ? Settings : App, { target });

export default app;
