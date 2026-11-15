// What the launcher's screen-reader live region says. The search box is an ARIA
// combobox whose `aria-activedescendant` names the selected row, so a screen
// reader already reads the row the arrows move to; this adds what it cannot
// see: how many results a search found, that a view opened in place of the
// list, and that nothing matched. Pure, so the wording is unit-tested; the
// words come from the `announce` messages of the current language.

import { t } from "./i18n.svelte";

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

/** The sentence for the live region, or "" when there is nothing to say. */
export function launcherAnnouncement(s: LauncherState): string {
  if (s.largeText !== null) return t("announce.largeType", { text: s.largeText });
  if (s.textView) return t("announce.textView");
  if (s.panelActions !== null) {
    return s.panelTitle
      ? t("announce.panelTitled", { title: s.panelTitle, count: s.panelActions })
      : t("announce.panel", { count: s.panelActions });
  }
  if (s.query.trim() === "") return "";
  if (s.count === 0) return s.indexing ? t("announce.noResultsIndexing") : t("announce.noResults");
  return t(s.grid ? "announce.items" : "announce.results", { count: s.count });
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
