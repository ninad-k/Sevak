//! Release tooling: checks a signed gallery list with the app's own verifier
//! before the signature is attached to a release.
//!
//! cargo run -p sevak-plugins --example verify_gallery_index -- <tag> <list> <sig>
//!
//! `<list>` is `gallery/index.json` or `gallery/themes.json` exactly as committed
//! at the release commit; its file name selects the policy. The signature must
//! be authenticated by the embedded updater key and bound to `<tag>` and that
//! list, and the list must parse with the app's parser.
use sevak_core::gallery_source::Pin;
use sevak_core::theme_store;
use sevak_plugins::index_signature::{verify_for_build, MAX_SIGNATURE_BYTES};
use sevak_plugins::workflow::gallery;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [tag, list, sig] = args.as_slice() else {
        return Err(
            "usage: verify_gallery_index <tag> <gallery/index.json|gallery/themes.json> <sig>"
                .into(),
        );
    };
    let list = Path::new(list);
    let file = list
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("the list has no file name")?;
    let max = match file {
        gallery::INDEX_FILE => gallery::MAX_INDEX_BYTES,
        theme_store::GALLERY_INDEX_FILE => theme_store::MAX_INDEX_BYTES as usize,
        other => return Err(format!("{other} is not a signed gallery list").into()),
    };
    // One byte over the limit, so an oversized list fails the verifier, not the read.
    let body = sevak_core::bounded_read::read_capped(list, max as u64 + 1)?;
    let signature =
        sevak_core::bounded_read::read_capped(Path::new(sig), MAX_SIGNATURE_BYTES as u64 + 1)?;
    verify_for_build(tag, file, &body, &signature)?;
    // A beta (v1.3.0-beta.1) is parsed as its release number; only the paths
    // in the list depend on the tag.
    let pin = Pin::new(tag)
        .or_else(|| Pin::new(tag.split_once('-')?.0))
        .ok_or_else(|| format!("{tag} is not a release tag"))?;
    let text = std::str::from_utf8(&body)?;
    let entries = if file == gallery::INDEX_FILE {
        let index = gallery::parse_index(text, &pin)?;
        if !index.skipped.is_empty() {
            return Err(format!("{file} has invalid entries: {:?}", index.skipped).into());
        }
        index.entries.len()
    } else {
        theme_store::parse_index(text, &pin)?.len()
    };
    println!("Verified the signature of {file} at {tag} ({entries} entries).");
    Ok(())
}
