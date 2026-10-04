//! Window management geometry: where a window goes for a layout name.
//!
//! Everything here is pure arithmetic on rectangles, so it is identical on
//! every OS and fully unit tested. The platform layer reads windows and
//! monitors from the OS, hands the numbers to [`plan`] and applies the
//! rectangle that comes back.
//!
//! # Coordinates
//!
//! All rectangles are in physical pixels of the virtual desktop, with the
//! origin at the top left and `y` growing downwards (macOS and X11 already
//! work that way; the macOS backend converts Cocoa's bottom-left origin).
//! Monitors may have negative coordinates (a monitor left of or above the
//! primary one). A monitor's *work area* is its bounds minus the taskbar,
//! dock and menu bar.
//!
//! # Gaps
//!
//! The configured gap is in logical pixels; [`scale_gap`] turns it into
//! physical pixels for a monitor's scale factor. A gap is kept between a
//! window and the edge of the work area and, between two tiles, between the
//! tiles, so the space around and between windows looks the same.
//!
//! # Invisible borders
//!
//! Windows 10 and 11 give top-level windows an invisible resize border, and
//! GTK client-side decorations draw a transparent shadow. Layouts are
//! computed for the *visible* rectangle; [`frame_insets`] measures the
//! difference to the rectangle the OS positions and [`Rect::expand`] adds it
//! back.

use std::fmt;

/// An axis-aligned rectangle in physical pixels (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// How much to add to (or take from) each side of a [`Rect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Insets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Insets {
    pub const ZERO: Self = Self {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    pub const fn uniform(amount: i32) -> Self {
        Self {
            left: amount,
            top: amount,
            right: amount,
            bottom: amount,
        }
    }

    pub fn is_zero(self) -> bool {
        self == Self::ZERO
    }
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The first column right of the rectangle.
    pub fn right(self) -> i32 {
        self.x.saturating_add(self.width)
    }

    /// The first row below the rectangle.
    pub fn bottom(self) -> i32 {
        self.y.saturating_add(self.height)
    }

    /// No area (zero or negative size).
    pub fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// The center, rounded towards the top left.
    pub fn center(self) -> (i32, i32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }

    pub fn contains_point(self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Whether `other` lies completely inside this rectangle.
    pub fn contains(self, other: Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    /// The area both rectangles cover, in square pixels (0 when apart).
    pub fn overlap_area(self, other: Self) -> i64 {
        let width = i64::from(self.right().min(other.right())) - i64::from(self.x.max(other.x));
        let height = i64::from(self.bottom().min(other.bottom())) - i64::from(self.y.max(other.y));
        if width <= 0 || height <= 0 {
            0
        } else {
            width * height
        }
    }

    /// The part both rectangles cover; `None` when they only touch or are apart.
    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| Self::new(x, y, right - x, bottom - y))
    }

    /// Grows the rectangle by `insets` on every side.
    #[must_use]
    pub fn expand(self, insets: Insets) -> Self {
        Self::new(
            self.x - insets.left,
            self.y - insets.top,
            self.width + insets.left + insets.right,
            self.height + insets.top + insets.bottom,
        )
    }

    /// Shrinks the rectangle by `insets` on every side. The size never goes
    /// below zero.
    #[must_use]
    pub fn shrink(self, insets: Insets) -> Self {
        Self::new(
            self.x + insets.left,
            self.y + insets.top,
            (self.width - insets.left - insets.right).max(0),
            (self.height - insets.top - insets.bottom).max(0),
        )
    }

    /// Moves the rectangle (never resizes it) so that as much of it as fits
    /// lies inside `bounds`. A rectangle larger than `bounds` is aligned to
    /// the top left.
    #[must_use]
    pub fn clamp_into(self, bounds: Self) -> Self {
        let max_x = (bounds.right() - self.width).max(bounds.x);
        let max_y = (bounds.bottom() - self.height).max(bounds.y);
        Self::new(
            self.x.clamp(bounds.x, max_x),
            self.y.clamp(bounds.y, max_y),
            self.width,
            self.height,
        )
    }

    /// Whether every side is within `tolerance` pixels of `other`'s (apps
    /// round sizes to their own grids, so positions are rarely exact).
    pub fn approx_eq(self, other: Self, tolerance: i32) -> bool {
        (self.x - other.x).abs() <= tolerance
            && (self.y - other.y).abs() <= tolerance
            && (self.width - other.width).abs() <= tolerance
            && (self.height - other.height).abs() <= tolerance
    }
}

/// The invisible border of a window: how far the rectangle the OS positions
/// (`outer`) reaches beyond the one the user sees (`visible`) on each side.
/// A side where `outer` is inside `visible` counts as 0.
pub fn frame_insets(outer: Rect, visible: Rect) -> Insets {
    Insets {
        left: (visible.x - outer.x).max(0),
        top: (visible.y - outer.y).max(0),
        right: (outer.right() - visible.right()).max(0),
        bottom: (outer.bottom() - visible.bottom()).max(0),
    }
}

