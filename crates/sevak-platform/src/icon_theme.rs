//! Icon lookup following the freedesktop Icon Theme Specification.
//!
//! Plain `std::fs` code with caller-supplied base directories, so it compiles
//! and is tested on every platform (the tests build a fake theme tree in a
//! temp directory); only the Linux provider feeds it real directories.
//!
//! Create one [`IconResolver`] per indexing pass and call
//! [`IconResolver::resolve`] for every application. Directory listings are read
//! lazily and cached, so hundreds of lookups cost a handful of `read_dir` calls
//! instead of a `stat` per candidate path.
//!
//! Only PNG and SVG are returned from theme lookups: web views cannot render
//! XPM, which legacy icons in `/usr/share/pixmaps` sometimes use.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::desktop_entry::parse_groups;
use crate::icon_file;

/// Every theme implicitly inherits from this one; it is also the theme used
/// when none is configured.
pub const FALLBACK_THEME: &str = "hicolor";

/// Extensions that mean "the `.desktop` author included the file extension in
/// `Icon=`", which the spec forbids but is common.
const IMAGE_EXTENSIONS: [&str; 7] = ["png", "svg", "svgz", "xpm", "ico", "jpg", "jpeg"];

/// Parses the output of `gsettings get org.gnome.desktop.interface icon-theme`
/// (`'Yaru'\n`) into the theme name.
pub fn parse_gsettings_string(output: &str) -> Option<String> {
    let trimmed = output.trim();
    let unquoted = trimmed
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .or_else(|| trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
        .unwrap_or(trimmed)
        .trim();
    (!unquoted.is_empty()).then(|| unquoted.to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirKind {
    Fixed,
    Scalable,
    Threshold,
}

/// One subdirectory of a theme, from its `index.theme` section.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ThemeDir {
    subdir: String,
    size: i64,
    scale: i64,
    kind: DirKind,
    min_size: i64,
    max_size: i64,
    threshold: i64,
}

impl ThemeDir {
    /// The spec's `DirectoryMatchesSize`.
    fn matches(&self, size: u32, scale: u32) -> bool {
        let (size, scale) = (i64::from(size), i64::from(scale));
        if self.scale != scale {
            return false;
        }
        match self.kind {
            DirKind::Fixed => self.size == size,
            DirKind::Scalable => self.min_size <= size && size <= self.max_size,
            DirKind::Threshold => {
                self.size - self.threshold <= size && size <= self.size + self.threshold
            }
        }
    }

    /// The spec's `DirectorySizeDistance`.
    fn distance(&self, size: u32, scale: u32) -> i64 {
        let wanted = i64::from(size) * i64::from(scale);
        let range_distance = |min: i64, max: i64| {
            if wanted < min * self.scale {
                min * self.scale - wanted
            } else if wanted > max * self.scale {
                wanted - max * self.scale
            } else {
                0
            }
        };
        match self.kind {
            DirKind::Fixed => (self.size * self.scale - wanted).abs(),
            DirKind::Scalable => range_distance(self.min_size, self.max_size),
            DirKind::Threshold => {
                range_distance(self.size - self.threshold, self.size + self.threshold)
            }
        }
    }
}

/// The parsed `index.theme` of one theme.
#[derive(Debug, Default)]
struct ThemeIndex {
    inherits: Vec<String>,
    dirs: Vec<ThemeDir>,
}

fn parse_index_theme(content: &str) -> ThemeIndex {
    let groups = parse_groups(content);
    let Some(main) = groups.iter().find(|group| group.name == "Icon Theme") else {
        return ThemeIndex::default();
    };
    let list = |key: &str| -> Vec<String> {
        main.get(key)
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };

    let mut seen = HashSet::new();
    let mut dirs = Vec::new();
    for name in list("Directories")
        .into_iter()
        .chain(list("ScaledDirectories"))
    {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(section) = groups.iter().find(|group| group.name == name) else {
            continue;
        };
        let number = |key: &str| section.get(key).and_then(|v| v.trim().parse::<i64>().ok());
        let Some(size) = number("Size") else {
            continue;
        };
        let kind = match section.get("Type").map(str::trim) {
            Some("Fixed") => DirKind::Fixed,
            Some("Scalable") => DirKind::Scalable,
            _ => DirKind::Threshold,
        };
        dirs.push(ThemeDir {
            subdir: name,
            size,
            scale: number("Scale").unwrap_or(1).max(1),
            kind,
            min_size: number("MinSize").unwrap_or(size),
            max_size: number("MaxSize").unwrap_or(size),
            threshold: number("Threshold").unwrap_or(2),
        });
    }
    ThemeIndex {
        inherits: list("Inherits"),
        dirs,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ext {
    Png,
    Svg,
}

impl Ext {
    fn from_str(ext: &str) -> Option<Self> {
        if ext.eq_ignore_ascii_case("png") {
            Some(Self::Png)
        } else if ext.eq_ignore_ascii_case("svg") {
            Some(Self::Svg)
        } else {
            None
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
        }
    }
}

/// One file found while scanning a theme: where it lives, not its path (the
/// path is rebuilt on demand to keep the per-theme map small).
#[derive(Debug, Clone, Copy)]
struct Candidate {
    dir: usize,
    root: usize,
    ext: Ext,
}

#[derive(Debug)]
struct Theme {
    inherits: Vec<String>,
    dirs: Vec<ThemeDir>,
    /// `<base dir>/<theme name>` for every base directory that has the theme.
    roots: Vec<PathBuf>,
    /// icon name -> files, built on first lookup.
    files: Option<HashMap<String, Vec<Candidate>>>,
}

impl Theme {
    fn ensure_scanned(&mut self) {
        self.files.get_or_insert_with(|| {
            let mut files: HashMap<String, Vec<Candidate>> = HashMap::new();
            for (root_idx, root) in self.roots.iter().enumerate() {
                for (dir_idx, dir) in self.dirs.iter().enumerate() {
                    let Ok(entries) = fs::read_dir(root.join(&dir.subdir)) else {
                        continue;
                    };
                    for entry in entries.flatten() {
                        let file_name = entry.file_name();
                        let Some((stem, ext)) = file_name.to_str().and_then(split_extension) else {
                            continue;
                        };
                        let Some(ext) = Ext::from_str(ext) else {
                            continue;
                        };
                        files.entry(stem.to_owned()).or_default().push(Candidate {
                            dir: dir_idx,
                            root: root_idx,
                            ext,
                        });
                    }
                }
            }
            files
        });
    }

    /// The spec's `LookupIcon` for one theme: the first directory that matches
    /// the size exactly, otherwise the one with the smallest size distance.
    fn lookup(&mut self, name: &str, size: u32, scale: u32) -> Option<PathBuf> {
        self.ensure_scanned();
        let candidates = self.files.as_ref()?.get(name)?;
        let best = candidates.iter().min_by_key(|candidate| {
            let dir = &self.dirs[candidate.dir];
            // Directory order, then base-directory order, break ties. Scalable
            // directories prefer SVG, fixed ones PNG.
            let ext_rank = match (dir.kind, candidate.ext) {
                (DirKind::Scalable, Ext::Svg) | (_, Ext::Png) => 0,
                _ => 1,
            };
            (
                !dir.matches(size, scale),
                dir.distance(size, scale),
                candidate.dir,
                candidate.root,
                ext_rank,
            )
        })?;
        Some(
            self.roots[best.root]
                .join(&self.dirs[best.dir].subdir)
                .join(format!("{name}.{}", best.ext.as_str())),
        )
    }
}

/// Splits `foo.png` into (`foo`, `png`); the extension is the part after the
/// last dot, so `org.gnome.Calculator.svg` keeps its reverse-DNS stem.
fn split_extension(file_name: &str) -> Option<(&str, &str)> {
    file_name
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
}

/// Resolves `Icon=` values to image files. See the module docs.
#[derive(Debug)]
pub struct IconResolver {
    theme_name: String,
    /// Icon-theme roots: `~/.icons`, `<data dir>/icons`, ...
    base_dirs: Vec<PathBuf>,
    /// Unthemed fallback directories such as `/usr/share/pixmaps`.
    pixmap_dirs: Vec<PathBuf>,
    /// The requested theme's inheritance chain, ending in `hicolor`.
    chain: Option<Vec<String>>,
    themes: HashMap<String, Option<Theme>>,
    /// Directory listings for the unthemed fallback: stem -> has png / has svg.
    flat_dirs: HashMap<PathBuf, HashMap<String, (bool, bool)>>,
    results: HashMap<(String, u32, u32), Option<PathBuf>>,
}

impl IconResolver {
    /// `theme` is the user's current icon theme (`None` means `hicolor`).
    /// `base_dirs` are the icon-theme roots in precedence order and
    /// `pixmap_dirs` the unthemed directories searched last.
    pub fn new(theme: Option<&str>, base_dirs: Vec<PathBuf>, pixmap_dirs: Vec<PathBuf>) -> Self {
        Self {
            theme_name: theme
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .unwrap_or(FALLBACK_THEME)
                .to_owned(),
            base_dirs,
            pixmap_dirs,
            chain: None,
            themes: HashMap::new(),
            flat_dirs: HashMap::new(),
            results: HashMap::new(),
        }
    }

    /// Finds the image file for an `Icon=` value at `size` logical pixels and
    /// `scale` (Sevak asks for 48 at scale 1).
    ///
    /// Absolute paths are used as-is when the file exists and the web view can
    /// display it. Names go through the theme lookup: the current theme, its
    /// `Inherits` chain, `hicolor`, then the unthemed directories.
    pub fn resolve(&mut self, icon: &str, size: u32, scale: u32) -> Option<PathBuf> {
        let icon = icon.trim();
        if icon.is_empty() {
            return None;
        }
        let key = (icon.to_owned(), size, scale);
        if let Some(cached) = self.results.get(&key) {
            return cached.clone();
        }
        let found = self.resolve_uncached(icon, size, scale);
        self.results.insert(key, found.clone());
        found
    }

    fn resolve_uncached(&mut self, icon: &str, size: u32, scale: u32) -> Option<PathBuf> {
        let as_path = Path::new(icon);
        if as_path.is_absolute() {
            return (icon_file::mime_for(as_path).is_some() && as_path.is_file())
                .then(|| as_path.to_owned());
        }

        let name = strip_image_extension(icon);
        if name.is_empty() || name.contains(['/', '\\']) {
            return None;
        }
        for theme in self.chain() {
            if let Some(path) = self.lookup_in_theme(&theme, name, size, scale) {
                return Some(path);
            }
        }
        self.lookup_unthemed(name)
    }

    /// The requested theme, its ancestors depth-first without cycles, and
    /// `hicolor` last.
    fn chain(&mut self) -> Vec<String> {
        if let Some(chain) = &self.chain {
            return chain.clone();
        }
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        self.collect_chain(&self.theme_name.clone(), &mut chain, &mut visited);
        if !visited.contains(FALLBACK_THEME) {
            chain.push(FALLBACK_THEME.to_owned());
        }
        self.chain = Some(chain.clone());
        chain
    }

    fn collect_chain(
        &mut self,
        name: &str,
        chain: &mut Vec<String>,
        visited: &mut HashSet<String>,
    ) {
        if !visited.insert(name.to_owned()) {
            return;
        }
        let Some(theme) = self.theme(name) else {
            return;
        };
        let parents = theme.inherits.clone();
        chain.push(name.to_owned());
        for parent in parents {
            self.collect_chain(&parent, chain, visited);
        }
    }

    /// Loads (once) the theme's `index.theme`; `None` if it is not installed.
    fn theme(&mut self, name: &str) -> Option<&mut Theme> {
        if !self.themes.contains_key(name) {
            let loaded = self.load_theme(name);
            self.themes.insert(name.to_owned(), loaded);
        }
        self.themes.get_mut(name)?.as_mut()
    }

    fn load_theme(&self, name: &str) -> Option<Theme> {
        if name.is_empty() || name == ".." || name.contains(['/', '\\']) {
            return None;
        }
        let roots: Vec<PathBuf> = self
            .base_dirs
            .iter()
            .map(|base| base.join(name))
            .filter(|root| root.is_dir())
            .collect();
        // The first base directory with an index.theme defines the theme.
        let content = roots
            .iter()
            .find_map(|root| fs::read_to_string(root.join("index.theme")).ok())?;
        let index = parse_index_theme(&content);
        Some(Theme {
            inherits: index.inherits,
            dirs: index.dirs,
            roots,
            files: None,
        })
    }

    fn lookup_in_theme(
        &mut self,
        theme: &str,
        name: &str,
        size: u32,
        scale: u32,
    ) -> Option<PathBuf> {
        self.theme(theme)?.lookup(name, size, scale)
    }

    /// The spec's `LookupFallbackIcon`: `<dir>/<name>.png|svg` directly in the
    /// base directories, then the pixmap directories.
    fn lookup_unthemed(&mut self, name: &str) -> Option<PathBuf> {
        let dirs: Vec<PathBuf> = self
            .base_dirs
            .iter()
            .chain(&self.pixmap_dirs)
            .cloned()
            .collect();
        for dir in dirs {
            let listing = self
                .flat_dirs
                .entry(dir.clone())
                .or_insert_with(|| read_flat_dir(&dir));
            if let Some(&(png, svg)) = listing.get(name) {
                let ext = if png {
                    Ext::Png
                } else if svg {
                    Ext::Svg
                } else {
                    continue;
                };
                return Some(dir.join(format!("{name}.{}", ext.as_str())));
            }
        }
        None
    }
}

/// Lists the png/svg files directly inside `dir`: stem -> (has png, has svg).
fn read_flat_dir(dir: &Path) -> HashMap<String, (bool, bool)> {
    let mut listing: HashMap<String, (bool, bool)> = HashMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return listing;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some((stem, ext)) = file_name.to_str().and_then(split_extension) else {
            continue;
        };
        match Ext::from_str(ext) {
            Some(Ext::Png) => listing.entry(stem.to_owned()).or_default().0 = true,
            Some(Ext::Svg) => listing.entry(stem.to_owned()).or_default().1 = true,
            None => {}
        }
    }
    listing
}

/// Removes a trailing image extension (`foo.png` -> `foo`) but not other dots
/// (`org.gnome.Calculator` stays whole).
fn strip_image_extension(icon: &str) -> &str {
    match split_extension(icon) {
        Some((stem, ext)) if IMAGE_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)) => stem,
        _ => icon,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    /// Writes `<root>/<theme>/index.theme` and returns the theme directory.
    fn write_theme(root: &Path, theme: &str, index: &str) -> PathBuf {
        let dir = root.join(theme);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("index.theme"), index).unwrap();
        dir
    }

    const HICOLOR_INDEX: &str = "\
[Icon Theme]
Name=Hicolor
Directories=16x16/apps,48x48/apps,128x128/apps,scalable/apps

[16x16/apps]
Size=16
Context=Applications
Type=Threshold

[48x48/apps]
Size=48
Context=Applications
Type=Threshold

[128x128/apps]
Size=128
Context=Applications
Type=Threshold

[scalable/apps]
Size=128
MinSize=8
MaxSize=512
Context=Applications
Type=Scalable
";

    fn resolver(theme: Option<&str>, tmp: &TempDir) -> IconResolver {
        IconResolver::new(
            theme,
            vec![tmp.path().join("icons")],
            vec![tmp.path().join("pixmaps")],
        )
    }

    // --- index.theme parsing ---

    #[test]
    fn parses_index_theme() {
        let index = parse_index_theme(
            "\
[Icon Theme]
Name=Yaru
Inherits=Adwaita, hicolor
Directories=16x16/apps,scalable/apps
ScaledDirectories=16x16@2x/apps

[16x16/apps]
Size=16

[16x16@2x/apps]
Size=16
Scale=2
Type=Fixed

[scalable/apps]
Size=128
MinSize=16
MaxSize=256
Type=Scalable
Threshold=9
",
        );
        assert_eq!(index.inherits, ["Adwaita", "hicolor"]);
        assert_eq!(index.dirs.len(), 3);
        // Defaults: Type=Threshold, Threshold=2, Scale=1, Min/MaxSize=Size.
        assert_eq!(
            index.dirs[0],
            ThemeDir {
                subdir: "16x16/apps".into(),
                size: 16,
                scale: 1,
                kind: DirKind::Threshold,
                min_size: 16,
                max_size: 16,
                threshold: 2,
            }
        );
        // Directories come first, then ScaledDirectories.
        assert_eq!(index.dirs[1].kind, DirKind::Scalable);
        assert_eq!((index.dirs[1].min_size, index.dirs[1].max_size), (16, 256));
        assert_eq!(index.dirs[1].threshold, 9);
        assert_eq!(index.dirs[2].scale, 2);
        assert_eq!(index.dirs[2].kind, DirKind::Fixed);
    }

    #[test]
    fn index_skips_directories_without_a_section_or_size() {
        let index = parse_index_theme(
            "[Icon Theme]\nDirectories=a,b,c\n\n[a]\nSize=16\n\n[c]\nType=Fixed\n",
        );
        let names: Vec<_> = index.dirs.iter().map(|d| d.subdir.as_str()).collect();
        assert_eq!(names, ["a"]);
    }

    #[test]
    fn size_matching_and_distance() {
        let fixed = ThemeDir {
            subdir: "f".into(),
            size: 32,
            scale: 1,
            kind: DirKind::Fixed,
            min_size: 32,
            max_size: 32,
            threshold: 2,
        };
        assert!(fixed.matches(32, 1));
        assert!(!fixed.matches(33, 1));
        assert!(!fixed.matches(32, 2));
        assert_eq!(fixed.distance(48, 1), 16);
        assert_eq!(fixed.distance(16, 1), 16);

        let scalable = ThemeDir {
            kind: DirKind::Scalable,
            min_size: 16,
            max_size: 256,
            size: 128,
            ..fixed.clone()
        };
        assert!(scalable.matches(48, 1));
        assert!(!scalable.matches(512, 1));
        assert_eq!(scalable.distance(48, 1), 0);
        assert_eq!(scalable.distance(8, 1), 8);
        assert_eq!(scalable.distance(300, 1), 44);

        let threshold = ThemeDir {
            kind: DirKind::Threshold,
            size: 48,
            threshold: 2,
            ..fixed
        };
        assert!(threshold.matches(46, 1) && threshold.matches(50, 1));
        assert!(!threshold.matches(51, 1));
        assert_eq!(threshold.distance(50, 1), 0);
        assert_eq!(threshold.distance(52, 1), 2);
        assert_eq!(threshold.distance(40, 1), 6);

        let hidpi = ThemeDir {
            scale: 2,
            size: 24,
            kind: DirKind::Fixed,
            ..threshold
        };
        assert!(hidpi.matches(24, 2));
        assert_eq!(hidpi.distance(48, 1), 0);
    }

    #[test]
    fn gsettings_output() {
        assert_eq!(parse_gsettings_string("'Yaru'\n").as_deref(), Some("Yaru"));
        assert_eq!(
            parse_gsettings_string("'Adwaita-dark'").as_deref(),
            Some("Adwaita-dark")
        );
        assert_eq!(parse_gsettings_string("\"x\"").as_deref(), Some("x"));
        assert_eq!(parse_gsettings_string("''\n"), None);
        assert_eq!(parse_gsettings_string(""), None);
    }

    // --- lookup ---

    #[test]
    fn picks_the_exact_size() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(&icons, "hicolor", HICOLOR_INDEX);
        touch(&hicolor.join("16x16/apps/foo.png"));
        touch(&hicolor.join("48x48/apps/foo.png"));
        touch(&hicolor.join("128x128/apps/foo.png"));
        touch(&hicolor.join("scalable/apps/foo.svg"));

        let found = resolver(None, &tmp).resolve("foo", 48, 1).unwrap();
        assert_eq!(found, hicolor.join("48x48/apps/foo.png"));
    }

    #[test]
    fn falls_back_to_the_closest_size() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(&icons, "hicolor", HICOLOR_INDEX);
        touch(&hicolor.join("16x16/apps/small.png"));
        touch(&hicolor.join("128x128/apps/small.png"));
        touch(&hicolor.join("128x128/apps/large.png"));
        touch(&hicolor.join("16x16/apps/tiny.png"));

        let mut r = resolver(None, &tmp);
        // 48 is 32 away from 16 and 80 away from 128.
        assert_eq!(
            r.resolve("small", 48, 1).unwrap(),
            hicolor.join("16x16/apps/small.png")
        );
        assert_eq!(
            r.resolve("large", 48, 1).unwrap(),
            hicolor.join("128x128/apps/large.png")
        );
        assert_eq!(
            r.resolve("tiny", 48, 1).unwrap(),
            hicolor.join("16x16/apps/tiny.png")
        );
    }

    #[test]
    fn scalable_directory_matches_any_size_in_range() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(&icons, "hicolor", HICOLOR_INDEX);
        touch(&hicolor.join("scalable/apps/vector.svg"));
        touch(&hicolor.join("16x16/apps/vector.png"));
        let found = resolver(None, &tmp).resolve("vector", 48, 1).unwrap();
        assert_eq!(found, hicolor.join("scalable/apps/vector.svg"));
    }

    #[test]
    fn follows_the_inherits_chain_before_hicolor() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let index = |inherits: &str| {
            format!(
                "[Icon Theme]\nName=T\nInherits={inherits}\nDirectories=48x48/apps\n\n[48x48/apps]\nSize=48\nType=Fixed\n"
            )
        };
        let top = write_theme(&icons, "Top", &index("Mid"));
        let mid = write_theme(&icons, "Mid", &index("Base"));
        let base = write_theme(&icons, "Base", &index(""));
        let hicolor = write_theme(&icons, "hicolor", &index(""));

        touch(&top.join("48x48/apps/only-top.png"));
        touch(&mid.join("48x48/apps/only-mid.png"));
        touch(&base.join("48x48/apps/only-base.png"));
        touch(&hicolor.join("48x48/apps/only-hicolor.png"));
        for t in [&top, &mid, &base, &hicolor] {
            touch(&t.join("48x48/apps/everywhere.png"));
        }

        let mut r = resolver(Some("Top"), &tmp);
        assert_eq!(
            r.resolve("everywhere", 48, 1).unwrap(),
            top.join("48x48/apps/everywhere.png")
        );
        assert_eq!(
            r.resolve("only-mid", 48, 1).unwrap(),
            mid.join("48x48/apps/only-mid.png")
        );
        assert_eq!(
            r.resolve("only-base", 48, 1).unwrap(),
            base.join("48x48/apps/only-base.png")
        );
        assert_eq!(
            r.resolve("only-hicolor", 48, 1).unwrap(),
            hicolor.join("48x48/apps/only-hicolor.png")
        );
        assert_eq!(r.resolve("missing", 48, 1), None);
    }

    #[test]
    fn inheritance_cycles_terminate() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let index = |name: &str, parent: &str| {
            format!(
                "[Icon Theme]\nName={name}\nInherits={parent}\nDirectories=48x48/apps\n\n[48x48/apps]\nSize=48\n"
            )
        };
        let a = write_theme(&icons, "A", &index("A", "B"));
        write_theme(&icons, "B", &index("B", "A"));
        touch(&a.join("48x48/apps/x.png"));
        let mut r = resolver(Some("A"), &tmp);
        assert_eq!(r.resolve("x", 48, 1).unwrap(), a.join("48x48/apps/x.png"));
        assert_eq!(r.resolve("nope", 48, 1), None);
    }

    #[test]
    fn unknown_theme_still_uses_hicolor() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(&icons, "hicolor", HICOLOR_INDEX);
        touch(&hicolor.join("48x48/apps/foo.png"));
        let found = resolver(Some("Does-Not-Exist"), &tmp)
            .resolve("foo", 48, 1)
            .unwrap();
        assert_eq!(found, hicolor.join("48x48/apps/foo.png"));
    }

    #[test]
    fn user_directory_overrides_system_directory() {
        let tmp = TempDir::new().unwrap();
        let user = tmp.path().join("user-icons");
        let system = tmp.path().join("system-icons");
        let user_hi = write_theme(&user, "hicolor", HICOLOR_INDEX);
        let system_hi = write_theme(&system, "hicolor", HICOLOR_INDEX);
        touch(&user_hi.join("48x48/apps/foo.png"));
        touch(&system_hi.join("48x48/apps/foo.png"));
        touch(&system_hi.join("48x48/apps/bar.png"));

        let mut r = IconResolver::new(None, vec![user, system], vec![]);
        assert_eq!(
            r.resolve("foo", 48, 1).unwrap(),
            user_hi.join("48x48/apps/foo.png")
        );
        // Files only in the lower-precedence base dir are still found.
        assert_eq!(
            r.resolve("bar", 48, 1).unwrap(),
            system_hi.join("48x48/apps/bar.png")
        );
    }

    #[test]
    fn skips_xpm_and_other_formats() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(&icons, "hicolor", HICOLOR_INDEX);
        touch(&hicolor.join("48x48/apps/legacy.xpm"));
        touch(&hicolor.join("48x48/apps/legacy.txt"));
        touch(&tmp.path().join("pixmaps/legacy.xpm"));
        assert_eq!(resolver(None, &tmp).resolve("legacy", 48, 1), None);
    }

    #[test]
    fn pixmaps_are_the_last_resort() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        write_theme(&icons, "hicolor", HICOLOR_INDEX);
        let pixmaps = tmp.path().join("pixmaps");
        touch(&pixmaps.join("old-app.png"));
        touch(&pixmaps.join("old-app.xpm"));
        touch(&pixmaps.join("both.svg"));
        touch(&pixmaps.join("both.png"));
        let mut r = resolver(None, &tmp);
        assert_eq!(
            r.resolve("old-app", 48, 1).unwrap(),
            pixmaps.join("old-app.png")
        );
        assert_eq!(r.resolve("both", 48, 1).unwrap(), pixmaps.join("both.png"));
    }

    #[test]
    fn themed_icon_beats_pixmap() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(&icons, "hicolor", HICOLOR_INDEX);
        touch(&hicolor.join("48x48/apps/foo.png"));
        touch(&tmp.path().join("pixmaps/foo.png"));
        assert_eq!(
            resolver(None, &tmp).resolve("foo", 48, 1).unwrap(),
            hicolor.join("48x48/apps/foo.png")
        );
    }

    #[test]
    fn loose_files_in_the_icon_base_dir_are_found() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        write_theme(&icons, "hicolor", HICOLOR_INDEX);
        touch(&icons.join("loose.png"));
        assert_eq!(
            resolver(None, &tmp).resolve("loose", 48, 1).unwrap(),
            icons.join("loose.png")
        );
    }

    #[test]
    fn extension_in_icon_name_is_stripped() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(&icons, "hicolor", HICOLOR_INDEX);
        touch(&hicolor.join("48x48/apps/foo.png"));
        let mut r = resolver(None, &tmp);
        let expected = hicolor.join("48x48/apps/foo.png");
        assert_eq!(r.resolve("foo.png", 48, 1).unwrap(), expected);
        assert_eq!(r.resolve("foo.svg", 48, 1).unwrap(), expected);
        assert_eq!(r.resolve("foo.XPM", 48, 1).unwrap(), expected);
    }

    #[test]
    fn reverse_dns_names_keep_their_dots() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(&icons, "hicolor", HICOLOR_INDEX);
        touch(&hicolor.join("scalable/apps/org.gnome.Calculator.svg"));
        let mut r = resolver(None, &tmp);
        assert_eq!(
            r.resolve("org.gnome.Calculator", 48, 1).unwrap(),
            hicolor.join("scalable/apps/org.gnome.Calculator.svg")
        );
        assert_eq!(
            strip_image_extension("org.gnome.Calculator"),
            "org.gnome.Calculator"
        );
        assert_eq!(
            strip_image_extension("org.gnome.Calculator.png"),
            "org.gnome.Calculator"
        );
    }

    #[test]
    fn absolute_paths() {
        let tmp = TempDir::new().unwrap();
        let png = tmp.path().join("custom.png");
        let xpm = tmp.path().join("custom.xpm");
        let svg = tmp.path().join("custom.svg");
        touch(&png);
        touch(&xpm);
        let mut r = resolver(None, &tmp);
        assert_eq!(r.resolve(png.to_str().unwrap(), 48, 1), Some(png.clone()));
        assert_eq!(r.resolve(xpm.to_str().unwrap(), 48, 1), None);
        assert_eq!(r.resolve(svg.to_str().unwrap(), 48, 1), None);
    }

    #[test]
    fn rejects_empty_and_path_like_names() {
        let tmp = TempDir::new().unwrap();
        let mut r = resolver(None, &tmp);
        assert_eq!(r.resolve("", 48, 1), None);
        assert_eq!(r.resolve("  ", 48, 1), None);
        assert_eq!(r.resolve("../etc/passwd", 48, 1), None);
        assert_eq!(r.resolve("sub/dir", 48, 1), None);
    }

    #[test]
    fn scale_selects_scaled_directories() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(
            &icons,
            "hicolor",
            "[Icon Theme]\nDirectories=48x48/apps\nScaledDirectories=48x48@2x/apps\n\n\
             [48x48/apps]\nSize=48\nType=Fixed\n\n[48x48@2x/apps]\nSize=48\nScale=2\nType=Fixed\n",
        );
        touch(&hicolor.join("48x48/apps/foo.png"));
        touch(&hicolor.join("48x48@2x/apps/foo.png"));
        let mut r = resolver(None, &tmp);
        assert_eq!(
            r.resolve("foo", 48, 1).unwrap(),
            hicolor.join("48x48/apps/foo.png")
        );
        assert_eq!(
            r.resolve("foo", 48, 2).unwrap(),
            hicolor.join("48x48@2x/apps/foo.png")
        );
    }

    #[test]
    fn results_are_cached_across_lookups() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(&icons, "hicolor", HICOLOR_INDEX);
        touch(&hicolor.join("48x48/apps/foo.png"));
        let mut r = resolver(None, &tmp);
        let first = r.resolve("foo", 48, 1);
        // Deleting the file does not matter: the directory was already listed.
        fs::remove_file(hicolor.join("48x48/apps/foo.png")).unwrap();
        assert_eq!(r.resolve("foo", 48, 1), first);
        assert!(first.is_some());
    }

    #[test]
    fn many_lookups_against_a_large_theme_stay_fast() {
        let tmp = TempDir::new().unwrap();
        let icons = tmp.path().join("icons");
        let hicolor = write_theme(&icons, "hicolor", HICOLOR_INDEX);
        let dir = hicolor.join("48x48/apps");
        fs::create_dir_all(&dir).unwrap();
        for i in 0..2000 {
            fs::write(dir.join(format!("icon-{i}.png")), b"x").unwrap();
        }
        let mut r = resolver(None, &tmp);
        let started = std::time::Instant::now();
        for i in 0..2000 {
            assert!(r.resolve(&format!("icon-{i}"), 48, 1).is_some());
        }
        for i in 0..500 {
            assert!(r.resolve(&format!("missing-{i}"), 48, 1).is_none());
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
