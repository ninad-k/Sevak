// What the launcher's screen-reader live region says. The search box is an ARIA
// combobox whose `aria-activedescendant` names the selected row, so a screen
// reader already reads the row the arrows move to; this adds what it cannot
// see: how many results a search found, that a view opened in place of the
// list, and that nothing matched. Pure, so the wording is unit-tested.

export interface LauncherState {
  /** The text in the search box. */
  query: string;
  /** Rows (or tiles) shown. */
  count: number;
  /** The results are tiles in a grid. */
  grid: boolean;
  /** The action panel is open, listing this many actions (else `null`). */
  panelActions: number | null;
  /** The panel's title, such as "Actions for Firefox". */
  panelTitle?: string;
  /** The Text View is showing a row's long text. */
  textView: boolean;
  /** Large Type is showing this text (else `null`). */
  largeText: string | null;
  /** The application index is still filling. */
  indexing: boolean;
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** The sentence for the live region, or "" when there is nothing to say. */
export function launcherAnnouncement(s: LauncherState): string {
  if (s.largeText !== null) return `Large Type: ${s.largeText}. Press Escape to close.`;
  if (s.textView) return "Text view. Up and Down scroll, Escape goes back to the results.";
  if (s.panelActions !== null) {
    const title = s.panelTitle ? `${s.panelTitle}: ` : "";
    return `${title}${plural(s.panelActions, "action", "actions")}. Escape closes.`;
  }
  if (s.query.trim() === "") return "";
  if (s.count === 0) return s.indexing ? "No results yet, still indexing applications." : "No results.";
  return plural(s.count, s.grid ? "item" : "result", s.grid ? "items" : "results");
}

/**
 * Delays announcements while the user types, so a screen reader speaks the
 * count of the search that settled instead of one per keystroke. `say` gets the
 * latest text once it has been stable for `delay` ms; `flush` cancels.
 */
export function debouncedAnnouncer(say: (text: string) => void, delay = 350) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let last = "";
  return {
    update(text: string) {
      clearTimeout(timer);
      if (text === last) return;
      timer = setTimeout(() => {
        last = text;
        say(text);
      }, delay);
    },
    cancel() {
      clearTimeout(timer);
    },
  };
}
