import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import Settings from "./Settings.svelte";

// One frontend serves both windows: the settings window loads `index.html#settings`.
const isSettings = window.location.hash === "#settings";
if (isSettings) document.documentElement.classList.add("settings");

const app = mount(isSettings ? Settings : App, {
  target: document.getElementById("app")!,
});

export default app;
