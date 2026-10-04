//! Windows: the first page of a PDF through `Windows.Data.Pdf`, and Shell
//! thumbnails of Office documents and videos.
//!
//! `Windows.Data.Pdf` is the PDF engine Windows ships for its own apps. It
//! works for an unpackaged desktop app: the file is opened as a plain
//! `StorageFile` by path, which needs no capability.
//!
//! Neither call can be told to stop, and a damaged file could in principle keep
//! one busy for a long time. So each runs on a worker thread of its own that the
//! caller waits for with a timeout; a thread that overruns is left to finish by
//! itself (its result is thrown away), and only one drawing job runs at a time,
//! so a stuck one can never pile up threads: a preview that has to wait for the
//! slot waits within its own timeout (moving down a list of PDFs queues them),
//! and is answered with "could not be drawn" if the slot does not free up.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Condvar, Mutex};
use std::time::{Duration, Instant};

use sevak_core::preview::{RenderKind, Rendered, RENDER_WIDTH};
use windows::core::{Interface, HSTRING};
use windows::Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions};
use windows::Storage::Streams::{DataReader, IRandomAccessStream, InMemoryRandomAccessStream};
use windows::Storage::{FileAccessMode, StorageFile};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use crate::thumbnail::{
    fit_page, measured_scale, scaled_request, MAX_RENDERED_BYTES, RENDER_TIMEOUT,
};

use super::icons::load_shell_thumbnail;

/// The one slot for drawing jobs: `true` while a job (possibly one that
/// overran its timeout) is still running.
struct Slot {
    busy: Mutex<bool>,
    freed: Condvar,
}

static SLOT: Slot = Slot {
    busy: Mutex::new(false),
    freed: Condvar::new(),
};

/// Holds the slot; frees it when dropped, on the job's thread, however it ends.
struct SlotGuard;

impl Slot {
    /// Waits up to `timeout` for the slot.
    fn acquire(&self, timeout: Duration) -> Option<SlotGuard> {
        let busy = self.busy.lock().unwrap_or_else(|p| p.into_inner());
        let (mut busy, result) = self
            .freed
            .wait_timeout_while(busy, timeout, |busy| *busy)
            .unwrap_or_else(|p| p.into_inner());
        if result.timed_out() && *busy {
            return None;
        }
        *busy = true;
        Some(SlotGuard)
    }

    #[cfg(test)]
    fn is_busy(&self) -> bool {
        *self.busy.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        *SLOT.busy.lock().unwrap_or_else(|p| p.into_inner()) = false;
        SLOT.freed.notify_one();
    }
}

pub(crate) fn render(path: &Path, kind: RenderKind) -> Rendered {
    let path = path.to_path_buf();
    match kind {
        RenderKind::Pdf => run_limited(RENDER_TIMEOUT, move || render_pdf(&path)),
        RenderKind::Document | RenderKind::Video => run_limited(RENDER_TIMEOUT, move || {
            // Shell paths take backslashes; `\\?\` prefixes are not parsed names.
            let name = path.to_string_lossy().replace('/', "\\");
            match load_shell_thumbnail(&name, RENDER_WIDTH) {
                Ok(png) if png.len() <= MAX_RENDERED_BYTES => Rendered::Image { png, pages: None },
                Ok(_) => Rendered::Failed,
                Err(err) => {
                    tracing::debug!(%err, "no shell thumbnail");
                    Rendered::Failed
                }
            }
        }),
    }
}

/// Runs `job` on a fresh multithreaded-COM worker thread and waits up to
/// `timeout` (in all, including for the slot) for it. [`Rendered::Failed`] when
/// it is too slow.
fn run_limited(timeout: Duration, job: impl FnOnce() -> Rendered + Send + 'static) -> Rendered {
    let deadline = Instant::now() + timeout;
    let Some(slot) = SLOT.acquire(timeout) else {
        tracing::debug!("another preview is still being drawn; giving up on this one");
        return Rendered::Failed;
    };
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("sevak-thumbnail".into())
        .spawn(move || {
            let _slot = slot;
            // SAFETY: no pointers; balanced by `CoUninitialize` below on this thread.
            let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let rendered = job();
            if initialized {
                // SAFETY: matches the successful `CoInitializeEx` above.
                unsafe { CoUninitialize() };
            }
            // The receiver is gone when the caller gave up.
            let _ = sender.send(rendered);
        });
    if spawned.is_err() {
        return Rendered::Failed;
    }
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(rendered) => rendered,
        Err(_) => {
            tracing::warn!(?timeout, "drawing a preview took too long; giving up on it");
            Rendered::Failed
        }
    }
}