/// One display, as the platform layer reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    /// What the OS calls the display (`\\.\DISPLAY1`, `DP-1`, `Built-in`).
    pub name: String,
    /// The whole display.
    pub bounds: Rect,
    /// The part not covered by the taskbar, dock, menu bar or panels.
    pub work_area: Rect,
    /// Scale in percent: 100 for no scaling, 150 for 150 %.
    pub scale_percent: u32,
    pub primary: bool,
}

impl Monitor {
    pub fn new(name: &str, bounds: Rect, work_area: Rect) -> Self {
        Self {
            name: name.to_owned(),
            bounds,
            work_area,
            scale_percent: 100,
            primary: false,
        }
    }
}

/// Largest gap the settings accept, in logical pixels.
pub const MAX_GAP: i32 = 200;
/// What fraction of the work area "almost maximize" fills, in percent.
pub const ALMOST_MAXIMIZE_PERCENT: i32 = 90;
/// Differences up to this many pixels still count as "the window did not move".
pub const POSITION_TOLERANCE: i32 = 2;

/// Where in the work area a window goes. Each variant maps to a fixed
/// rectangle, so the platform layer never receives anything but a
/// rectangle computed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layout {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    LeftThird,
    CenterThird,
    RightThird,
    LeftTwoThirds,
    RightTwoThirds,
    Maximize,
    AlmostMaximize,
    /// Keeps the window's size and centers it.
    Center,
}

impl Layout {
    pub const ALL: [Self; 16] = [
        Self::LeftHalf,
        Self::RightHalf,
        Self::TopHalf,
        Self::BottomHalf,
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
        Self::LeftThird,
        Self::CenterThird,
        Self::RightThird,
        Self::LeftTwoThirds,
        Self::RightTwoThirds,
        Self::Maximize,
        Self::AlmostMaximize,
        Self::Center,
    ];

    /// Stable spelling used in result ids and payloads.
    pub fn key(self) -> &'static str {
        match self {
            Self::LeftHalf => "left",
            Self::RightHalf => "right",
            Self::TopHalf => "top",
            Self::BottomHalf => "bottom",
            Self::TopLeft => "top_left",
            Self::TopRight => "top_right",
            Self::BottomLeft => "bottom_left",
            Self::BottomRight => "bottom_right",
            Self::LeftThird => "left_third",
            Self::CenterThird => "center_third",
            Self::RightThird => "right_third",
            Self::LeftTwoThirds => "left_two_thirds",
            Self::RightTwoThirds => "right_two_thirds",
            Self::Maximize => "maximize",
            Self::AlmostMaximize => "almost_maximize",
            Self::Center => "center",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layout| layout.key() == key)
    }

    /// The columns and rows of the grid this layout lives on, and the cells it
    /// spans as `(columns, rows, first column, last column + 1, first row,
    /// last row + 1)`. `None` for layouts that are not grid cells.
    fn grid(self) -> Option<(i32, i32, i32, i32, i32, i32)> {
        Some(match self {
            Self::LeftHalf => (2, 1, 0, 1, 0, 1),
            Self::RightHalf => (2, 1, 1, 2, 0, 1),
            Self::TopHalf => (1, 2, 0, 1, 0, 1),
            Self::BottomHalf => (1, 2, 0, 1, 1, 2),
            Self::TopLeft => (2, 2, 0, 1, 0, 1),
            Self::TopRight => (2, 2, 1, 2, 0, 1),
            Self::BottomLeft => (2, 2, 0, 1, 1, 2),
            Self::BottomRight => (2, 2, 1, 2, 1, 2),
            Self::LeftThird => (3, 1, 0, 1, 0, 1),
            Self::CenterThird => (3, 1, 1, 2, 0, 1),
            Self::RightThird => (3, 1, 2, 3, 0, 1),
            Self::LeftTwoThirds => (3, 1, 0, 2, 0, 1),
            Self::RightTwoThirds => (3, 1, 1, 3, 0, 1),
            Self::Maximize | Self::AlmostMaximize | Self::Center => return None,
        })
    }

    /// The visible rectangle for a window currently at `current`, on a monitor
    /// whose work area is `work`, with `gap` physical pixels around and
    /// between windows.
    pub fn target(self, work: Rect, current: Rect, gap: i32) -> Rect {
        let gap = effective_gap(work, gap);
        let area = work.shrink(Insets::uniform(gap));
        match self {
            Self::Maximize => area,
            Self::AlmostMaximize => {
                let width = area.width * ALMOST_MAXIMIZE_PERCENT / 100;
                let height = area.height * ALMOST_MAXIMIZE_PERCENT / 100;
                centered(area, width, height)
            }
            Self::Center => {
                let width = current.width.clamp(1, area.width.max(1));
                let height = current.height.clamp(1, area.height.max(1));
                centered(area, width, height)
            }
            _ => {
                let (columns, rows, left, right, top, bottom) =
                    self.grid().expect("the remaining layouts are grid cells");
                let (x, width) = cells(area.x, area.width, columns, left, right, gap);
                let (y, height) = cells(area.y, area.height, rows, top, bottom, gap);
                Rect::new(x, y, width, height)
            }
        }
    }
}

