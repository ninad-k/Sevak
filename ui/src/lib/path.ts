// Helpers for typed file paths in the search box (the files plugin lists the
// directory a path names; see crates/sevak-plugins/src/path_browse.rs).

/** Starts like a path the files plugin browses: `~/`, `/`, `C:\`, `\\server\`. */
const PATH_START = /^(~[\\/]|[\\/]|[A-Za-z]:[\\/])/;

const lastSeparator = (text: string) => Math.max(text.lastIndexOf("/"), text.lastIndexOf("\\"));

/**
 * The input after going up one directory level (Shift+Tab): from
 * `~/Documents/Work/re` or `~/Documents/Work/` to `~/Documents/`. A keyword in
 * front (`f ~/Documents/`) is kept. `null` when the input is not a path or
 * there is nowhere higher to go.
 */
export function parentPath(input: string): string | null {
  let prefix = "";
  let path = input;
  if (!PATH_START.test(path)) {
    const keyword = /^(\S+\s+)(.*)$/.exec(input);
    if (!keyword || !PATH_START.test(keyword[2])) return null;
    [, prefix, path] = keyword;
  }
  // Drop the name being typed, then the directory it is in.
  const directory = path.slice(0, lastSeparator(path) + 1);
  const body = directory.slice(0, -1);
  const above = lastSeparator(body);
  if (above < 0) return null;
  const parent = body.slice(0, above + 1);
  // `\\server\` alone is not a place that can be listed.
  if (/^\\\\[^\\/]*[\\/]$/.test(parent)) return null;
  return prefix + parent;
}
