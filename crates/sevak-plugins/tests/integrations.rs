//! The contacts, dictionary and 1Password plugins through the real registry and
//! search engine: keyword routing (`c`, `@`, `define`, `spell`), that nothing
//! leaks into global queries or the search history, and that Enter reaches the
//! platform. A recording platform stands in for the OS.

use std::path::Path;
use std::sync::{Arc, Mutex};

use sevak_core::config::{ContactsConfig, DictionaryConfig, OnePasswordConfig};
use sevak_core::{
    Config, EngineOptions, IconData, IconSource, LaunchTarget, SearchEngine, UsageStore,
};
use sevak_platform::{DeepLink, PlatformError, PlatformProvider, Result as PlatformResult};
use sevak_plugins::PluginRegistry;

#[derive(Default)]
struct Recorder {
    clipboard: Mutex<Vec<String>>,
    urls: Mutex<Vec<String>>,
    links: Mutex<Vec<String>>,
}

impl PlatformProvider for Recorder {
    fn list_applications(&self) -> PlatformResult<Vec<sevak_core::AppEntry>> {
        Ok(Vec::new())
    }
    fn launch(&self, _target: &LaunchTarget) -> PlatformResult<()> {
        Ok(())
    }
    fn open_url(&self, url: &str) -> PlatformResult<()> {
        self.urls.lock().unwrap().push(url.to_owned());
        Ok(())
    }
    fn open_link(&self, link: &DeepLink) -> PlatformResult<()> {
        self.links.lock().unwrap().push(link.as_str().to_owned());
        Ok(())
    }
    fn load_icon(&self, _source: &IconSource, _size: u32) -> PlatformResult<IconData> {
        Err(PlatformError::Unsupported("icons in tests"))
    }
    fn set_clipboard_text(&self, text: &str) -> PlatformResult<()> {
        self.clipboard.lock().unwrap().push(text.to_owned());
        Ok(())
    }
}

const VCARDS: &str = "BEGIN:VCARD\nVERSION:3.0\nFN:Ada Lovelace\nEMAIL:ada@example.org\nTEL:+44 20 7946 0000\nORG:Analytical Engines\nUID:ada\nEND:VCARD\n\
BEGIN:VCARD\nVERSION:3.0\nFN:Grace Hopper\nEMAIL:grace@navy.example\nUID:grace\nEND:VCARD\n";

fn engine(vcf: &Path, platform: &Arc<Recorder>) -> SearchEngine {
    let config = Config {
        contacts: ContactsConfig {
            enabled: true,
            use_system: false,
            vcard_files: vec![vcf.display().to_string()],
            ..ContactsConfig::default()
        },
        dictionary: DictionaryConfig {
            use_system: false,
            ..DictionaryConfig::default()
        },
        onepassword: OnePasswordConfig::default(),
        ..Config::default()
    };
    let plugins = PluginRegistry::builtin().instantiate(&config, platform.clone());
    let engine = SearchEngine::new(plugins, UsageStore::default(), EngineOptions::default());
    assert!(
        engine.refresh_all().is_empty(),
        "a plugin failed to refresh"
    );
    engine
}

fn titles(engine: &SearchEngine, query: &str) -> Vec<String> {
    engine.query(query).into_iter().map(|r| r.title).collect()
}

#[test]
fn contacts_answer_both_keywords_and_never_global_queries() {
    let dir = tempfile::tempdir().unwrap();
    let vcf = dir.path().join("people.vcf");
    std::fs::write(&vcf, VCARDS).unwrap();
    let platform = Arc::new(Recorder::default());
    let engine = engine(&vcf, &platform);

    assert_eq!(titles(&engine, "c ada")[0], "Ada Lovelace");
    assert_eq!(titles(&engine, "C ada")[0], "Ada Lovelace");
    // The symbol keyword needs no space.
    assert_eq!(titles(&engine, "@hopper")[0], "Grace Hopper");
    assert_eq!(titles(&engine, "@ hopper")[0], "Grace Hopper");
    // A plain query never shows contacts.
    assert!(!titles(&engine, "ada lovelace").contains(&"Ada Lovelace".to_owned()));
    assert!(!titles(&engine, "grace@navy.example").contains(&"Grace Hopper".to_owned()));
}