/// The gap that is safe for `work`: never so large that tiles vanish.
fn effective_gap(work: Rect, gap: i32) -> i32 {
    let limit = (work.width.min(work.height) / 8).max(0);
    gap.clamp(0, limit)
}

/// A `width` x `height` rectangle in the middle of `area`.
fn centered(area: Rect, width: i32, height: i32) -> Rect {
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// Origin and length of the cells `first..last` of `count` equal cells that
/// divide `length` pixels from `origin`, with `gap` pixels between cells.
/// Cells are computed from their edges, so neighbours share an edge exactly
/// (a rounding remainder never opens a hole or an overlap).
fn cells(origin: i32, length: i32, count: i32, first: i32, last: i32, gap: i32) -> (i32, i32) {
    let step = i64::from(length) + i64::from(gap);
    let edge = |index: i32| i64::from(origin) + i64::from(index) * step / i64::from(count);
    let start = edge(first);
    let end = edge(last) - i64::from(gap);
    (start as i32, (end - start).max(0) as i32)
}

/// The gap in physical pixels for a monitor at `scale_percent` when the user
/// configured `logical` pixels.
pub fn scale_gap(logical: i32, scale_percent: u32) -> i32 {
    let logical = logical.clamp(0, MAX_GAP);
    let scaled = (i64::from(logical) * i64::from(scale_percent.max(1)) + 50) / 100;
    i32::try_from(scaled).unwrap_or(MAX_GAP)
}

/// What to do with the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowCommand {
    Layout(Layout),
    /// Back to where the window was before Sevak first moved it.
    Restore,
    /// To the display right of (or below) this one, wrapping around.
    NextDisplay,
    /// To the display left of (or above) this one, wrapping around.
    PreviousDisplay,
}

impl WindowCommand {
    /// Every command, in the order the cheat sheet lists them.
    pub fn all() -> Vec<Self> {
        Layout::ALL
            .into_iter()
            .map(Self::Layout)
            .chain([Self::Restore, Self::NextDisplay, Self::PreviousDisplay])
            .collect()
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Layout(layout) => layout.key(),
            Self::Restore => "restore",
            Self::NextDisplay => "next_display",
            Self::PreviousDisplay => "previous_display",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "restore" => Some(Self::Restore),
            "next_display" => Some(Self::NextDisplay),
            "previous_display" => Some(Self::PreviousDisplay),
            _ => Layout::from_key(key).map(Self::Layout),
        }
    }
}

/// Why no rectangle could be planned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The system reported no display.
    NoMonitors,
    /// Moving between displays needs at least two.
    SingleDisplay,
    /// Sevak has not moved this window yet, so there is nothing to go back to.
    NothingToRestore,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoMonitors => "No display was found",
            Self::SingleDisplay => "There is only one display",
            Self::NothingToRestore => {
                "Sevak has not moved this window yet, so there is nothing to restore"
            }
        })
    }
}

impl std::error::Error for PlanError {}

/// The index of the monitor `window` is on: the one it overlaps most, else the
/// one whose center is nearest to the window's center.
pub fn monitor_index_for(window: Rect, monitors: &[Monitor]) -> Option<usize> {
    let by_overlap = monitors
        .iter()
        .enumerate()
        .map(|(index, monitor)| (index, window.overlap_area(monitor.bounds)))
        .filter(|(_, area)| *area > 0)
        .max_by_key(|(index, area)| (*area, std::cmp::Reverse(*index)));
    if let Some((index, _)) = by_overlap {
        return Some(index);
    }
    let (cx, cy) = window.center();
    monitors
        .iter()
        .enumerate()
        .min_by_key(|(index, monitor)| {
            let (mx, my) = monitor.bounds.center();
            let dx = i64::from(mx) - i64::from(cx);
            let dy = i64::from(my) - i64::from(cy);
            (dx * dx + dy * dy, *index)
        })
        .map(|(index, _)| index)
}

/// Monitor indices ordered left to right, then top to bottom: the order
/// "next display" walks through.
pub fn display_order(monitors: &[Monitor]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..monitors.len()).collect();
    order.sort_by_key(|index| {
        let bounds = monitors[*index].bounds;
        (bounds.x, bounds.y, *index)
    });
    order
}

