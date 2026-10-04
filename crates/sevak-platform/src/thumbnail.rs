//! Pictures of files for the preview pane: the first page of a PDF, and
//! thumbnails of Office documents and videos.
//!
//! Sevak does not parse these formats. It asks the operating system, which
//! already has the engine and which the user already trusts with those files:
//!
//! | | PDF | Office documents, videos |
//! |---|---|---|
//! | Windows | `Windows.Data.Pdf` (WinRT) | the Shell thumbnail (`IShellItemImageFactory`) |
//! | macOS | Quick Look (`qlmanage -t`) | Quick Look |
//! | Linux | `pdftoppm` from poppler-utils, when installed | not drawn |
//!
//! Everything is bounded: helper processes get a timeout and are killed when
//! it runs out, their output goes to an anonymous temporary file that is read
//! back with a size limit, temporary folders are deleted, and nothing is
//! cached on disk. The command lines are built by pure functions
//! ([`pdftoppm_command`], [`qlmanage_command`], ...) so they are tested
//! everywhere; [`PlatformProvider::render_thumbnail`](crate::PlatformProvider)
//! is the entry point.
//!
//! The size limits on the *input* (50 MB for PDFs and documents) are the
//! preview's business (`sevak_core::preview`); the limit on the picture that
//! comes back is enforced here as well, so no helper can make Sevak read more
//! than [`MAX_RENDERED_BYTES`].

// Each OS runs one family of helpers; all of them compile everywhere so that
// their command lines and parsers are tested everywhere.
#![allow(dead_code)]

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sevak_core::preview::{
    RenderKind, Rendered, MAX_RENDERED_BYTES as CORE_MAX_RENDERED_BYTES, RENDER_WIDTH,
};

/// Longest wait for a helper that draws a page.
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest wait for a helper that only answers a question (the page count).
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
/// Largest picture read back from a helper.
pub const MAX_RENDERED_BYTES: usize = CORE_MAX_RENDERED_BYTES;
/// Most of a helper's text output that is kept.
const MAX_HELPER_TEXT: usize = 64 * 1024;
/// How often a running helper is looked at while waiting for it.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// The page of a Windows PDF is drawn no taller than this (pixels); a poster
/// is shrunk to fit, keeping its shape.
const MAX_PAGE_HEIGHT: u32 = 1400;

/// A picture of the first page of `path`, drawn by the operating system. Blocks
/// for as long as the helper takes, up to its timeout: call it from a
/// background thread.
pub fn render(path: &Path, kind: RenderKind) -> Rendered {
    #[cfg(windows)]
    {
        crate::windows::thumbnail::render(path, kind)
    }
    #[cfg(target_os = "macos")]
    {
        quick_look(path, kind, &run_helper)
    }
    #[cfg(target_os = "linux")]
    {
        poppler(path, kind, &run_helper)
    }
}

// ---- helper processes ------------------------------------------------------------

/// An external program and its arguments, built without running anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperCommand {
    pub program: &'static str,
    pub args: Vec<OsString>,
}

impl HelperCommand {
    fn new(program: &'static str, args: Vec<OsString>) -> Self {
        Self { program, args }
    }
}

/// Why a helper gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperError {
    /// The program is not installed.
    NotInstalled,
    /// It did not finish in time and was killed.
    TimedOut,
    /// It could not be started, or exited with an error.
    Failed,
}

/// Runs a helper to completion, or kills it after `timeout`. Returns the start
/// of what it wrote to standard output (at most `output_limit` bytes). Its
/// standard error is discarded and it gets no input.
///
/// Output goes to an anonymous temporary file rather than a pipe: a helper that
/// writes a lot can never block on a full pipe while we wait for it, and what we
/// read back is bounded by `output_limit` whatever it wrote.
pub fn run_helper(
    command: &HelperCommand,
    timeout: Duration,
    output_limit: usize,
) -> Result<Vec<u8>, HelperError> {
    let mut output = tempfile::tempfile().map_err(|_| HelperError::Failed)?;
    let sink = output.try_clone().map_err(|_| HelperError::Failed)?;
    let mut child = match Command::new(command.program)
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(sink))
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(HelperError::NotInstalled)
        }
        Err(_) => return Err(HelperError::Failed),
    };

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                // Kill, then reap, so no zombie or runaway renderer is left.
                let _ = child.kill();
                let _ = child.wait();
                return Err(HelperError::TimedOut);
            }
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(HelperError::Failed);
            }
        }
    };
    if !status.success() {
        return Err(HelperError::Failed);
    }

    let mut bytes = Vec::new();
    output
        .seek(SeekFrom::Start(0))
        .and_then(|_| output.take(output_limit as u64).read_to_end(&mut bytes))
        .map_err(|_| HelperError::Failed)?;
    Ok(bytes)
}

