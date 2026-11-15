use sevak_plugins::extensions::revocations::{verify_for_build, MAX_BYTES, MAX_SIGNATURE_BYTES};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: verify_extension_advisories <json> <sig>".into());
    }
    let body =
        sevak_core::bounded_read::read_capped(std::path::Path::new(&args[0]), MAX_BYTES as u64)?;
    let signature = sevak_core::bounded_read::read_capped(
        std::path::Path::new(&args[1]),
        MAX_SIGNATURE_BYTES as u64,
    )?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let feed = verify_for_build(&body, &signature, now)?;
    println!("Verified {} extension revocations.", feed.revocations.len());
    Ok(())
}