/// The monitor after (`forward`) or before the one at `from` in
/// [`display_order`], wrapping around. `None` for an unknown `from`.
pub fn neighbour_display(monitors: &[Monitor], from: usize, forward: bool) -> Option<usize> {
    let order = display_order(monitors);
    let position = order.iter().position(|index| *index == from)?;
    let count = order.len();
    let next = if forward {
        (position + 1) % count
    } else {
        (position + count - 1) % count
    };
    Some(order[next])
}

/// Carries a window to another work area, keeping its relative position and
/// size (a window filling the left half stays the left half, a maximized one
/// stays maximized), and never leaving it larger than the target.
pub fn move_to_display(window: Rect, from: Rect, to: Rect) -> Rect {
    if from.is_empty() || to.is_empty() {
        return window;
    }
    let scale = |value: i32, source: i32, target: i32| -> i32 {
        i32::try_from(i64::from(value) * i64::from(target) / i64::from(source)).unwrap_or(target)
    };
    let width = scale(window.width, from.width, to.width).clamp(1, to.width);
    let height = scale(window.height, from.height, to.height).clamp(1, to.height);
    let x = to.x + scale(window.x - from.x, from.width, to.width);
    let y = to.y + scale(window.y - from.y, from.height, to.height);
    Rect::new(x, y, width, height).clamp_into(to)
}

