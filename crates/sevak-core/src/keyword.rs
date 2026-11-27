//! Keyword aliases: a configurable keyword may hold several alternatives
//! separated by `||` (`g||google`). Any of them selects the plugin; the first
//! is the primary one that hints and plugin ids show.

/// Separates the aliases of one keyword.
pub const ALIAS_SEPARATOR: &str = "||";

/// The aliases of `keyword`: trimmed, empty ones skipped.
pub fn aliases(keyword: &str) -> impl Iterator<Item = &str> {
    keyword
        .split(ALIAS_SEPARATOR)
        .map(str::trim)
        .filter(|alias| !alias.is_empty())
}

/// The first alias; `""` when there is none.
pub fn primary(keyword: &str) -> &str {
    aliases(keyword).next().unwrap_or("")
}

/// Whether `typed` is one of the aliases of `keyword` (ignoring case).
pub fn matches(keyword: &str, typed: &str) -> bool {
    let typed = typed.to_lowercase();
    aliases(keyword).any(|alias| alias.to_lowercase() == typed)
}

/// Checks `keyword` as the settings do: no empty alias (`a||`, `||`), no
/// whitespace inside an alias, no alias twice. An entirely empty keyword is
/// not checked (it means "off" or "the default"). Returns the lowercased
/// aliases.
pub fn check(keyword: &str) -> Result<Vec<String>, AliasProblem> {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen: Vec<String> = Vec::new();
    for raw in keyword.split(ALIAS_SEPARATOR) {
        let alias = raw.trim();
        if alias.is_empty() {
            return Err(AliasProblem::Empty);
        }
        if alias.chars().any(char::is_whitespace) {
            return Err(AliasProblem::Spaces(alias.to_owned()));
        }
        let lower = alias.to_lowercase();
        if seen.contains(&lower) {
            return Err(AliasProblem::Repeated(alias.to_owned()));
        }
        seen.push(lower);
    }
    Ok(seen)
}

/// Rewrites `keyword` with trimmed aliases joined by `||` (`" a || b "` to
/// `"a||b"`). A single keyword is only trimmed.
pub fn normalize(keyword: &str) -> String {
    if !keyword.contains(ALIAS_SEPARATOR) {
        return keyword.trim().to_owned();
    }
    keyword
        .split(ALIAS_SEPARATOR)
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(ALIAS_SEPARATOR)
}

/// What is wrong with a keyword that holds aliases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AliasProblem {
    /// An alias between two `||` (or at an end) is empty.
    Empty,
    /// The alias contains whitespace.
    Spaces(String),
    /// The alias is listed twice.
    Repeated(String),
}

impl AliasProblem {
    /// A sentence about `what` ("The files keyword").
    pub fn message(&self, what: &str) -> String {
        match self {
            Self::Empty => format!("{what} has an empty alias."),
            Self::Spaces(alias) => format!("{what} \"{alias}\" cannot contain spaces."),
            Self::Repeated(alias) => format!("{what} lists \"{alias}\" twice."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_trims_aliases() {
        assert_eq!(
            aliases(" g || google ").collect::<Vec<_>>(),
            ["g", "google"]
        );
        assert_eq!(primary("g||google"), "g");
        assert_eq!(primary(""), "");
        assert!(matches("g||Google", "GOOGLE"));
        assert!(!matches("g||google", "go"));
    }

    #[test]
    fn check_rejects_empty_spaced_and_repeated_aliases() {
        assert_eq!(check("g||google").unwrap(), ["g", "google"]);
        assert_eq!(check("").unwrap(), Vec::<String>::new());
        assert_eq!(check("a||"), Err(AliasProblem::Empty));
        assert_eq!(check("||"), Err(AliasProblem::Empty));
        assert_eq!(check("a||b c"), Err(AliasProblem::Spaces("b c".into())));
        assert_eq!(check("a||A"), Err(AliasProblem::Repeated("A".into())));
    }

    #[test]
    fn normalize_trims_around_the_separator() {
        assert_eq!(normalize(" a || b "), "a||b");
        assert_eq!(normalize(" a "), "a");
    }
}
