/** A button that lets the user choose a path instead of typing it. */
export interface Picker {
  label: string;
  /** Resolves to the chosen path, or `null` when the user cancelled. */
  run: () => Promise<string | null>;
}

/** One key of a plugin that can be switched on or off in a list (a task, a command, a browser). */
export interface KeyOption {
  key: string;
  label: string;
  /** Headings are drawn above the first option of each group. */
  group?: string;
}
