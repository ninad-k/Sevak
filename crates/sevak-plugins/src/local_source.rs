//! Reading files that other programs write (ssh's config, editors' recent
//! lists, calendar exports) without trusting them.
//!
//! - **Confined**: a file is read only if its real path (symbolic links
//!   resolved) is inside the folder it is expected in, so a link in
//!   `~/.ssh` cannot make Sevak read a file elsewhere.
//! - **Bounded**: every read goes through [`sevak_core::bounded_read`].
//! - **Lossy text**: a stray byte that is not UTF-8 costs one character, not
//!   the whole file.

use std::path::{Path, PathBuf};

use sevak_core::bounded_read::read_capped;

/// The real path of `path` if it is inside the real path of `root` (or is
/// `root` itself). `None` if either does not exist or `path` leads elsewhere.
pub fn confined(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let real = path.canonicalize().ok()?;
    real.starts_with(&root).then_some(real)
}

/// The text of `path`, read only if it is a file inside `root` and at most
/// `cap` bytes long.
pub fn read_confined_text(root: &Path, path: &Path, cap: u64) -> Option<String> {
    let real = confined(root, path)?;
    if !real.is_file() {
        return None;
    }
    match read_capped(&real, cap) {
        Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Err(err) => {
            tracing::debug!(%err, "skipped a file that could not be read");
            None
        }
    }
}

/// Whether `name` matches the shell-style `pattern` (`*` is any run of
/// characters, `?` one character; nothing else is special).
pub fn wildcard_match(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((star_p, star_n)) = star {
            p = star_p + 1;
            n = star_n + 1;
            star = Some((star_p, star_n + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn wildcards_match_like_a_shell() {
        assert!(wildcard_match("*", "config"));
        assert!(wildcard_match("*.conf", "work.conf"));
        assert!(!wildcard_match("*.conf", "work.conf.bak"));
        assert!(wildcard_match("w?rk*", "work.conf"));
        assert!(wildcard_match("a*b*c", "axxbyyc"));
        assert!(!wildcard_match("a*b*c", "axxbyy"));
        assert!(wildcard_match("", ""));
        assert!(!wildcard_match("", "x"));
    }

    #[test]
    fn reads_inside_the_root_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("in.txt"), "inside").unwrap();
        fs::write(dir.path().join("out.txt"), "outside").unwrap();

        assert_eq!(
            read_confined_text(&root, &root.join("in.txt"), 100).as_deref(),
            Some("inside")
        );
        // `..` resolves outside the root.
        assert_eq!(
            read_confined_text(&root, &root.join("..").join("out.txt"), 100),
            None
        );
        assert_eq!(read_confined_text(&root, &root.join("missing"), 100), None);
        // A folder is not a file; a file over the cap is not read.
        assert_eq!(read_confined_text(&root, &root, 100), None);
        assert_eq!(read_confined_text(&root, &root.join("in.txt"), 3), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_root_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::write(dir.path().join("secret"), "secret").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret"), root.join("link")).unwrap();
        assert_eq!(read_confined_text(&root, &root.join("link"), 100), None);
        fs::write(root.join("real"), "ok").unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("inner")).unwrap();
        assert_eq!(
            read_confined_text(&root, &root.join("inner"), 100).as_deref(),
            Some("ok")
        );
    }
}
