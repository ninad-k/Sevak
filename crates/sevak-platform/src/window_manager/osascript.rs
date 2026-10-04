//! macOS window management through `osascript` (JavaScript for Automation and
//! System Events): the scripts, the parsers for what they print and the
//! translation of their failures into advice. Pure text handling, so the tests
//! run on every OS; only `macos::windows` runs the scripts.
//!
//! # Why a script
//!
//! Moving another app's window needs the Accessibility API. Calling it through
//! System Events keeps the unsafe Core Foundation plumbing out of Sevak, and
//! the same script can ask `NSScreen` for the displays and their visible
//! frames. The scripts take their input from `argv`, never from string
//! interpolation, so a window title can never become code.
//!
//! # Permissions
//!
//! System Events only answers when Sevak is allowed under System Settings,
//! Privacy & Security, Accessibility, and the first use also asks for permission
//! to control System Events (Automation). [`explain_failure`] turns the
//! errors those missing permissions cause into a sentence that says what to do.
//!
//! # Output format
//!
//! One record per line, fields separated by tabs (titles have tabs, line
//! breaks and runs of spaces replaced by single spaces):
//!
//! ```text
//! S  x  y  width  height  visible-x  visible-y  visible-width  visible-height  scale
//! W  pid  index  app  title  x  y  width  height  minimized
//! ERR  message
//! ```
//!
//! `S` lines come first, primary display first, in Cocoa's coordinates (origin
//! at the bottom left of the primary display); `W` lines are in the top-left
//! coordinates the Accessibility API uses.

use sevak_core::window_layout::{Monitor, Rect};

/// Added to coordinates passed to the scripts and taken off again there: an
/// argument that starts with a minus sign would be read by `osascript` as one of
/// its own options, and windows on a display left of or above the primary one
/// have negative coordinates.
pub const COORDINATE_OFFSET: i32 = 100_000;
/// Put in front of a window title passed to the scripts, for the same reason
/// (a title may start with `-`); the scripts cut it off.
pub const TITLE_PREFIX: &str = "t:";