/// Runs a helper through `runner`: [`run_helper`], or a fake in tests.
pub type Runner<'a> = &'a dyn Fn(&HelperCommand, Duration, usize) -> Result<Vec<u8>, HelperError>;

/// The text a helper printed, for parsing a number out of it.
fn helper_text(runner: Runner<'_>, command: &HelperCommand, timeout: Duration) -> Option<String> {
    let bytes = runner(command, timeout, MAX_HELPER_TEXT).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads a picture a helper wrote, refusing anything over
/// [`MAX_RENDERED_BYTES`] (checked before reading, so a huge file costs nothing).
fn read_picture(path: &Path) -> Option<Vec<u8>> {
    let length = fs::metadata(path).ok()?.len();
    if length == 0 || length > MAX_RENDERED_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::with_capacity(length as usize);
    File::open(path)
        .ok()?
        .take(MAX_RENDERED_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_RENDERED_BYTES).then_some(bytes)
}

/// A private temporary folder for a helper's output; deleted when dropped.
fn scratch_dir() -> Option<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix("sevak-preview-")
        .tempdir()
        .ok()
}

// ---- Linux: poppler ---------------------------------------------------------------

/// What to tell the user when `pdftoppm` is missing.
pub const POPPLER_HINT: &str =
    "Install poppler-utils (pdftoppm) to preview PDF pages; Enter opens the file";

/// `pdftoppm -png -f 1 -l 1 -singlefile -scale-to 900 <pdf> <output root>`:
/// the first page only, as `<output root>.png`, its longer side 900 pixels.
pub fn pdftoppm_command(pdf: &Path, output_root: &Path, long_side: u32) -> HelperCommand {
    HelperCommand::new(
        "pdftoppm",
        vec![
            "-png".into(),
            "-f".into(),
            "1".into(),
            "-l".into(),
            "1".into(),
            "-singlefile".into(),
            "-scale-to".into(),
            long_side.to_string().into(),
            // The path is absolute (the preview refuses relative ones), so it
            // cannot be read as an option.
            pdf.as_os_str().to_owned(),
            output_root.as_os_str().to_owned(),
        ],
    )
}

/// `pdfinfo <pdf>`, whose output has a `Pages:` line.
pub fn pdfinfo_command(pdf: &Path) -> HelperCommand {
    HelperCommand::new("pdfinfo", vec![pdf.as_os_str().to_owned()])
}

/// The page count from `pdfinfo`'s output (`Pages:          12`).
pub fn parse_pdfinfo_pages(output: &str) -> Option<u32> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("Pages:"))
        .and_then(|rest| rest.trim().parse().ok())
}

/// Draws a PDF's first page with poppler. Only PDFs: documents and videos are
/// not drawn on Linux.
pub fn poppler(path: &Path, kind: RenderKind, runner: Runner<'_>) -> Rendered {
    if kind != RenderKind::Pdf {
        return Rendered::Unavailable { hint: None };
    }
    let Some(dir) = scratch_dir() else {
        return Rendered::Failed;
    };
    let root = dir.path().join("page");
    let command = pdftoppm_command(path, &root, RENDER_WIDTH);
    match runner(&command, RENDER_TIMEOUT, 0) {
        Ok(_) => {}
        Err(HelperError::NotInstalled) => {
            return Rendered::Unavailable {
                hint: Some(POPPLER_HINT.to_owned()),
            }
        }
        Err(_) => return Rendered::Failed,
    }
    let Some(png) = read_picture(&dir.path().join("page.png")) else {
        return Rendered::Failed;
    };
    let pages = helper_text(runner, &pdfinfo_command(path), QUERY_TIMEOUT)
        .and_then(|text| parse_pdfinfo_pages(&text));
    Rendered::Image { png, pages }
}

// ---- macOS: Quick Look ------------------------------------------------------------

