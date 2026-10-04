//! X11 window geometry that needs no X server: the work area left by panels
//! (EWMH struts), the invisible frame of a window and the `_NET_MOVERESIZE_WINDOW`
//! message. Pure functions, so they are tested on every OS.
//!
//! # Work areas
//!
//! X11 has no per-monitor work area. Panels reserve screen edges with
//! `_NET_WM_STRUT_PARTIAL` (or the older `_NET_WM_STRUT`) on their window, and
//! the window manager publishes a single `_NET_WORKAREA` rectangle per desktop.
//! [`work_area_from_struts`] derives the area of one monitor from the struts;
//! [`choose_work_area`] falls back to `_NET_WORKAREA` for desktops that publish
//! no struts (GNOME's top bar).

use sevak_core::window_layout::{Insets, Rect};

/// The space one window reserves along the screen edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Strut {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
    /// For each edge, the range along the edge that is reserved (inclusive
    /// start and end): `left_start_y`, `left_end_y`, `right_start_y`,
    /// `right_end_y`, `top_start_x`, `top_end_x`, `bottom_start_x`,
    /// `bottom_end_x`.
    pub ranges: [u32; 8],
}

impl Strut {
    /// From the twelve values of `_NET_WM_STRUT_PARTIAL`.
    pub fn from_partial(values: &[u32]) -> Option<Self> {
        let values: &[u32; 12] = values.get(..12)?.try_into().ok()?;
        Some(Self {
            left: values[0],
            right: values[1],
            top: values[2],
            bottom: values[3],
            ranges: [
                values[4], values[5], values[6], values[7], values[8], values[9], values[10],
                values[11],
            ],
        })
    }

    /// From the four values of `_NET_WM_STRUT`: the whole edge is reserved.
    pub fn from_basic(values: &[u32]) -> Option<Self> {
        let values: &[u32; 4] = values.get(..4)?.try_into().ok()?;
        Some(Self {
            left: values[0],
            right: values[1],
            top: values[2],
            bottom: values[3],
            ranges: [0, u32::MAX, 0, u32::MAX, 0, u32::MAX, 0, u32::MAX],
        })
    }

    pub fn is_empty(&self) -> bool {
        self.left == 0 && self.right == 0 && self.top == 0 && self.bottom == 0
    }
}

fn to_i32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// A rectangle from inclusive `start..=end` along one axis and a fixed span
/// on the other, or `None` when it has no area.
fn band(horizontal: bool, fixed_start: i64, fixed_len: i64, start: u32, end: u32) -> Option<Rect> {
    let start = i64::from(start);
    let len = i64::from(end) - start + 1;
    if fixed_len <= 0 || len <= 0 {
        return None;
    }
    let clamp =
        |v: i64| i32::try_from(v.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0);
    Some(if horizontal {
        Rect::new(
            clamp(start),
            clamp(fixed_start),
            clamp(len),
            clamp(fixed_len),
        )
    } else {
        Rect::new(
            clamp(fixed_start),
            clamp(start),
            clamp(fixed_len),
            clamp(len),
        )
    })
}

/// The part of `monitor` that panels leave free, given the struts of all
/// windows. `screen` is the whole X screen (the root window); struts are
/// measured from its edges, so a bottom strut only affects monitors that reach
/// down into that band.
pub fn work_area_from_struts(monitor: Rect, screen: Rect, struts: &[Strut]) -> Rect {
    let mut left = monitor.x;
    let mut top = monitor.y;
    let mut right = monitor.right();
    let mut bottom = monitor.bottom();
    for strut in struts {
        let [ls, le, rs, re, ts, te, bs, be] = strut.ranges;
        let screen_right = i64::from(screen.right());
        let screen_bottom = i64::from(screen.bottom());
        let reserved = [
            // (rectangle reserved on the screen, which monitor edge it moves)
            (
                band(false, i64::from(screen.x), i64::from(strut.left), ls, le),
                Edge::Left,
            ),
            (
                band(
                    false,
                    screen_right - i64::from(strut.right),
                    i64::from(strut.right),
                    rs,
                    re,
                ),
                Edge::Right,
            ),
            (
                band(true, i64::from(screen.y), i64::from(strut.top), ts, te),
                Edge::Top,
            ),
            (
                band(
                    true,
                    screen_bottom - i64::from(strut.bottom),
                    i64::from(strut.bottom),
                    bs,
                    be,
                ),
                Edge::Bottom,
            ),
        ];
        for (area, edge) in reserved {
            let Some(area) = area else { continue };
            if area.overlap_area(monitor) == 0 {
                continue;
            }
            match edge {
                Edge::Left => left = left.max(area.right()),
                Edge::Right => right = right.min(area.x),
                Edge::Top => top = top.max(area.bottom()),
                Edge::Bottom => bottom = bottom.min(area.y),
            }
        }
    }
    if right <= left || bottom <= top {
        // Struts that swallow the whole monitor are not trustworthy.
        return monitor;
    }
    Rect::new(left, top, right - left, bottom - top)
}