/// Prints the displays and, for every visible app but Sevak itself, its
/// windows. `argv[0]` is Sevak's process id; `argv[1]` is `screens` (only the
/// displays), `all`, or the process id of the one app to list.
pub const LIST_SCRIPT: &str = r#"
function clean(value) {
  return String(value === null || value === undefined ? '' : value).replace(/\s+/g, ' ').trim();
}
function run(argv) {
  const lines = [];
  try {
    ObjC.import('AppKit');
    const screens = $.NSScreen.screens;
    for (let i = 0; i < screens.count; i++) {
      const screen = screens.objectAtIndex(i);
      const f = screen.frame;
      const v = screen.visibleFrame;
      lines.push(['S', f.origin.x, f.origin.y, f.size.width, f.size.height,
        v.origin.x, v.origin.y, v.size.width, v.size.height, screen.backingScaleFactor].join('\t'));
    }
    const own = parseInt(argv[0], 10);
    const mode = argv.length > 1 ? argv[1] : 'all';
    if (mode === 'screens') return lines.join('
');
    const only = mode === 'all' ? 0 : parseInt(mode, 10);
    const events = Application('System Events');
    const processes = events.applicationProcesses.whose({backgroundOnly: false})();
    for (const process of processes) {
      let pid, app, windows;
      try {
        pid = process.unixId();
        if (pid === own || (only > 0 && pid !== only)) continue;
        app = process.name();
        windows = process.windows();
      } catch (e) { continue; }
      for (let i = 0; i < windows.length; i++) {
        try {
          const window = windows[i];
          const position = window.position();
          const size = window.size();
          let minimized = 0;
          try { minimized = window.minimized() ? 1 : 0; } catch (e) {}
          lines.push(['W', pid, i, clean(app), clean(window.name()),
            position[0], position[1], size[0], size[1], minimized].join('\t'));
        } catch (e) {}
      }
    }
  } catch (e) {
    lines.push('ERR\t' + clean(e.message || e));
  }
  return lines.join('\n');
}
"#;

/// Moves and resizes one window. `argv`: process id, window index, expected
/// title with [`TITLE_PREFIX`] (empty after it to skip the check), then x, y
/// (both plus [`COORDINATE_OFFSET`]), width and height. When the window at
/// that index no longer has the expected title, the process's window with that
/// title is used instead. Prints `OK` or `ERR<TAB>message`.
pub const SET_RECT_SCRIPT: &str = r#"
function find(process, index, title) {
  const windows = process.windows();
  if (title === '') return windows[index];
  const clean = (value) => String(value === null || value === undefined ? '' : value).replace(/\s+/g, ' ').trim();
  if (index < windows.length && clean(windows[index].name()) === title) return windows[index];
  for (const window of windows) { if (clean(window.name()) === title) return window; }
  return windows[index];
}
function run(argv) {
  try {
    const events = Application('System Events');
    const process = events.applicationProcesses.whose({unixId: parseInt(argv[0], 10)})()[0];
    const window = find(process, parseInt(argv[1], 10), argv[2].slice(2));
    const x = parseInt(argv[3], 10) - 100000, y = parseInt(argv[4], 10) - 100000;
    const width = parseInt(argv[5], 10), height = parseInt(argv[6], 10);
    try { if (window.minimized()) window.minimized = false; } catch (e) {}
    try { window.attributes.byName('AXFullScreen').value = false; } catch (e) {}
    // Position, size, position: an app may clamp the size to the display the
    // window was on, so the position is set again once the size is final.
    window.position = [x, y];
    window.size = [width, height];
    window.position = [x, y];
    return 'OK';
  } catch (e) {
    return 'ERR\t' + String(e.message || e).replace(/\s+/g, ' ');
  }
}
"#;

/// Brings one window to the front. `argv`: process id, window index, expected
/// title with [`TITLE_PREFIX`] (empty after it to skip the check). Prints `OK`
/// or `ERR<TAB>message`.
pub const FOCUS_SCRIPT: &str = r#"
function run(argv) {
  try {
    const clean = (value) => String(value === null || value === undefined ? '' : value).replace(/\s+/g, ' ').trim();
    const events = Application('System Events');
    const process = events.applicationProcesses.whose({unixId: parseInt(argv[0], 10)})()[0];
    const windows = process.windows();
    const index = parseInt(argv[1], 10);
    const title = argv[2].slice(2);
    let window = windows[index];
    if (title !== '' && !(window && clean(window.name()) === title)) {
      for (const candidate of windows) { if (clean(candidate.name()) === title) { window = candidate; break; } }
    }
    if (!window) return 'ERR\tThe window is gone';
    try { if (window.minimized()) window.minimized = false; } catch (e) {}
    process.frontmost = true;
    try { window.actions.byName('AXRaise').perform(); } catch (e) {}
    return 'OK';
  } catch (e) {
    return 'ERR\t' + String(e.message || e).replace(/\s+/g, ' ');
  }
}
"#;

/// One window line of [`LIST_SCRIPT`]'s output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptWindow {
    pub pid: i32,
    pub index: usize,
    pub app: String,
    pub title: String,
    pub rect: Rect,
    pub minimized: bool,
}

/// One display line of [`LIST_SCRIPT`]'s output, in Cocoa coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptScreen {
    pub frame: [f64; 4],
    pub visible: [f64; 4],
    pub scale: f64,
}

/// Everything [`LIST_SCRIPT`] printed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScriptListing {
    pub screens: Vec<ScriptScreen>,
    pub windows: Vec<ScriptWindow>,
}

/// Parses what [`LIST_SCRIPT`] printed. A script-side error (`ERR` line) comes
/// back as the explained failure; malformed lines are skipped.
pub fn parse_listing(output: &str) -> Result<ScriptListing, String> {
    let mut listing = ScriptListing::default();
    for line in output.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["ERR", message, ..] => return Err(explain_failure(message)),
            ["S", rest @ ..] if rest.len() == 9 => {
                let numbers: Option<Vec<f64>> = rest.iter().map(|f| f.parse().ok()).collect();
                if let Some(n) = numbers {
                    listing.screens.push(ScriptScreen {
                        frame: [n[0], n[1], n[2], n[3]],
                        visible: [n[4], n[5], n[6], n[7]],
                        scale: n[8],
                    });
                }
            }
            ["W", pid, index, app, title, x, y, width, height, minimized] => {
                let number = |text: &str| text.parse::<f64>().ok().map(|v| v.round() as i32);
                let (Ok(pid), Ok(index)) = (pid.parse::<i32>(), index.parse::<usize>()) else {
                    continue;
                };
                let (Some(x), Some(y), Some(w), Some(h)) =
                    (number(x), number(y), number(width), number(height))
                else {
                    continue;
                };
                listing.windows.push(ScriptWindow {
                    pid,
                    index,
                    app: (*app).to_owned(),
                    title: (*title).to_owned(),
                    rect: Rect::new(x, y, w, h),
                    minimized: *minimized == "1",
                });
            }
            _ => {}
        }
    }
    Ok(listing)
}

