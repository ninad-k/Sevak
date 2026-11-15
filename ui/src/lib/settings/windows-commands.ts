// The window layouts the launcher understands, for the cheat sheet on the
// Windows settings page. `key` is the layout's key in Rust (`Layout::key` and
// `WindowCommand::key` in sevak-core's `window_layout`); it is also the last part
// of the result id a global hotkey runs (`windows:<key>`). `typed` is what to
// type after the keyword; the first spelling is the one shown. `group` is an id;
// `groupName` gives its heading in the UI language.

import { t, type MessageKey } from "../i18n.svelte";

export interface WindowCommandInfo {
  key: string;
  /** What to type after the keyword, shortest useful spelling first. */
  typed: string[];
  /** What the layout does, in the UI language (read through a getter). */
  readonly label: string;
  group: "Halves" | "Quarters" | "Thirds" | "Size and place" | "Displays";
}

function command(key: string, typed: string[], group: WindowCommandInfo["group"]): WindowCommandInfo {
  const label = `pages.windows.commands.${camelCase(key)}` as MessageKey;
  return {
    key,
    typed,
    group,
    get label() {
      return t(label);
    },
  };
}

const camelCase = (key: string) => key.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());

const GROUP_NAMES: Record<WindowCommandInfo["group"], MessageKey> = {
  Halves: "pages.windows.groups.halves",
  Quarters: "pages.windows.groups.quarters",
  Thirds: "pages.windows.groups.thirds",
  "Size and place": "pages.windows.groups.sizePlace",
  Displays: "pages.windows.groups.displays",
};

/** The heading of a group in the UI language. */
export function groupName(group: WindowCommandInfo["group"]): string {
  return t(GROUP_NAMES[group]);
}

export const WINDOW_COMMANDS: WindowCommandInfo[] = [
  command("left", ["left"], "Halves"),
  command("right", ["right"], "Halves"),
  command("top", ["top"], "Halves"),
  command("bottom", ["bottom"], "Halves"),
  command("top_left", ["top left", "tl"], "Quarters"),
  command("top_right", ["top right", "tr"], "Quarters"),
  command("bottom_left", ["bottom left", "bl"], "Quarters"),
  command("bottom_right", ["bottom right", "br"], "Quarters"),
  command("left_third", ["left third"], "Thirds"),
  command("center_third", ["center third"], "Thirds"),
  command("right_third", ["right third"], "Thirds"),
  command("left_two_thirds", ["left two thirds"], "Thirds"),
  command("right_two_thirds", ["right two thirds"], "Thirds"),
  command("maximize", ["max"], "Size and place"),
  command("almost_maximize", ["almost-max"], "Size and place"),
  command("center", ["center"], "Size and place"),
  command("restore", ["restore"], "Size and place"),
  command("next_display", ["next display"], "Displays"),
  command("previous_display", ["previous display"], "Displays"),
];

/** The groups in display order, with their commands. */
export function commandGroups(): { name: WindowCommandInfo["group"]; commands: WindowCommandInfo[] }[] {
  const names: WindowCommandInfo["group"][] = [];
  for (const command of WINDOW_COMMANDS) {
    if (!names.includes(command.group)) names.push(command.group);
  }
  return names.map((name) => ({
    name,
    commands: WINDOW_COMMANDS.filter((command) => command.group === name),
  }));
}

/** The search to type for a command: the keyword (as configured, or the default) and the layout. */
export function typedCommand(keyword: string, command: WindowCommandInfo): string {
  return `${keyword.trim() || "win"} ${command.typed[0]}`;
}

/** The `[[hotkey]]` entry that runs a layout from a global shortcut. */
export function hotkeyRun(command: WindowCommandInfo): string {
  return `windows:${command.key}`;
}

/** Whether `text` is a whole number from 0 to `max` (as the Rust side requires of the gap). */
export function isValidGap(value: unknown, max: number): boolean {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= max;
}
