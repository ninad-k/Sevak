//! Builds a gallery package: a zip of one folder, ready for `gallery/packages/`.
//!
//! ```text
//! cargo run -p sevak-plugins --example gallery_pack -- examples/workflows/duckduckgo gallery/packages/duckduckgo.zip
//! ```
//!
//! The zip holds the folder under its own name (`duckduckgo/workflow.toml`),
//! files in sorted order, with fixed timestamps and permissions, so packing the
//! same folder twice gives the same bytes and the same checksum. The program
//! prints the SHA-256 to put in `gallery/index.json`.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sevak_plugins::workflow::gallery::sha256_hex;
use zip::write::SimpleFileOptions;

fn files_below(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            files_below(root, &path, out)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(relative.to_path_buf());
        }
    }
    Ok(())
}

fn pack(source: &Path) -> Result<Vec<u8>, String> {
    let folder = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("the folder has no name")?;
    let mut files = Vec::new();
    files_below(source, source, &mut files).map_err(|err| err.to_string())?;
    files.sort();
    if files.is_empty() {
        return Err("the folder is empty".to_owned());
    }
    let mut out = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut out);
    // No `time` feature: entries get the format's earliest date, 1980-01-01.
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for relative in files {
        let name = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let bytes = fs::read(source.join(&relative)).map_err(|err| err.to_string())?;
        writer
            .start_file(format!("{folder}/{name}"), options)
            .map_err(|err| err.to_string())?;
        writer.write_all(&bytes).map_err(|err| err.to_string())?;
    }
    writer.finish().map_err(|err| err.to_string())?;
    Ok(out.into_inner())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [source, target] = args.as_slice() else {
        eprintln!("usage: gallery_pack <folder> <output.zip>");
        return ExitCode::from(2);
    };
    match pack(Path::new(source)) {
        Ok(bytes) => {
            if let Err(err) = fs::write(target, &bytes) {
                eprintln!("cannot write {target}: {err}");
                return ExitCode::FAILURE;
            }
            println!("{}  {target}  ({} bytes)", sha256_hex(&bytes), bytes.len());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("cannot pack {source}: {err}");
            ExitCode::FAILURE
        }
    }
}