fn render_pdf(path: &Path) -> Rendered {
    match try_render_pdf(path) {
        Ok(rendered) => rendered,
        Err(err) => {
            tracing::debug!(%err, "could not draw the PDF");
            Rendered::Failed
        }
    }
}

fn try_render_pdf(path: &Path) -> windows::core::Result<Rendered> {
    // WinRT paths are plain backslash paths.
    let name = path.to_string_lossy().replace('/', "\\");
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(name))?.join()?;
    let source = file.OpenAsync(FileAccessMode::Read)?.join()?;
    let document = PdfDocument::LoadFromStreamAsync(&source)?.join()?;
    if document.IsPasswordProtected().unwrap_or(false) {
        return Ok(Rendered::Failed);
    }
    let pages = document.PageCount().ok().filter(|&n| n > 0);
    let page = document.GetPage(0)?;

    let size = page.Size()?;
    let Some((width, height)) = fit_page(size.Width, size.Height, RENDER_WIDTH) else {
        return Ok(Rendered::Failed);
    };

    // The engine multiplies the size it is given by the display's scale (140%
    // here gave 1260 pixels for 900). Ask for less when the scale is known, and
    // learn it from the first picture when it is not.
    let mut scale = f32::from_bits(DISPLAY_SCALE.load(Ordering::Relaxed));
    let mut request = scaled_request(width, height, scale);
    let mut png = draw(&page, request)?;
    if let Some(measured) = measured_scale(&png, request.0) {
        if (measured - scale).abs() > 0.03 {
            scale = measured;
            DISPLAY_SCALE.store(scale.to_bits(), Ordering::Relaxed);
            request = scaled_request(width, height, scale);
            png = draw(&page, request)?;
        }
    }
    if png.len() > MAX_RENDERED_BYTES {
        return Ok(Rendered::Failed);
    }
    Ok(Rendered::Image { png, pages })
}

/// The display scale the PDF engine applied last time (`f32` bits), 1.0 until
/// the first page has been drawn.
static DISPLAY_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);

/// Largest picture read from the engine before the final size check.
const MAX_ENGINE_BYTES: u64 = 16 * 1024 * 1024;

