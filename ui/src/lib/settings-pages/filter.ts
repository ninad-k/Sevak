import type { KeyOption } from "./types";

/**
 * The options whose name, group or key (as words: `force_quit_app` is "force
 * quit app") contain every word typed. An empty query keeps all of them.
 */
export function filterOptions(options: KeyOption[], query: string): KeyOption[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return options;
  return options.filter((option) => {
    const haystack = `${option.label} ${option.group ?? ""} ${option.key.replace(/_/g, " ")}`.toLowerCase();
    return words.every((word) => haystack.includes(word));
  });
}
