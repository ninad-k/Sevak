//! File logging. Blocking writes on purpose: Tauri may end the process with
//! `std::process::exit`, which would lose lines buffered by a worker thread,
//! and Sevak logs very little. It also keeps the process single-threaded until
//! the environment tweaks in `main` are done.

use std::io;

use sevak_platform::AppPaths;
use tracing_appender::rolling::{Builder, RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

const FILTER_ENV: &str = "SEVAK_LOG";
const LOG_FILE_PREFIX: &str = "sevak";
const LOG_FILE_SUFFIX: &str = "log";
const MAX_LOG_FILES: usize = 7;

pub fn init(paths: &AppPaths) {
    let filter = EnvFilter::try_from_env(FILTER_ENV).unwrap_or_else(|_| EnvFilter::new("info"));

    let (file_layer, file_error) = match file_appender(paths) {
        Ok(appender) => (
            Some(fmt::layer().with_ansi(false).with_writer(appender).boxed()),
            None,
        ),
        Err(err) => (None, Some(err)),
    };
    // Without a file we always need stderr; otherwise only for debug builds.
    let stderr_layer = (cfg!(debug_assertions) || file_layer.is_none())
        .then(|| fmt::layer().with_writer(io::stderr).boxed());

    // `try_init` also bridges `log` records (Tauri's) through tracing-log.
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init();

    if let Some(err) = file_error {
        tracing::warn!(dir = %paths.log_dir.display(), "file logging disabled: {err}");
    }
}

fn file_appender(paths: &AppPaths) -> Result<RollingFileAppender, String> {
    // The data and log folders are owner-only on Unix (0700).
    paths.ensure_private_dirs().map_err(|err| err.to_string())?;
    Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(&paths.log_dir)
        .map_err(|err| err.to_string())
}