#[derive(Clone, Copy)]
enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// The work area to use for `monitor`: what the struts left (`from_struts`),
/// else the window manager's own `_NET_WORKAREA` where it covers a good part of
/// the monitor (desktops whose panels reserve no struts), else the whole
/// monitor.
pub fn choose_work_area(monitor: Rect, from_struts: Rect, net_workarea: Option<Rect>) -> Rect {
    if from_struts != monitor {
        return from_struts;
    }
    let Some(workarea) = net_workarea else {
        return monitor;
    };
    match monitor.intersection(workarea) {
        // Ignore a rectangle that has nothing to do with this monitor (some
        // window managers publish only the primary's).
        Some(inside) if inside.overlap_area(monitor) * 2 >= monitor.overlap_area(monitor) => inside,
        _ => monitor,
    }
}

/// The visible rectangle of a window from its client area, the decorations the
/// window manager draws around it (`_NET_FRAME_EXTENTS`) and the transparent
/// shadow a client-side-decorated GTK window includes in its own surface
/// (`_GTK_FRAME_EXTENTS`).
pub fn visible_rect(client: Rect, frame: Insets, gtk_shadow: Insets) -> Rect {
    client.expand(frame).shrink(gtk_shadow)
}

/// The client area to ask for so that the window is `visible` afterwards; the
/// inverse of [`visible_rect`].
pub fn client_rect_for(visible: Rect, frame: Insets, gtk_shadow: Insets) -> Rect {
    visible.expand(gtk_shadow).shrink(frame)
}

/// `_NET_MOVERESIZE_WINDOW` source indication "pager": window managers honour
/// requests from it even though the sender is not the window's client.
const SOURCE_PAGER: u32 = 2;
/// Gravity 10: the coordinates are those of the client area's top left, which
/// is how [`client_rect_for`] describes the target.
const STATIC_GRAVITY: u32 = 10;

/// The five data words of a `_NET_MOVERESIZE_WINDOW` client message that sets
/// x, y, width and height of `client` (see the EWMH specification).
pub fn moveresize_data(client: Rect) -> [u32; 5] {
    const X: u32 = 1 << 8;
    const Y: u32 = 1 << 9;
    const WIDTH: u32 = 1 << 10;
    const HEIGHT: u32 = 1 << 11;
    let flags = STATIC_GRAVITY | X | Y | WIDTH | HEIGHT | (SOURCE_PAGER << 12);
    [
        flags,
        client.x as u32,
        client.y as u32,
        client.width.max(1) as u32,
        client.height.max(1) as u32,
    ]
}

