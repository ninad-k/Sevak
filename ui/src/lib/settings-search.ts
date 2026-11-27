// The Settings sidebar filter and the "go to this section" index.
//
// `settings-index.json` is the same list the launcher's `? ` search uses
// (`crates/sevak-plugins/src/settings_index.json`; a Rust test keeps the two
// files identical). Pure functions only, so they are easy to test.

import index from "./settings-index.json";

export interface IndexEntry {
  id: string;
  section: string;
  title: string;
  keywords: string;
}

export const SETTINGS_INDEX: IndexEntry[] = index;

/** Section ids, in sidebar order as the index lists them. */
export const SECTION_IDS: string[] = SETTINGS_INDEX.filter((entry) => entry.id === entry.section).map(
  (entry) => entry.id,
);

export function isSectionId(id: string): boolean {
  return SECTION_IDS.includes(id);
}

/** Lowercased words of a section's title and of every entry in it (options too). */
function synonyms(sectionId: string): string {
  return SETTINGS_INDEX.filter((entry) => entry.section === sectionId)
    .map((entry) => `${entry.title} ${entry.keywords}`)
    .join(" ")
    .toLowerCase();
}

/**
 * The sections whose title or synonyms contain every word typed. An empty
 * query keeps all of them, in order.
 */
export function filterSections<T extends { id: string; label: string }>(sections: T[], query: string): T[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return sections;
  return sections.filter((section) => {
    const haystack = `${section.label.toLowerCase()} ${synonyms(section.id)}`;
    return words.every((word) => haystack.includes(word));
  });
}