/// `qlmanage -t -s 900 -o <dir> <file>`: Quick Look's thumbnail of the file, as
/// `<dir>/<file name>.png`.
pub fn qlmanage_command(file: &Path, out_dir: &Path, size: u32) -> HelperCommand {
    HelperCommand::new(
        "qlmanage",
        vec![
            "-t".into(),
            "-s".into(),
            size.to_string().into(),
            "-o".into(),
            out_dir.as_os_str().to_owned(),
            file.as_os_str().to_owned(),
        ],
    )
}

/// Where `qlmanage` writes the thumbnail of `file`.
pub fn qlmanage_output(file: &Path, out_dir: &Path) -> Option<PathBuf> {
    let mut name = file.file_name()?.to_owned();
    name.push(".png");
    Some(out_dir.join(name))
}

/// `mdls -name kMDItemNumberOfPages -raw <file>`: Spotlight's page count.
pub fn mdls_pages_command(file: &Path) -> HelperCommand {
    HelperCommand::new(
        "mdls",
        vec![
            "-name".into(),
            "kMDItemNumberOfPages".into(),
            "-raw".into(),
            file.as_os_str().to_owned(),
        ],
    )
}

/// The count from `mdls -raw`: a number, or `(null)` when Spotlight has none.
pub fn parse_mdls_pages(output: &str) -> Option<u32> {
    output.trim().parse().ok()
}

/// Draws the first page or a thumbnail of a file with Quick Look.
pub fn quick_look(path: &Path, kind: RenderKind, runner: Runner<'_>) -> Rendered {
    let Some(dir) = scratch_dir() else {
        return Rendered::Failed;
    };
    let command = qlmanage_command(path, dir.path(), RENDER_WIDTH);
    match runner(&command, RENDER_TIMEOUT, 0) {
        Ok(_) => {}
        Err(HelperError::NotInstalled) => return Rendered::Unavailable { hint: None },
        Err(_) => return Rendered::Failed,
    }
    // `qlmanage` exits successfully even when it made nothing.
    let Some(png) = qlmanage_output(path, dir.path()).and_then(|file| read_picture(&file)) else {
        return Rendered::Failed;
    };
    let pages = (kind == RenderKind::Pdf)
        .then(|| helper_text(runner, &mdls_pages_command(path), QUERY_TIMEOUT))
        .flatten()
        .and_then(|text| parse_mdls_pages(&text));
    Rendered::Image { png, pages }
}

// ---- Windows: sizing -------------------------------------------------------------

/// The pixel size to draw a page of `width` x `height` (any unit) at: as wide as
/// `max_width` allows, but no taller than [`MAX_PAGE_HEIGHT`], keeping the shape.
/// `None` for a page with no usable size.
pub fn fit_page(width: f32, height: f32, max_width: u32) -> Option<(u32, u32)> {
    if !(width.is_finite() && height.is_finite()) || width < 1.0 || height < 1.0 {
        return None;
    }
    let mut w = max_width as f32;
    let mut h = w * height / width;
    if h > MAX_PAGE_HEIGHT as f32 {
        h = MAX_PAGE_HEIGHT as f32;
        w = h * width / height;
    }
    Some((w.round().max(1.0) as u32, h.round().max(1.0) as u32))
}

/// The size to ask the PDF engine for when it multiplies sizes by the display
/// `scale` (1.4 at 140%), so that the picture comes out `width` x `height`.
pub fn scaled_request(width: u32, height: u32, scale: f32) -> (u32, u32) {
    let scale = if scale.is_finite() && scale >= 0.5 {
        scale
    } else {
        1.0
    };
    (
        ((width as f32 / scale).round() as u32).max(1),
        ((height as f32 / scale).round() as u32).max(1),
    )
}

/// The width of a PNG, from its header.
pub fn png_width(png: &[u8]) -> Option<u32> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if !png.starts_with(&SIGNATURE) || png.get(12..16)? != b"IHDR" {
        return None;
    }
    Some(u32::from_be_bytes(png.get(16..20)?.try_into().ok()?))
}