#[test]
fn picking_a_contact_reaches_the_platform_and_leaves_no_trace() {
    let dir = tempfile::tempdir().unwrap();
    let vcf = dir.path().join("people.vcf");
    std::fs::write(&vcf, VCARDS).unwrap();
    let platform = Arc::new(Recorder::default());
    let engine = engine(&vcf, &platform);

    let ada = engine.query("c ada").remove(0);
    engine.execute(&ada, "c ada").unwrap();
    assert_eq!(*platform.clipboard.lock().unwrap(), ["ada@example.org"]);

    let compose = ada
        .secondary
        .iter()
        .position(|s| s.label == "Write an email");
    engine
        .execute_secondary(&ada, compose.unwrap(), "c ada")
        .unwrap();
    assert_eq!(*platform.urls.lock().unwrap(), ["mailto:ada@example.org"]);

    let call = ada
        .secondary
        .iter()
        .position(|s| s.label == "Call")
        .unwrap();
    engine.execute_secondary(&ada, call, "c ada").unwrap();
    assert_eq!(*platform.links.lock().unwrap(), ["tel:+442079460000"]);

    // Neither the query nor the contact is in the search history.
    assert!(engine.history().is_empty(), "{:?}", engine.history());
}

#[test]
fn dictionary_keywords_define_and_correct_offline() {
    let dir = tempfile::tempdir().unwrap();
    let vcf = dir.path().join("none.vcf");
    let platform = Arc::new(Recorder::default());
    let engine = engine(&vcf, &platform);

    let define = engine.query("define dictionary");
    assert!(
        define[0].title.contains("alphabetical"),
        "{}",
        define[0].title
    );
    engine.execute(&define[0], "define dictionary").unwrap();
    assert_eq!(platform.clipboard.lock().unwrap().len(), 1);

    let spell = engine.query("spell recieve");
    assert_eq!(spell[0].title, "receive");
    // Pasting is not available on the recording platform, so Enter copies.
    engine.execute(&spell[0], "spell recieve").unwrap();
    assert_eq!(
        platform.clipboard.lock().unwrap().last().unwrap(),
        "receive"
    );
    assert!(engine.history().is_empty());

    // The bare keywords are offered for Tab completion and are not global.
    let hint = engine.query("define");
    assert_eq!(
        hint.last().unwrap().autocomplete.as_deref(),
        Some("define ")
    );
    assert!(!titles(&engine, "dictionary")
        .iter()
        .any(|t| t.contains("alphabetical")));
}

#[test]
fn onepassword_is_off_until_enabled_and_never_starts_op_for_other_queries() {
    let dir = tempfile::tempdir().unwrap();
    let platform = Arc::new(Recorder::default());
    let engine = engine(&dir.path().join("none.vcf"), &platform);
    let rows = engine.query("1p github");
    assert_eq!(rows[0].title, "1Password is off");
    // Nothing for the keyword without a space, and nothing globally.
    assert!(!titles(&engine, "github").contains(&"1Password is off".to_owned()));
}

/// Asks the real operating system: Windows' spell checker answers `spell`, and
/// macOS' Dictionary answers `define`. Run with `cargo test -p sevak-plugins
/// --test integrations -- --ignored --nocapture os_dictionary`.
#[test]
#[ignore = "uses the real OS spell checker or dictionary"]
fn os_dictionary() {
    let platform: Arc<dyn PlatformProvider> = Arc::from(sevak_platform::native_provider());
    let plugins = PluginRegistry::builtin().instantiate(&Config::default(), platform);
    let engine = SearchEngine::new(plugins, UsageStore::default(), EngineOptions::default());
    // The Windows checker starts on the first call and may miss that keystroke.
    let mut rows = Vec::new();
    for _ in 0..20 {
        rows = titles(&engine, "spell recieve");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    println!("spell recieve: {rows:?}");
    assert!(rows.contains(&"receive".to_owned()));
    println!(
        "define dictionary: {:?}",
        titles(&engine, "define dictionary")
    );
}