/// Whether the `_NET_WM_WINDOW_TYPE` atoms (by name) describe a window the user
/// would switch to: a normal window, a dialog, or one that says nothing.
pub fn is_switchable_type(type_names: &[&str]) -> bool {
    type_names.is_empty()
        || type_names.iter().any(|name| {
            matches!(
                *name,
                "_NET_WM_WINDOW_TYPE_NORMAL" | "_NET_WM_WINDOW_TYPE_DIALOG"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(x, y, w, h)
    }

    /// A bottom panel 40 pixels high across the whole width of a 1920 screen.
    fn bottom_panel() -> Strut {
        Strut::from_partial(&[0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, 1919]).unwrap()
    }

    #[test]
    fn struts_parse_from_the_property_values() {
        let partial = Strut::from_partial(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]).unwrap();
        assert_eq!(
            (partial.left, partial.right, partial.top, partial.bottom),
            (1, 2, 3, 4)
        );
        assert_eq!(partial.ranges, [5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(Strut::from_partial(&[0; 11]), None);
        let basic = Strut::from_basic(&[0, 0, 28, 0]).unwrap();
        assert_eq!(basic.top, 28);
        assert!(!basic.is_empty());
        assert!(Strut::from_basic(&[0, 0, 0, 0]).unwrap().is_empty());
        assert_eq!(Strut::from_basic(&[1, 2, 3]), None);
    }

    #[test]
    fn a_panel_takes_its_band_from_the_monitor() {
        let screen = r(0, 0, 1920, 1080);
        let work = work_area_from_struts(screen, screen, &[bottom_panel()]);
        assert_eq!(work, r(0, 0, 1920, 1040));
        // No struts, no change.
        assert_eq!(work_area_from_struts(screen, screen, &[]), screen);
        assert_eq!(
            work_area_from_struts(screen, screen, &[Strut::default()]),
            screen
        );
    }

    #[test]
    fn panels_on_every_edge_combine() {
        let screen = r(0, 0, 1920, 1080);
        let top = Strut::from_basic(&[0, 0, 28, 0]).unwrap();
        let left = Strut::from_basic(&[64, 0, 0, 0]).unwrap();
        let right = Strut::from_basic(&[0, 10, 0, 0]).unwrap();
        let work = work_area_from_struts(screen, screen, &[top, left, right, bottom_panel()]);
        assert_eq!(work, r(64, 28, 1920 - 64 - 10, 1080 - 28 - 40));
    }

    #[test]
    fn a_panel_on_one_monitor_does_not_touch_the_other() {
        // Two 1920x1080 monitors side by side; a bottom panel only under the
        // left one (its range stops at x = 1919).
        let screen = r(0, 0, 3840, 1080);
        let left_monitor = r(0, 0, 1920, 1080);
        let right_monitor = r(1920, 0, 1920, 1080);
        let struts = [bottom_panel()];
        assert_eq!(
            work_area_from_struts(left_monitor, screen, &struts),
            r(0, 0, 1920, 1040)
        );
        assert_eq!(
            work_area_from_struts(right_monitor, screen, &struts),
            right_monitor
        );
    }

    #[test]
    fn a_left_strut_only_reaches_the_monitor_at_the_screen_edge() {
        let screen = r(0, 0, 3840, 1080);
        let dock = Strut::from_basic(&[48, 0, 0, 0]).unwrap();
        assert_eq!(
            work_area_from_struts(r(0, 0, 1920, 1080), screen, &[dock]),
            r(48, 0, 1872, 1080)
        );
        assert_eq!(
            work_area_from_struts(r(1920, 0, 1920, 1080), screen, &[dock]),
            r(1920, 0, 1920, 1080)
        );
    }

    #[test]
    fn a_right_strut_and_monitors_of_different_heights() {
        // A 1920x1080 monitor and a 1280x1024 one below-left, as one screen.
        let screen = r(0, 0, 3200, 1080);
        let big = r(0, 0, 1920, 1080);
        let small = r(1920, 0, 1280, 1024);
        let panel = Strut::from_basic(&[0, 30, 0, 0]).unwrap();
        // The right strut is at the screen's right edge: only the small monitor.
        assert_eq!(work_area_from_struts(big, screen, &[panel]), big);
        assert_eq!(
            work_area_from_struts(small, screen, &[panel]),
            r(1920, 0, 1250, 1024)
        );
    }

    #[test]
    fn absurd_struts_are_ignored() {
        let screen = r(0, 0, 800, 600);
        let everything = Strut::from_basic(&[0, 0, 700, 700]).unwrap();
        assert_eq!(work_area_from_struts(screen, screen, &[everything]), screen);
        let huge = Strut::from_basic(&[u32::MAX, 0, 0, 0]).unwrap();
        assert_eq!(work_area_from_struts(screen, screen, &[huge]), screen);
    }

    #[test]
    fn the_net_workarea_fills_in_where_there_are_no_struts() {
        let monitor = r(0, 0, 1920, 1080);
        // Struts found something: they win.
        let from_struts = r(0, 0, 1920, 1040);
        assert_eq!(
            choose_work_area(monitor, from_struts, Some(r(0, 27, 1920, 1053))),
            from_struts
        );
        // GNOME: a top bar the struts do not know about.
        assert_eq!(
            choose_work_area(monitor, monitor, Some(r(0, 27, 3840, 1053))),
            r(0, 27, 1920, 1053)
        );
        // A rectangle that barely touches the monitor is not about this monitor.
        assert_eq!(
            choose_work_area(monitor, monitor, Some(r(1900, 0, 1920, 1080))),
            monitor
        );
        assert_eq!(choose_work_area(monitor, monitor, None), monitor);
        assert_eq!(
            choose_work_area(monitor, monitor, Some(r(5000, 0, 100, 100))),
            monitor
        );
    }

    #[test]
    fn frame_and_shadow_extents_convert_both_ways() {
        // A server-decorated window: 1 px border, 24 px title bar.
        let frame = Insets {
            left: 1,
            top: 24,
            right: 1,
            bottom: 1,
        };
        let client = r(101, 74, 798, 575);
        let visible = visible_rect(client, frame, Insets::ZERO);
        assert_eq!(visible, r(100, 50, 800, 600));
        assert_eq!(client_rect_for(visible, frame, Insets::ZERO), client);

        // A GTK client-side-decorated window with a 26 px transparent shadow.
        let shadow = Insets::uniform(26);
        let client = r(74, 24, 852, 652);
        let visible = visible_rect(client, Insets::ZERO, shadow);
        assert_eq!(visible, r(100, 50, 800, 600));
        assert_eq!(client_rect_for(visible, Insets::ZERO, shadow), client);

        for (frame, shadow) in [
            (Insets::ZERO, Insets::ZERO),
            (frame, Insets::ZERO),
            (Insets::ZERO, shadow),
            (frame, Insets::uniform(3)),
        ] {
            let target = r(10, 20, 640, 480);
            assert_eq!(
                visible_rect(client_rect_for(target, frame, shadow), frame, shadow),
                target
            );
        }
    }

    #[test]
    fn moveresize_data_sets_every_field_with_pager_source_and_static_gravity() {
        let data = moveresize_data(r(100, 50, 800, 600));
        assert_eq!(data[0] & 0xff, 10, "static gravity");
        assert_eq!(
            data[0] & (0xf << 8),
            0xf << 8,
            "x, y, width and height are set"
        );
        assert_eq!((data[0] >> 12) & 0x3, 2, "source indication: pager");
        assert_eq!(data[1..], [100, 50, 800, 600]);
        // Negative coordinates travel as two's complement 32-bit values.
        let negative = moveresize_data(r(-1280, -20, 640, 480));
        assert_eq!(negative[1] as i32, -1280);
        assert_eq!(negative[2] as i32, -20);
        // A degenerate size is lifted to 1 so the window manager accepts it.
        assert_eq!(moveresize_data(r(0, 0, 0, -3))[3..], [1, 1]);
    }

    #[test]
    fn only_normal_windows_and_dialogs_are_offered_for_switching() {
        assert!(is_switchable_type(&[]));
        assert!(is_switchable_type(&["_NET_WM_WINDOW_TYPE_NORMAL"]));
        assert!(is_switchable_type(&["_NET_WM_WINDOW_TYPE_DIALOG"]));
        assert!(is_switchable_type(&[
            "_NET_WM_WINDOW_TYPE_UTILITY",
            "_NET_WM_WINDOW_TYPE_NORMAL"
        ]));
        for hidden in [
            "_NET_WM_WINDOW_TYPE_DOCK",
            "_NET_WM_WINDOW_TYPE_DESKTOP",
            "_NET_WM_WINDOW_TYPE_SPLASH",
            "_NET_WM_WINDOW_TYPE_MENU",
            "_NET_WM_WINDOW_TYPE_TOOLTIP",
            "_NET_WM_WINDOW_TYPE_NOTIFICATION",
        ] {
            assert!(!is_switchable_type(&[hidden]), "{hidden}");
        }
    }
}