/// The factor by which a picture came out wider than the `requested` width.
pub fn measured_scale(png: &[u8], requested: u32) -> Option<f32> {
    let width = png_width(png)?;
    (requested > 0 && width > 0).then(|| width as f32 / requested as f32)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

    fn args(command: &HelperCommand) -> Vec<String> {
        command
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    // --- command lines and parsers (pure) ---

    #[test]
    fn pdftoppm_draws_page_one_to_a_known_file() {
        let command = pdftoppm_command(Path::new("/docs/a b.pdf"), Path::new("/tmp/x/page"), 900);
        assert_eq!(command.program, "pdftoppm");
        assert_eq!(
            args(&command),
            [
                "-png",
                "-f",
                "1",
                "-l",
                "1",
                "-singlefile",
                "-scale-to",
                "900",
                "/docs/a b.pdf",
                "/tmp/x/page"
            ]
        );
    }

    #[test]
    fn qlmanage_writes_the_file_name_plus_png() {
        let command = qlmanage_command(Path::new("/docs/paper.pdf"), Path::new("/tmp/x"), 900);
        assert_eq!(command.program, "qlmanage");
        assert_eq!(
            args(&command),
            ["-t", "-s", "900", "-o", "/tmp/x", "/docs/paper.pdf"]
        );
        assert_eq!(
            qlmanage_output(Path::new("/docs/paper.pdf"), Path::new("/tmp/x")),
            Some(Path::new("/tmp/x").join("paper.pdf.png"))
        );
        assert_eq!(qlmanage_output(Path::new("/"), Path::new("/tmp/x")), None);
    }

    #[test]
    fn page_counts_are_parsed_from_helper_output() {
        assert_eq!(
            parse_pdfinfo_pages("Title:  x\nPages:          12\nEncrypted: no\n"),
            Some(12)
        );
        assert_eq!(parse_pdfinfo_pages("Pages: lots"), None);
        assert_eq!(parse_pdfinfo_pages("nothing here"), None);
        assert_eq!(parse_mdls_pages("7\n"), Some(7));
        assert_eq!(parse_mdls_pages("(null)"), None);
        assert_eq!(mdls_pages_command(Path::new("/a.pdf")).program, "mdls");
        assert_eq!(pdfinfo_command(Path::new("/a.pdf")).program, "pdfinfo");
    }

    #[test]
    fn pages_are_fitted_keeping_their_shape() {
        // US Letter and A4 portrait: 900 wide.
        assert_eq!(fit_page(612.0, 792.0, 900), Some((900, 1165)));
        assert_eq!(fit_page(595.0, 842.0, 900), Some((900, 1274)));
        // Landscape.
        assert_eq!(fit_page(792.0, 612.0, 900), Some((900, 695)));
        // A tall poster is shrunk to the height limit instead of growing.
        let (w, h) = fit_page(100.0, 1000.0, 900).unwrap();
        assert_eq!(h, MAX_PAGE_HEIGHT);
        assert_eq!(w, 140);
        // Nonsense sizes.
        for (w, h) in [
            (0.0, 10.0),
            (10.0, 0.0),
            (f32::NAN, 10.0),
            (10.0, f32::INFINITY),
            (-5.0, 5.0),
        ] {
            assert_eq!(fit_page(w, h, 900), None, "{w} x {h}");
        }
    }

    #[test]
    fn display_scaling_is_compensated_for() {
        assert_eq!(scaled_request(900, 450, 1.0), (900, 450));
        assert_eq!(scaled_request(1260, 630, 1.4), (900, 450));
        assert_eq!(scaled_request(900, 1165, 1.25), (720, 932));
        // Nonsense scales are ignored.
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(scaled_request(900, 450, scale), (900, 450), "{scale}");
        }
        assert_eq!(scaled_request(1, 1, 4.0), (1, 1));
    }

    #[test]
    fn png_width_comes_from_the_header() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend([0, 0, 0, 13]);
        png.extend(b"IHDR");
        png.extend(1260u32.to_be_bytes());
        png.extend(630u32.to_be_bytes());
        assert_eq!(png_width(&png), Some(1260));
        assert_eq!(measured_scale(&png, 900), Some(1.4));
        assert_eq!(measured_scale(&png, 0), None);
        assert_eq!(png_width(&png[..18]), None);
        assert_eq!(png_width(b"GIF89a"), None);
        assert_eq!(png_width(&[]), None);
    }

    // --- the flows, with a fake runner that "draws" ---

    /// A runner that behaves like `pdftoppm`/`qlmanage`: it records the commands
    /// and writes `picture` where the real helper would.
    struct FakeHelpers {
        picture: Vec<u8>,
        outcome: Result<(), HelperError>,
        info: Option<&'static str>,
        seen: RefCell<Vec<HelperCommand>>,
        wrote_to: RefCell<Option<PathBuf>>,
    }

    impl FakeHelpers {
        fn new(picture: &[u8]) -> Self {
            Self {
                picture: picture.to_vec(),
                outcome: Ok(()),
                info: None,
                seen: RefCell::new(Vec::new()),
                wrote_to: RefCell::new(None),
            }
        }

        fn run(
            &self,
            command: &HelperCommand,
            _timeout: Duration,
            _limit: usize,
        ) -> Result<Vec<u8>, HelperError> {
            self.seen.borrow_mut().push(command.clone());
            match command.program {
                "pdftoppm" => {
                    self.outcome.clone()?;
                    let root = PathBuf::from(command.args.last().unwrap());
                    let file = root.with_extension("png");
                    fs::write(&file, &self.picture).unwrap();
                    *self.wrote_to.borrow_mut() = Some(file);
                    Ok(Vec::new())
                }
                "qlmanage" => {
                    self.outcome.clone()?;
                    let dir = PathBuf::from(&command.args[4]);
                    let source = PathBuf::from(command.args.last().unwrap());
                    let file = qlmanage_output(&source, &dir).unwrap();
                    if !self.picture.is_empty() {
                        fs::write(&file, &self.picture).unwrap();
                        *self.wrote_to.borrow_mut() = Some(file);
                    }
                    Ok(Vec::new())
                }
                "pdfinfo" | "mdls" => self
                    .info
                    .map(|text| text.as_bytes().to_vec())
                    .ok_or(HelperError::Failed),
                other => panic!("unexpected helper {other}"),
            }
        }
    }

    #[test]
    fn poppler_returns_the_page_and_its_count_and_cleans_up() {
        let fake = FakeHelpers {
            info: Some("Pages:   3\n"),
            ..FakeHelpers::new(PNG)
        };
        let rendered = poppler(Path::new("/docs/a.pdf"), RenderKind::Pdf, &|c, t, l| {
            fake.run(c, t, l)
        });
        assert_eq!(
            rendered,
            Rendered::Image {
                png: PNG.to_vec(),
                pages: Some(3)
            }
        );
        let seen = fake.seen.borrow();
        assert_eq!(seen[0].program, "pdftoppm");
        assert_eq!(seen[1].program, "pdfinfo");
        // The temporary folder is gone.
        let written = fake.wrote_to.borrow().clone().unwrap();
        assert!(!written.exists(), "{}", written.display());
        assert!(!written.parent().unwrap().exists());
    }

    #[test]
    fn poppler_without_page_info_still_shows_the_page() {
        let fake = FakeHelpers::new(PNG);
        let rendered = poppler(Path::new("/docs/a.pdf"), RenderKind::Pdf, &|c, t, l| {
            fake.run(c, t, l)
        });
        assert_eq!(
            rendered,
            Rendered::Image {
                png: PNG.to_vec(),
                pages: None
            }
        );
    }

    #[test]
    fn poppler_missing_gives_an_install_hint_and_other_failures_do_not() {
        let missing = FakeHelpers {
            outcome: Err(HelperError::NotInstalled),
            ..FakeHelpers::new(PNG)
        };
        let rendered = poppler(Path::new("/a.pdf"), RenderKind::Pdf, &|c, t, l| {
            missing.run(c, t, l)
        });
        assert_eq!(
            rendered,
            Rendered::Unavailable {
                hint: Some(POPPLER_HINT.to_owned())
            }
        );
        assert!(POPPLER_HINT.contains("poppler-utils"));

        for error in [HelperError::TimedOut, HelperError::Failed] {
            let fake = FakeHelpers {
                outcome: Err(error),
                ..FakeHelpers::new(PNG)
            };
            let rendered = poppler(Path::new("/a.pdf"), RenderKind::Pdf, &|c, t, l| {
                fake.run(c, t, l)
            });
            assert_eq!(rendered, Rendered::Failed);
        }
    }

    #[test]
    fn poppler_does_not_draw_documents_or_videos() {
        let fake = FakeHelpers::new(PNG);
        for kind in [RenderKind::Document, RenderKind::Video] {
            let rendered = poppler(Path::new("/a.docx"), kind, &|c, t, l| fake.run(c, t, l));
            assert_eq!(rendered, Rendered::Unavailable { hint: None });
        }
        assert!(fake.seen.borrow().is_empty());
    }

    #[test]
    fn an_oversized_or_empty_picture_is_refused() {
        let big = vec![7u8; MAX_RENDERED_BYTES + 1];
        let fake = FakeHelpers::new(&big);
        let rendered = poppler(Path::new("/a.pdf"), RenderKind::Pdf, &|c, t, l| {
            fake.run(c, t, l)
        });
        assert_eq!(rendered, Rendered::Failed);

        let fake = FakeHelpers::new(&[]);
        let rendered = poppler(Path::new("/a.pdf"), RenderKind::Pdf, &|c, t, l| {
            fake.run(c, t, l)
        });
        assert_eq!(rendered, Rendered::Failed, "no picture, no preview");

        let at_limit = vec![7u8; MAX_RENDERED_BYTES];
        let fake = FakeHelpers::new(&at_limit);
        let rendered = poppler(Path::new("/a.pdf"), RenderKind::Pdf, &|c, t, l| {
            fake.run(c, t, l)
        });
        assert!(matches!(rendered, Rendered::Image { .. }));
    }

    #[test]
    fn quick_look_returns_the_thumbnail_and_a_pdfs_page_count() {
        let fake = FakeHelpers {
            info: Some("42\n"),
            ..FakeHelpers::new(PNG)
        };
        let rendered = quick_look(Path::new("/docs/a.pdf"), RenderKind::Pdf, &|c, t, l| {
            fake.run(c, t, l)
        });
        assert_eq!(
            rendered,
            Rendered::Image {
                png: PNG.to_vec(),
                pages: Some(42)
            }
        );
        let written = fake.wrote_to.borrow().clone().unwrap();
        assert!(!written.exists(), "temporary folder deleted");

        // A document has no page count asked for.
        let fake = FakeHelpers {
            info: Some("42\n"),
            ..FakeHelpers::new(PNG)
        };
        let rendered = quick_look(
            Path::new("/docs/a.docx"),
            RenderKind::Document,
            &|c, t, l| fake.run(c, t, l),
        );
        assert_eq!(
            rendered,
            Rendered::Image {
                png: PNG.to_vec(),
                pages: None
            }
        );
        assert_eq!(fake.seen.borrow().len(), 1);
    }

    #[test]
    fn quick_look_that_made_nothing_is_a_failure() {
        // `qlmanage` exits 0 without writing a file for what it cannot draw.
        let fake = FakeHelpers::new(&[]);
        let rendered = quick_look(Path::new("/a.mov"), RenderKind::Video, &|c, t, l| {
            fake.run(c, t, l)
        });
        assert_eq!(rendered, Rendered::Failed);
    }

    // --- real processes: timeout, output cap, missing program ---

    #[cfg(windows)]
    fn long_running() -> HelperCommand {
        // `ping` waits a second between replies: about half a minute in all.
        HelperCommand::new("ping", vec!["-n".into(), "30".into(), "127.0.0.1".into()])
    }

    #[cfg(not(windows))]
    fn long_running() -> HelperCommand {
        HelperCommand::new("sleep", vec!["30".into()])
    }

    #[test]
    fn a_helper_that_runs_too_long_is_killed() {
        let started = Instant::now();
        let result = run_helper(&long_running(), Duration::from_millis(200), 1024);
        assert_eq!(result, Err(HelperError::TimedOut));
        // It was killed, not waited for (the helper itself runs for ~30 s).
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn a_missing_helper_is_reported_as_not_installed() {
        let command = HelperCommand::new("sevak-no-such-helper-xyz", Vec::new());
        assert_eq!(
            run_helper(&command, Duration::from_secs(5), 1024),
            Err(HelperError::NotInstalled)
        );
    }

    #[cfg(windows)]
    fn print_file(path: &Path) -> HelperCommand {
        HelperCommand::new(
            "cmd",
            vec!["/C".into(), "type".into(), path.as_os_str().to_owned()],
        )
    }

    #[cfg(not(windows))]
    fn print_file(path: &Path) -> HelperCommand {
        HelperCommand::new("cat", vec![path.as_os_str().to_owned()])
    }

    #[test]
    fn helper_output_is_capped_and_failures_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("out.txt");
        fs::write(&file, "x".repeat(10_000)).unwrap();
        let output = run_helper(&print_file(&file), Duration::from_secs(30), 100).unwrap();
        assert_eq!(output.len(), 100);
        let all = run_helper(&print_file(&file), Duration::from_secs(30), 1 << 20).unwrap();
        assert_eq!(all.len(), 10_000);

        // A non-zero exit is a failure: `type`/`cat` of a missing file.
        let missing = dir.path().join("missing.txt");
        assert_eq!(
            run_helper(&print_file(&missing), Duration::from_secs(30), 100),
            Err(HelperError::Failed)
        );
    }
}