/// Parses the `OK` / `ERR<TAB>message` answer of the set and focus scripts.
pub fn parse_outcome(output: &str) -> Result<(), String> {
    // Only line breaks are cut: an empty message leaves "ERR<TAB>" intact.
    let output = output.trim_start().trim_end_matches(['\r', '\n']);
    if output.trim() == "OK" {
        return Ok(());
    }
    match output.strip_prefix("ERR\t") {
        Some(message) => Err(explain_failure(message)),
        None => Err(format!("Unexpected answer from the system: {output}")),
    }
}

/// The monitors for [`ScriptListing::screens`]: Cocoa's rectangles (origin at
/// the bottom left of the primary display, which is the first screen) turned
/// into the top-left coordinates windows use. Points, not pixels: the
/// Accessibility API and `NSScreen` both work in points, so no scaling is
/// needed here and `scale_percent` stays 100 (the gap is in points too).
pub fn monitors_from_screens(screens: &[ScriptScreen]) -> Vec<Monitor> {
    let Some(primary) = screens.first() else {
        return Vec::new();
    };
    let primary_height = primary.frame[3];
    let to_rect = |cocoa: [f64; 4]| {
        Rect::new(
            cocoa[0].round() as i32,
            (primary_height - (cocoa[1] + cocoa[3])).round() as i32,
            cocoa[2].round() as i32,
            cocoa[3].round() as i32,
        )
    };
    screens
        .iter()
        .enumerate()
        .map(|(index, screen)| Monitor {
            name: format!("Display {}", index + 1),
            bounds: to_rect(screen.frame),
            work_area: to_rect(screen.visible),
            scale_percent: 100,
            primary: index == 0,
        })
        .collect()
}

/// The sentence shown for an error a script reported: the permission ones say
/// where to grant it, the rest keep the system's own words.
pub fn explain_failure(message: &str) -> String {
    let lower = message.to_lowercase();
    if lower.contains("-1719")
        || lower.contains("-25211")
        || lower.contains("assistive access")
        || lower.contains("not allowed assistive")
    {
        ACCESSIBILITY_HELP.to_owned()
    } else if lower.contains("-1743") || lower.contains("not authorized to send apple events") {
        AUTOMATION_HELP.to_owned()
    } else if message.trim().is_empty() {
        "The system did not say why".to_owned()
    } else {
        message.trim().to_owned()
    }
}

/// Shown when the Accessibility permission is missing.
pub const ACCESSIBILITY_HELP: &str = "Allow Sevak in System Settings > Privacy & Security > Accessibility to move and switch windows, then try again";
/// Shown when controlling System Events was refused.
pub const AUTOMATION_HELP: &str = "Allow Sevak to control System Events in System Settings > Privacy & Security > Automation, then try again";

/// Words for the window id the macOS backend uses: the process id and the
/// index of the window in that process's window list.
pub fn format_id(pid: i32, index: usize) -> String {
    format!("{pid}:{index}")
}

