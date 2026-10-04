// The window layouts the launcher understands, for the cheat sheet on the
// Windows settings page. `key` is the layout's key in Rust (`Layout::key` and
// `WindowCommand::key` in sevak-core's `window_layout`); it is also the last part
// of the result id a global hotkey runs (`windows:<key>`). `typed` is what to
// type after the keyword; the first spelling is the one shown.

export interface WindowCommandInfo {
  key: string;
  /** What to type after the keyword, shortest useful spelling first. */
  typed: string[];
  label: string;
  group: "Halves" | "Quarters" | "Thirds" | "Size and place" | "Displays";
}

export const WINDOW_COMMANDS: WindowCommandInfo[] = [
  { key: "left", typed: ["left"], label: "Left half of the screen", group: "Halves" },
  { key: "right", typed: ["right"], label: "Right half of the screen", group: "Halves" },
  { key: "top", typed: ["top"], label: "Top half of the screen", group: "Halves" },
  { key: "bottom", typed: ["bottom"], label: "Bottom half of the screen", group: "Halves" },
  { key: "top_left", typed: ["top left", "tl"], label: "Top left quarter", group: "Quarters" },
  { key: "top_right", typed: ["top right", "tr"], label: "Top right quarter", group: "Quarters" },
  { key: "bottom_left", typed: ["bottom left", "bl"], label: "Bottom left quarter", group: "Quarters" },
  { key: "bottom_right", typed: ["bottom right", "br"], label: "Bottom right quarter", group: "Quarters" },
  { key: "left_third", typed: ["left third"], label: "Left third", group: "Thirds" },
  { key: "center_third", typed: ["center third"], label: "Middle third", group: "Thirds" },
  { key: "right_third", typed: ["right third"], label: "Right third", group: "Thirds" },
  { key: "left_two_thirds", typed: ["left two thirds"], label: "Left two thirds", group: "Thirds" },
  { key: "right_two_thirds", typed: ["right two thirds"], label: "Right two thirds", group: "Thirds" },
  { key: "maximize", typed: ["max"], label: "Fill the screen", group: "Size and place" },
  { key: "almost_maximize", typed: ["almost-max"], label: "Fill most of the screen, with a margin", group: "Size and place" },
  { key: "center", typed: ["center"], label: "Keep the size, center the window", group: "Size and place" },
  { key: "restore", typed: ["restore"], label: "Back to where the window was before", group: "Size and place" },
  { key: "next_display", typed: ["next display"], label: "Move to the next display", group: "Displays" },
  { key: "previous_display", typed: ["previous display"], label: "Move to the previous display", group: "Displays" },
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
