//! Names that are fine on one file system and a trap on another.

/// Whether Windows treats `name` as a device rather than a file: `CON`, `PRN`,
/// `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9` (also with the superscript digits
/// `¹ ² ³`), `CONIN$`, `CONOUT$` and `CLOCK$`, in any case, with or without an
/// extension (`nul.txt`), and with trailing dots or spaces (`con .`), all of
/// which Windows ignores.
///
/// Used for names that come from somewhere else (a package's file names, a
/// gallery id, a theme name) so that the same package installs the same way
/// everywhere and cannot name a device on Windows.
pub fn is_reserved_device_name(name: &str) -> bool {
    // Windows looks at the part before the first dot (or colon, a stream name).
    let base = name.split(['.', ':']).next().unwrap_or_default().trim();
    let upper = base.to_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
    ) {
        return true;
    }
    ["COM", "LPT"].iter().any(|prefix| {
        upper.strip_prefix(prefix).is_some_and(|rest| {
            let mut chars = rest.chars();
            matches!(
                (chars.next(), chars.next()),
                (Some('1'..='9' | '¹' | '²' | '³'), None)
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_are_recognised_in_every_disguise() {
        for name in [
            "CON",
            "con",
            "Prn",
            "AUX",
            "nul",
            "COM1",
            "com9",
            "LPT1",
            "lpt9",
            "COM¹",
            "lpt²",
            "NUL.txt",
            "con.tar.gz",
            "aux.",
            "con ",
            "con . ",
            "com1:stream",
            "CONIN$",
            "CONOUT$",
        ] {
            assert!(is_reserved_device_name(name), "{name:?}");
        }
    }

    #[test]
    fn ordinary_names_are_not() {
        for name in [
            "",
            "a",
            "console",
            "connect",
            "nulls",
            "com",
            "com0",
            "com10",
            "lpt0",
            "lpt",
            "aux1",
            "my.con",
            "readme.txt",
            ".con",
            "con-x",
            "xcon",
        ] {
            assert!(!is_reserved_device_name(name), "{name:?}");
        }
    }
}