/// The inverse of [`format_id`].
pub fn parse_id(id: &str) -> Option<(i32, usize)> {
    let (pid, index) = id.split_once(':')?;
    let pid = pid.parse::<i32>().ok().filter(|pid| *pid > 0)?;
    Some((pid, index.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_is_parsed_into_screens_and_windows() {
        let output = "S\t0\t0\t1728\t1117\t0\t0\t1728\t1079\t2\n\
                      S\t-1920\t-200\t1920\t1080\t-1920\t-200\t1920\t1055\t1\n\
                      W\t501\t0\tSafari\tApple - Start\t120\t80\t1000.4\t700\t0\n\
                      W\t501\t1\tSafari\t\t10\t10\t300\t200\t1\n\
                      W\t77\t0\tCode\ta.rs - sevak\t-1900.0\t-150\t800\t600\t0\n";
        let listing = parse_listing(output).unwrap();
        assert_eq!(listing.screens.len(), 2);
        assert_eq!(listing.screens[0].visible, [0.0, 0.0, 1728.0, 1079.0]);
        assert_eq!(listing.screens[0].scale, 2.0);
        assert_eq!(listing.windows.len(), 3);
        assert_eq!(
            listing.windows[0],
            ScriptWindow {
                pid: 501,
                index: 0,
                app: "Safari".into(),
                title: "Apple - Start".into(),
                rect: Rect::new(120, 80, 1000, 700),
                minimized: false,
            }
        );
        assert!(listing.windows[1].minimized);
        assert_eq!(listing.windows[1].title, "");
        assert_eq!(listing.windows[2].rect, Rect::new(-1900, -150, 800, 600));
    }

    #[test]
    fn malformed_lines_are_skipped_and_empty_output_is_an_empty_listing() {
        assert_eq!(parse_listing("").unwrap(), ScriptListing::default());
        let listing = parse_listing(
            "W\tx\t0\tA\tB\t1\t2\t3\t4\t0\n\
             W\t5\t0\tA\tB\tone\t2\t3\t4\t0\n\
             W\t5\t0\tA\tB\t1\t2\t3\n\
             S\t1\t2\n\
             noise\n\
             W\t5\t0\tA\tB\t1\t2\t3\t4\t0\n",
        )
        .unwrap();
        assert_eq!(listing.windows.len(), 1);
        assert!(listing.screens.is_empty());
    }

    #[test]
    fn script_errors_are_explained() {
        let err = parse_listing(
            "ERR\tSystem Events got an error: osascript is not allowed assistive access. (-1719)",
        )
        .unwrap_err();
        assert_eq!(err, ACCESSIBILITY_HELP);
        let err =
            parse_outcome("ERR\tNot authorized to send Apple events to System Events. (-1743)")
                .unwrap_err();
        assert_eq!(err, AUTOMATION_HELP);
        assert_eq!(
            parse_outcome("ERR\tCan't get window 3").unwrap_err(),
            "Can't get window 3"
        );
        assert_eq!(
            parse_outcome("ERR\t").unwrap_err(),
            "The system did not say why"
        );
        assert!(parse_outcome("OK\n").is_ok());
        assert!(parse_outcome("").unwrap_err().contains("Unexpected"));
        for help in [ACCESSIBILITY_HELP, AUTOMATION_HELP] {
            assert!(help.contains("System Settings"), "{help}");
        }
    }

    #[test]
    fn cocoa_rectangles_become_top_left_coordinates() {
        let screens = parse_listing(
            "S\t0\t0\t1728\t1117\t0\t0\t1728\t1079\t2\n\
             S\t1728\t-200\t2560\t1440\t1728\t-200\t2560\t1415\t1\n\
             S\t-1920\t300\t1920\t1080\t-1920\t300\t1920\t1055\t1\n",
        )
        .unwrap()
        .screens;
        let monitors = monitors_from_screens(&screens);
        assert_eq!(monitors.len(), 3);
        // The primary: the menu bar (38 pt) takes the top, so the visible frame
        // starts 38 below the top edge.
        assert!(monitors[0].primary);
        assert_eq!(monitors[0].bounds, Rect::new(0, 0, 1728, 1117));
        assert_eq!(monitors[0].work_area, Rect::new(0, 38, 1728, 1079));
        // A display whose bottom edge is 200 below the primary's bottom, on
        // the right: y flips to 1117 - (-200 + 1440) = -123.
        assert_eq!(monitors[1].bounds, Rect::new(1728, -123, 2560, 1440));
        assert_eq!(monitors[1].work_area, Rect::new(1728, -98, 2560, 1415));
        // A display above-left: its top edge is at 1117 - (300 + 1080) = -263.
        assert_eq!(monitors[2].bounds, Rect::new(-1920, -263, 1920, 1080));
        assert!(!monitors[2].primary);
        assert!(monitors_from_screens(&[]).is_empty());
    }

    #[test]
    fn ids_name_the_process_and_the_window_index() {
        assert_eq!(format_id(501, 3), "501:3");
        assert_eq!(parse_id("501:3"), Some((501, 3)));
        for bad in [
            "", "501", "501:", ":3", "0:1", "-5:1", "a:1", "5:b", "5:1:2", "5 :1",
        ] {
            assert_eq!(parse_id(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn script_constants_agree_with_the_scripts() {
        assert_eq!(TITLE_PREFIX.len(), 2, "the scripts cut two characters");
        assert!(SET_RECT_SCRIPT.contains(&format!("- {COORDINATE_OFFSET}")));
        assert!(SET_RECT_SCRIPT.contains("argv[2].slice(2)"));
        assert!(FOCUS_SCRIPT.contains("argv[2].slice(2)"));
        assert!(LIST_SCRIPT.contains("mode === 'screens'"));
    }

    #[test]
    fn the_scripts_read_their_input_from_argv_only() {
        for script in [LIST_SCRIPT, SET_RECT_SCRIPT, FOCUS_SCRIPT] {
            assert!(script.contains("function run(argv)"));
            // No evaluation of strings and no shell access from the scripts.
            for forbidden in ["eval(", "doShellScript", "Function(", "NSTask"] {
                assert!(!script.contains(forbidden), "{forbidden}");
            }
        }
    }
}