/// The visible rectangle `command` puts a window in.
///
/// `window` is where the window is now, `gap` the configured gap in logical
/// pixels, and `original` where it was before Sevak first moved it (for
/// [`WindowCommand::Restore`]).
pub fn plan(
    command: WindowCommand,
    window: Rect,
    monitors: &[Monitor],
    gap: i32,
    original: Option<Rect>,
) -> Result<Rect, PlanError> {
    let index = monitor_index_for(window, monitors).ok_or(PlanError::NoMonitors)?;
    let monitor = &monitors[index];
    match command {
        WindowCommand::Layout(layout) => Ok(layout.target(
            monitor.work_area,
            window,
            scale_gap(gap, monitor.scale_percent),
        )),
        WindowCommand::Restore => original.ok_or(PlanError::NothingToRestore),
        WindowCommand::NextDisplay | WindowCommand::PreviousDisplay => {
            let forward = command == WindowCommand::NextDisplay;
            let destination = neighbour_display(monitors, index, forward)
                .filter(|other| *other != index)
                .ok_or(PlanError::SingleDisplay)?;
            Ok(move_to_display(
                window,
                monitor.work_area,
                monitors[destination].work_area,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect::new(0, 0, 1920, 1040);

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(x, y, w, h)
    }

    fn target(layout: Layout, work: Rect, gap: i32) -> Rect {
        layout.target(work, r(100, 100, 800, 600), gap)
    }

    #[test]
    fn layouts_without_a_gap_fill_the_work_area_exactly() {
        // 1920x1040 with the taskbar at the bottom of a 1080 display.
        let cases = [
            (Layout::LeftHalf, r(0, 0, 960, 1040)),
            (Layout::RightHalf, r(960, 0, 960, 1040)),
            (Layout::TopHalf, r(0, 0, 1920, 520)),
            (Layout::BottomHalf, r(0, 520, 1920, 520)),
            (Layout::TopLeft, r(0, 0, 960, 520)),
            (Layout::TopRight, r(960, 0, 960, 520)),
            (Layout::BottomLeft, r(0, 520, 960, 520)),
            (Layout::BottomRight, r(960, 520, 960, 520)),
            (Layout::LeftThird, r(0, 0, 640, 1040)),
            (Layout::CenterThird, r(640, 0, 640, 1040)),
            (Layout::RightThird, r(1280, 0, 640, 1040)),
            (Layout::LeftTwoThirds, r(0, 0, 1280, 1040)),
            (Layout::RightTwoThirds, r(640, 0, 1280, 1040)),
            (Layout::Maximize, r(0, 0, 1920, 1040)),
            (Layout::AlmostMaximize, r(96, 52, 1728, 936)),
        ];
        for (layout, expected) in cases {
            assert_eq!(target(layout, WORK, 0), expected, "{layout:?}");
        }
    }

    #[test]
    fn layouts_follow_the_work_area_origin_and_negative_coordinates() {
        // A monitor left of the primary one, taskbar on its left edge.
        let work = r(-1880, 20, 1880, 1060);
        assert_eq!(target(Layout::LeftHalf, work, 0), r(-1880, 20, 940, 1060));
        assert_eq!(target(Layout::RightHalf, work, 0), r(-940, 20, 940, 1060));
        assert_eq!(target(Layout::BottomRight, work, 0), r(-940, 550, 940, 530));
        assert_eq!(target(Layout::Maximize, work, 0), work);
    }

    #[test]
    fn a_gap_surrounds_and_separates_tiles() {
        let gap = 10;
        let cases = [
            (Layout::Maximize, r(10, 10, 1900, 1020)),
            // Edge gap 10, gap between the halves 10: (1900 - 10) / 2 = 945.
            (Layout::LeftHalf, r(10, 10, 945, 1020)),
            (Layout::RightHalf, r(965, 10, 945, 1020)),
            (Layout::TopHalf, r(10, 10, 1900, 505)),
            (Layout::BottomHalf, r(10, 525, 1900, 505)),
            (Layout::TopLeft, r(10, 10, 945, 505)),
            (Layout::BottomRight, r(965, 525, 945, 505)),
            // Thirds: (1900 - 2 * 10) / 3 = 626.67, edges rounded down.
            (Layout::LeftThird, r(10, 10, 626, 1020)),
            (Layout::CenterThird, r(646, 10, 627, 1020)),
            (Layout::RightThird, r(1283, 10, 627, 1020)),
        ];
        for (layout, expected) in cases {
            assert_eq!(target(layout, WORK, gap), expected, "{layout:?}");
        }
    }

    #[test]
    fn neighbouring_tiles_leave_exactly_the_gap_between_them() {
        for gap in [0, 1, 7, 10, 16, 33] {
            let left = target(Layout::LeftHalf, WORK, gap);
            let right = target(Layout::RightHalf, WORK, gap);
            assert_eq!(right.x - left.right(), gap, "halves, gap {gap}");
            let top = target(Layout::TopHalf, WORK, gap);
            let bottom = target(Layout::BottomHalf, WORK, gap);
            assert_eq!(bottom.y - top.bottom(), gap, "rows, gap {gap}");

            let thirds = [
                target(Layout::LeftThird, WORK, gap),
                target(Layout::CenterThird, WORK, gap),
                target(Layout::RightThird, WORK, gap),
            ];
            assert_eq!(thirds[1].x - thirds[0].right(), gap, "thirds, gap {gap}");
            assert_eq!(thirds[2].x - thirds[1].right(), gap, "thirds, gap {gap}");
            // The outer edges keep the gap to the work area.
            assert_eq!(thirds[0].x, WORK.x + gap);
            assert_eq!(thirds[2].right(), WORK.right() - gap);

            // A two-thirds tile and the third beside it also keep the gap.
            let two = target(Layout::LeftTwoThirds, WORK, gap);
            assert_eq!(thirds[2].x - two.right(), gap, "two thirds, gap {gap}");
            let two_right = target(Layout::RightTwoThirds, WORK, gap);
            assert_eq!(two_right.x - thirds[0].right(), gap);
        }
    }

    #[test]
    fn every_layout_stays_inside_the_work_area_for_odd_sizes() {
        for work in [
            r(0, 0, 1366, 728),
            r(0, 0, 1921, 1041),
            r(-1440, 25, 1439, 875),
            r(3840, -200, 2559, 1400),
            r(0, 0, 801, 601),
        ] {
            for gap in [0, 3, 12, 40] {
                for layout in Layout::ALL {
                    let rect = layout.target(work, r(5, 5, 300, 200), gap);
                    assert!(!rect.is_empty(), "{layout:?} {work:?} gap {gap}");
                    assert!(work.contains(rect), "{layout:?} {rect:?} not in {work:?}");
                }
            }
        }
    }

    #[test]
    fn quarters_and_thirds_tile_the_work_area_without_holes_or_overlap() {
        for work in [
            r(0, 0, 1921, 1041),
            r(-7, 13, 1366, 728),
            r(0, 0, 1000, 1000),
        ] {
            let covered = |layouts: &[Layout]| -> i64 {
                layouts
                    .iter()
                    .map(|layout| {
                        let rect = layout.target(work, Rect::default(), 0);
                        i64::from(rect.width) * i64::from(rect.height)
                    })
                    .sum()
            };
            let whole = i64::from(work.width) * i64::from(work.height);
            let quarters = [
                Layout::TopLeft,
                Layout::TopRight,
                Layout::BottomLeft,
                Layout::BottomRight,
            ];
            let thirds = [Layout::LeftThird, Layout::CenterThird, Layout::RightThird];
            assert_eq!(covered(&quarters), whole, "quarters of {work:?}");
            assert_eq!(covered(&thirds), whole, "thirds of {work:?}");
            assert_eq!(covered(&[Layout::LeftHalf, Layout::RightHalf]), whole);
            assert_eq!(covered(&[Layout::TopHalf, Layout::BottomHalf]), whole);
            for pair in quarters
                .iter()
                .enumerate()
                .flat_map(|(i, a)| quarters[i + 1..].iter().map(move |b| (*a, *b)))
            {
                let a = pair.0.target(work, Rect::default(), 0);
                let b = pair.1.target(work, Rect::default(), 0);
                assert_eq!(a.overlap_area(b), 0, "{pair:?}");
            }
            // A third beside a two-thirds tile completes the work area.
            let two = Layout::LeftTwoThirds.target(work, Rect::default(), 0);
            let one = Layout::RightThird.target(work, Rect::default(), 0);
            assert_eq!(two.right(), one.x);
            assert_eq!(one.right(), work.right());
        }
    }

    #[test]
    fn center_keeps_the_size_and_shrinks_what_does_not_fit() {
        let work = r(0, 0, 1920, 1040);
        let centered = Layout::Center.target(work, r(5, 5, 800, 600), 0);
        assert_eq!(centered, r(560, 220, 800, 600));
        // Odd leftovers round towards the top left.
        let odd = Layout::Center.target(work, r(0, 0, 801, 601), 0);
        assert_eq!(odd, r(559, 219, 801, 601));
        // Larger than the work area: shrunk to it (minus the gap).
        let huge = Layout::Center.target(work, r(0, 0, 5000, 4000), 20);
        assert_eq!(huge, r(20, 20, 1880, 1000));
        // A gap does not move a window that already fits.
        assert_eq!(
            Layout::Center.target(work, r(5, 5, 800, 600), 20),
            r(560, 220, 800, 600)
        );
        // A window reported with no size still gets a usable one.
        let empty = Layout::Center.target(work, Rect::default(), 0);
        assert!(!empty.is_empty());
    }

    #[test]
    fn almost_maximize_leaves_a_margin_on_every_side() {
        let rect = Layout::AlmostMaximize.target(r(0, 0, 1000, 800), Rect::default(), 0);
        assert_eq!(rect, r(50, 40, 900, 720));
        let with_gap = Layout::AlmostMaximize.target(r(0, 0, 1000, 800), Rect::default(), 20);
        // 90 % of the area inside the gap (960 x 760), centered in it.
        assert_eq!(with_gap, r(68, 58, 864, 684));
    }

    #[test]
    fn an_oversized_gap_is_limited_so_tiles_survive() {
        let work = r(0, 0, 800, 600);
        for layout in Layout::ALL {
            let rect = layout.target(work, r(0, 0, 400, 300), 10_000);
            assert!(rect.width >= 10 && rect.height >= 10, "{layout:?} {rect:?}");
            assert!(work.contains(rect));
        }
        // A negative gap behaves like none.
        assert_eq!(
            target(Layout::LeftHalf, WORK, -5),
            target(Layout::LeftHalf, WORK, 0)
        );
        // A degenerate work area does not panic.
        let tiny = Layout::LeftHalf.target(r(0, 0, 0, 0), Rect::default(), 5);
        assert!(tiny.width >= 0 && tiny.height >= 0);
    }

    #[test]
    fn gaps_scale_with_the_display() {
        for (logical, percent, expected) in [
            (0, 100, 0),
            (10, 100, 10),
            (10, 125, 13),
            (10, 150, 15),
            (10, 200, 20),
            (8, 175, 14),
            (1, 150, 2),
            (-4, 150, 0),
            (10_000, 100, MAX_GAP),
            (10, 0, 0),
        ] {
            assert_eq!(
                scale_gap(logical, percent),
                expected,
                "{logical} @ {percent}"
            );
        }
    }

    #[test]
    fn keys_round_trip_and_are_unique() {
        let all = WindowCommand::all();
        assert_eq!(all.len(), Layout::ALL.len() + 3);
        let mut keys: Vec<_> = all.iter().map(|c| c.key()).collect();
        for command in &all {
            assert_eq!(WindowCommand::from_key(command.key()), Some(*command));
        }
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), all.len(), "keys must be unique");
        for bad in [
            "",
            "Left",
            "left ",
            "layout:left",
            "left;calc",
            "maximise",
            "../x",
        ] {
            assert_eq!(WindowCommand::from_key(bad), None, "{bad:?}");
        }
        assert_eq!(Layout::from_key("restore"), None);
    }

    #[test]
    fn rect_helpers() {
        let a = r(0, 0, 100, 100);
        assert_eq!(a.right(), 100);
        assert_eq!(a.bottom(), 100);
        assert_eq!(a.center(), (50, 50));
        assert!(a.contains_point(0, 0) && a.contains_point(99, 99));
        assert!(!a.contains_point(100, 50) && !a.contains_point(-1, 0));
        assert!(a.contains(r(10, 10, 90, 90)));
        assert!(!a.contains(r(10, 10, 91, 90)));
        assert_eq!(a.overlap_area(r(50, 50, 100, 100)), 2500);
        assert_eq!(
            a.overlap_area(r(100, 0, 10, 10)),
            0,
            "touching is not overlap"
        );
        assert_eq!(a.overlap_area(r(-50, -50, 40, 40)), 0);
        assert_eq!(a.intersection(r(50, 60, 100, 100)), Some(r(50, 60, 50, 40)));
        assert_eq!(a.intersection(r(100, 0, 10, 10)), None);
        assert!(r(0, 0, 0, 5).is_empty() && r(0, 0, 5, -1).is_empty());
        assert!(!a.is_empty());
        assert!(a.approx_eq(r(1, -2, 99, 102), 2));
        assert!(!a.approx_eq(r(3, 0, 100, 100), 2));
    }

    #[test]
    fn expand_and_shrink_are_inverses() {
        let insets = Insets {
            left: 7,
            top: 0,
            right: 7,
            bottom: 7,
        };
        let visible = r(100, 50, 800, 600);
        let outer = visible.expand(insets);
        assert_eq!(outer, r(93, 50, 814, 607));
        assert_eq!(outer.shrink(insets), visible);
        assert_eq!(visible.expand(Insets::ZERO), visible);
        assert!(Insets::ZERO.is_zero() && !insets.is_zero());
        // Shrinking never produces a negative size.
        assert_eq!(r(0, 0, 10, 10).shrink(Insets::uniform(20)).width, 0);
    }

    #[test]
    fn frame_insets_measure_the_invisible_border() {
        // A Windows 11 window: 7 px invisible border left, right and bottom.
        let outer = r(93, 50, 814, 607);
        let visible = r(100, 50, 800, 600);
        let insets = frame_insets(outer, visible);
        assert_eq!(
            insets,
            Insets {
                left: 7,
                top: 0,
                right: 7,
                bottom: 7
            }
        );
        assert_eq!(visible.expand(insets), outer);
        // A visible rectangle that pokes out of the outer one counts as 0.
        assert_eq!(frame_insets(visible, outer), Insets::ZERO);
        assert_eq!(frame_insets(visible, visible), Insets::ZERO);
    }

    #[test]
    fn clamp_into_moves_but_never_resizes() {
        let bounds = r(0, 0, 1000, 800);
        assert_eq!(
            r(900, 700, 300, 200).clamp_into(bounds),
            r(700, 600, 300, 200)
        );
        assert_eq!(r(-50, -20, 300, 200).clamp_into(bounds), r(0, 0, 300, 200));
        assert_eq!(r(10, 10, 300, 200).clamp_into(bounds), r(10, 10, 300, 200));
        // Larger than the bounds: pinned to the top left.
        assert_eq!(
            r(400, 400, 2000, 900).clamp_into(bounds),
            r(0, 0, 2000, 900)
        );
    }

    fn two_monitors() -> Vec<Monitor> {
        // The primary one (index 0) is on the right of the secondary one.
        let mut primary = Monitor::new("DISPLAY1", r(0, 0, 1920, 1080), r(0, 0, 1920, 1040));
        primary.primary = true;
        let secondary = Monitor::new(
            "DISPLAY2",
            r(-1280, -100, 1280, 1024),
            r(-1280, -100, 1280, 984),
        );
        vec![primary, secondary]
    }

    #[test]
    fn the_monitor_with_most_overlap_wins() {
        let monitors = two_monitors();
        assert_eq!(monitor_index_for(r(100, 100, 800, 600), &monitors), Some(0));
        assert_eq!(monitor_index_for(r(-1200, 0, 800, 600), &monitors), Some(1));
        // Straddling the border: 100 px on the primary, 700 on the secondary.
        assert_eq!(monitor_index_for(r(-700, 0, 800, 600), &monitors), Some(1));
        assert_eq!(monitor_index_for(r(-100, 0, 800, 600), &monitors), Some(0));
        // Off every screen: the nearest one.
        assert_eq!(
            monitor_index_for(r(5000, 100, 400, 300), &monitors),
            Some(0)
        );
        assert_eq!(
            monitor_index_for(r(-9000, 100, 400, 300), &monitors),
            Some(1)
        );
        assert_eq!(monitor_index_for(r(0, 0, 10, 10), &[]), None);
    }

    #[test]
    fn displays_are_walked_left_to_right_and_wrap() {
        let monitors = two_monitors();
        assert_eq!(display_order(&monitors), vec![1, 0]);
        assert_eq!(neighbour_display(&monitors, 1, true), Some(0));
        assert_eq!(neighbour_display(&monitors, 0, true), Some(1));
        assert_eq!(neighbour_display(&monitors, 0, false), Some(1));
        assert_eq!(neighbour_display(&monitors, 1, false), Some(0));
        assert_eq!(neighbour_display(&monitors, 5, true), None);

        let mut three = monitors;
        three.push(Monitor::new(
            "DISPLAY3",
            r(1920, 0, 2560, 1440),
            r(1920, 0, 2560, 1400),
        ));
        assert_eq!(display_order(&three), vec![1, 0, 2]);
        assert_eq!(neighbour_display(&three, 0, true), Some(2));
        assert_eq!(neighbour_display(&three, 2, true), Some(1));
        assert_eq!(neighbour_display(&three, 1, false), Some(2));

        // Stacked displays order by y when x is equal.
        let stacked = vec![
            Monitor::new("B", r(0, 1080, 1920, 1080), r(0, 1080, 1920, 1080)),
            Monitor::new("A", r(0, 0, 1920, 1080), r(0, 0, 1920, 1040)),
        ];
        assert_eq!(display_order(&stacked), vec![1, 0]);
    }

    #[test]
    fn moving_between_displays_keeps_the_relative_position_and_size() {
        let from = r(0, 0, 1920, 1040);
        let to = r(-1280, -100, 1280, 984);
        // Maximized stays maximized.
        assert_eq!(move_to_display(from, from, to), to);
        // The left half stays the left half (proportions, not pixels).
        assert_eq!(
            move_to_display(r(0, 0, 960, 1040), from, to),
            r(-1280, -100, 640, 984)
        );
        // The right half of a big display onto a small one.
        assert_eq!(
            move_to_display(r(960, 0, 960, 1040), from, to),
            r(-640, -100, 640, 984)
        );
        // Same-size displays keep the window as it is, shifted.
        let same = r(2000, 0, 1920, 1040);
        assert_eq!(
            move_to_display(r(100, 100, 800, 600), from, same),
            r(2100, 100, 800, 600)
        );
        // Whatever the input, the result stays on the target.
        for window in [
            r(-500, -500, 4000, 3000),
            r(1900, 1000, 500, 500),
            r(0, 0, 1, 1),
        ] {
            let moved = move_to_display(window, from, to);
            assert!(
                to.contains(moved) || moved.width > to.width,
                "{window:?} -> {moved:?}"
            );
            assert!(moved.width <= to.width && moved.height <= to.height);
        }
        // Empty work areas leave the window alone.
        assert_eq!(
            move_to_display(r(1, 2, 3, 4), Rect::default(), to),
            r(1, 2, 3, 4)
        );
    }

    #[test]
    fn plan_applies_the_gap_of_the_window_monitor() {
        let mut monitors = two_monitors();
        monitors[1].scale_percent = 150;
        // On the primary (100 %): a 10 px gap is 10 px.
        let on_primary = plan(
            WindowCommand::Layout(Layout::LeftHalf),
            r(100, 100, 800, 600),
            &monitors,
            10,
            None,
        )
        .unwrap();
        assert_eq!(on_primary, r(10, 10, 945, 1020));
        // On the 150 % display it is 15 physical pixels.
        let on_second = plan(
            WindowCommand::Layout(Layout::Maximize),
            r(-1200, 0, 800, 600),
            &monitors,
            10,
            None,
        )
        .unwrap();
        assert_eq!(on_second, r(-1265, -85, 1250, 954));
    }

    #[test]
    fn plan_restore_needs_something_to_restore() {
        let monitors = two_monitors();
        let window = r(100, 100, 800, 600);
        assert_eq!(
            plan(WindowCommand::Restore, window, &monitors, 0, None),
            Err(PlanError::NothingToRestore)
        );
        let before = r(50, 60, 700, 500);
        assert_eq!(
            plan(WindowCommand::Restore, window, &monitors, 0, Some(before)),
            Ok(before)
        );
    }

    #[test]
    fn plan_moves_to_the_next_and_previous_display() {
        let monitors = two_monitors();
        let maximized_primary = r(0, 0, 1920, 1040);
        let next = plan(
            WindowCommand::NextDisplay,
            maximized_primary,
            &monitors,
            0,
            None,
        )
        .unwrap();
        assert_eq!(
            next, monitors[1].work_area,
            "next from the right one wraps to the left one"
        );
        let previous = plan(
            WindowCommand::PreviousDisplay,
            maximized_primary,
            &monitors,
            0,
            None,
        )
        .unwrap();
        assert_eq!(previous, monitors[1].work_area);
        // From the secondary (left) one, next goes right.
        let back = plan(
            WindowCommand::NextDisplay,
            monitors[1].work_area,
            &monitors,
            0,
            None,
        )
        .unwrap();
        assert_eq!(back, monitors[0].work_area);
    }

    #[test]
    fn plan_reports_missing_displays() {
        let one = vec![Monitor::new("A", r(0, 0, 100, 100), r(0, 0, 100, 90))];
        assert_eq!(
            plan(WindowCommand::NextDisplay, r(0, 0, 50, 50), &one, 0, None),
            Err(PlanError::SingleDisplay)
        );
        assert_eq!(
            plan(
                WindowCommand::PreviousDisplay,
                r(0, 0, 50, 50),
                &one,
                0,
                None
            ),
            Err(PlanError::SingleDisplay)
        );
        assert_eq!(
            plan(
                WindowCommand::Layout(Layout::Maximize),
                r(0, 0, 50, 50),
                &[],
                0,
                None
            ),
            Err(PlanError::NoMonitors)
        );
        // The messages are meant for the user.
        assert!(PlanError::SingleDisplay.to_string().contains("one display"));
    }
}