/// Draws `page` as a PNG of (about) `width` x `height` pixels.
fn draw(page: &PdfPage, (width, height): (u32, u32)) -> windows::core::Result<Vec<u8>> {
    let options = PdfPageRenderOptions::new()?;
    options.SetDestinationWidth(width)?;
    options.SetDestinationHeight(height)?;

    let out = InMemoryRandomAccessStream::new()?;
    let target: IRandomAccessStream = out.cast()?;
    page.RenderWithOptionsToStreamAsync(&target, &options)?
        .join()?;

    let length = out.Size()?;
    // Bound what is read, whatever the engine produced.
    if length == 0 || length > MAX_ENGINE_BYTES {
        return Err(windows::core::Error::from(E_FAIL));
    }
    let reader = DataReader::CreateDataReader(&out.GetInputStreamAt(0)?)?;
    reader.LoadAsync(length as u32)?.join()?;
    let mut png = vec![0u8; length as usize];
    reader.ReadBytes(&mut png)?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    /// The tests that draw share the one-job-at-a-time slot; they take turns.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A minimal valid one-page PDF (a 200 x 100 pt page with a filled box),
    /// with a correct cross-reference table, generated here: no real
    /// document is ever used.
    pub(super) fn minimal_pdf() -> Vec<u8> {
        let content = "0.2 0.4 0.8 rg 20 20 160 60 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R \
             /Resources << >> >>"
                .to_owned(),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
        }
        let xref = pdf.len();
        pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for offset in offsets {
            pdf.extend(format!("{offset:010} 00000 n \n").bytes());
        }
        pdf.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .bytes(),
        );
        pdf
    }

    /// The real Windows PDF engine on a PDF generated by this test. Ignored by
    /// default (it needs a desktop session with WinRT); run with
    /// `cargo test -p sevak-platform windows::thumbnail -- --ignored`.
    #[test]
    #[ignore = "uses the Windows PDF engine"]
    fn a_generated_pdf_is_drawn_by_windows_data_pdf() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("generated.pdf");
        fs::write(&path, minimal_pdf()).unwrap();
        let started = std::time::Instant::now();
        let rendered = render(&path, RenderKind::Pdf);
        eprintln!("rendered in {:?}", started.elapsed());
        let Rendered::Image { png, pages } = rendered else {
            panic!("not drawn: {rendered:?}");
        };
        assert!(png.starts_with(PNG_SIGNATURE));
        assert_eq!(pages, Some(1));
        // 200 x 100 pt: twice as wide as tall, drawn 900 wide.
        let decoder = png::Decoder::new(std::io::Cursor::new(&png));
        let info = decoder.read_info().unwrap().info().clone();
        // (rounding after the display-scale correction may cost a pixel)
        assert!((899..=901).contains(&info.width), "{}", info.width);
        assert!((449..=451).contains(&info.height), "{}", info.height);
        if let Some(out) = std::env::var_os("SEVAK_PDF_DUMP_DIR") {
            fs::write(Path::new(&out).join("page1.png"), &png).unwrap();
        }
    }

    #[test]
    #[ignore = "uses the Windows PDF engine"]
    fn damaged_and_missing_pdfs_fail_cleanly() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.pdf");
        fs::write(&bad, "%PDF-1.4\nthis is not a pdf at all").unwrap();
        assert_eq!(render(&bad, RenderKind::Pdf), Rendered::Failed);
        assert_eq!(
            render(&dir.path().join("missing.pdf"), RenderKind::Pdf),
            Rendered::Failed
        );
    }

    /// The Shell thumbnail route (used for Office documents and videos), tried
    /// on a PNG this test writes: the Shell has a thumbnail provider for it on
    /// every machine, unlike for `.docx` (which needs Office) or video codecs.
    #[test]
    #[ignore = "uses the Windows Shell thumbnail cache"]
    fn the_shell_thumbnail_route_draws_a_generated_picture() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("generated.png");
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 64, 48);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[200u8; 64 * 48 * 4]).unwrap();
        writer.finish().unwrap();
        fs::write(&path, bytes).unwrap();
        let rendered = render(&path, RenderKind::Document);
        let Rendered::Image { png, pages } = rendered else {
            panic!("no thumbnail: {rendered:?}");
        };
        assert!(png.starts_with(PNG_SIGNATURE));
        assert_eq!(pages, None);
        // A file without a thumbnail (a text file) is a failure, not a generic icon.
        let text = dir.path().join("notes.txt");
        fs::write(&text, "hello").unwrap();
        assert_eq!(render(&text, RenderKind::Document), Rendered::Failed);
    }

    #[test]
    fn the_generated_pdf_is_well_formed() {
        let pdf = String::from_utf8(minimal_pdf()).unwrap();
        assert!(pdf.starts_with("%PDF-1.4"));
        // Every xref offset points at its object.
        let xref = pdf.rfind("xref\n0 5").unwrap();
        let entries: Vec<&str> = pdf[xref..].lines().skip(3).take(4).collect();
        for (i, entry) in entries.iter().enumerate() {
            let offset: usize = entry[..10].parse().unwrap();
            assert!(
                pdf[offset..].starts_with(&format!("{} 0 obj", i + 1)),
                "object {}",
                i + 1
            );
        }
        let start: usize = pdf
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(start, xref);
    }

    #[test]
    fn jobs_take_turns_and_a_stuck_one_only_delays_the_others_until_their_timeout() {
        use std::sync::mpsc::channel;
        let _serial = serial();
        let (release, wait) = channel::<()>();
        let (started_tx, started_rx) = channel::<()>();
        // A job that outlives the caller's patience.
        let first = std::thread::spawn(move || {
            run_limited(Duration::from_millis(100), move || {
                started_tx.send(()).unwrap();
                let _ = wait.recv();
                Rendered::Failed
            })
        });
        started_rx.recv().unwrap();
        assert_eq!(first.join().unwrap(), Rendered::Failed, "timed out");
        // The abandoned job still holds the slot: another job waits for it
        // within its own timeout and then gives up without running.
        assert!(SLOT.is_busy());
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = ran.clone();
        let refused = run_limited(Duration::from_millis(100), move || {
            flag.store(true, Ordering::SeqCst);
            Rendered::Unavailable { hint: None }
        });
        assert_eq!(refused, Rendered::Failed);
        assert!(!ran.load(Ordering::SeqCst));

        // A patient job runs as soon as the slot frees up.
        let patient = std::thread::spawn(|| {
            run_limited(Duration::from_secs(60), || Rendered::Unavailable {
                hint: Some("ran".into()),
            })
        });
        release.send(()).unwrap();
        assert_eq!(
            patient.join().unwrap(),
            Rendered::Unavailable {
                hint: Some("ran".into())
            }
        );
        let after = run_limited(Duration::from_secs(60), || Rendered::Unavailable {
            hint: None,
        });
        assert_eq!(after, Rendered::Unavailable { hint: None });
    }
}
