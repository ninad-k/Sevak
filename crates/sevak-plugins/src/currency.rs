//! Opt-in fiat currency conversion for the calculator (`100 usd in eur`).
//!
//! Rates are the European Central Bank's daily euro foreign exchange reference
//! rates, one small XML file with no account or key. The only network request
//! Sevak makes for this is a `GET` of [`ECB_URL`], and only when
//! `[calculator] currency = true`:
//!
//! - it runs on its own background thread, started from [`Plugin::refresh`]
//!   (never from the typing path), so searching is never blocked by it;
//! - at most once per 24 hours: the rates are cached as JSON in the data
//!   directory and reused across restarts and "Reload index";
//! - after a failure it waits an hour before trying again.
//!
//! Until the first download finishes, a currency query shows a "Fetching
//! exchange rates…" row instead of nothing.
//!
//! [`Plugin::refresh`]: sevak_core::Plugin::refresh

use std::collections::BTreeMap;
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::calculator::{evaluate, format_number};
use crate::units::{round_significant, split_conversion, split_quantity};

/// The only URL Sevak requests for currency rates.
pub const ECB_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";
/// File name of the rate cache inside the data directory.
pub const CACHE_FILE: &str = "currency-rates.json";

/// Rates younger than this are not downloaded again.
const MAX_AGE_SECS: u64 = 24 * 60 * 60;
/// Wait this long before retrying after a failed download.
const RETRY_AFTER_FAILURE_SECS: u64 = 60 * 60;
/// The real file is about 1.5 KB; anything much bigger is not what we asked for.
const MAX_BODY_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// The currencies the ECB publishes (plus the euro itself), with their names.
pub const CURRENCIES: &[(&str, &str)] = &[
    ("EUR", "Euro"),
    ("USD", "US dollar"),
    ("JPY", "Japanese yen"),
    ("BGN", "Bulgarian lev"),
    ("CZK", "Czech koruna"),
    ("DKK", "Danish krone"),
    ("GBP", "Pound sterling"),
    ("HUF", "Hungarian forint"),
    ("PLN", "Polish zloty"),
    ("RON", "Romanian leu"),
    ("SEK", "Swedish krona"),
    ("CHF", "Swiss franc"),
    ("ISK", "Icelandic krona"),
    ("NOK", "Norwegian krone"),
    ("TRY", "Turkish lira"),
    ("AUD", "Australian dollar"),
    ("BRL", "Brazilian real"),
    ("CAD", "Canadian dollar"),
    ("CNY", "Chinese yuan"),
    ("HKD", "Hong Kong dollar"),
    ("IDR", "Indonesian rupiah"),
    ("ILS", "Israeli shekel"),
    ("INR", "Indian rupee"),
    ("KRW", "South Korean won"),
    ("MXN", "Mexican peso"),
    ("MYR", "Malaysian ringgit"),
    ("NZD", "New Zealand dollar"),
    ("PHP", "Philippine peso"),
    ("SGD", "Singapore dollar"),
    ("THB", "Thai baht"),
    ("ZAR", "South African rand"),
];

/// Currency signs and common words, longest first so `US$` wins over `$`.
/// `$` is the US dollar and `¥` the yen: pick a code (`cad`, `cny`) for others.
const SYMBOLS: &[(&str, &str)] = &[
    ("us$", "USD"),
    ("nz$", "NZD"),
    ("hk$", "HKD"),
    ("a$", "AUD"),
    ("c$", "CAD"),
    ("s$", "SGD"),
    ("r$", "BRL"),
    ("€", "EUR"),
    ("$", "USD"),
    ("£", "GBP"),
    ("¥", "JPY"),
    ("₹", "INR"),
    ("₩", "KRW"),
    ("₺", "TRY"),
    ("₪", "ILS"),
    ("฿", "THB"),
    ("₱", "PHP"),
    ("zł", "PLN"),
    ("kč", "CZK"),
];

const WORDS: &[(&str, &str)] = &[
    ("euro", "EUR"),
    ("euros", "EUR"),
    ("dollar", "USD"),
    ("dollars", "USD"),
    ("pound", "GBP"),
    ("pounds", "GBP"),
    ("sterling", "GBP"),
    ("yen", "JPY"),
    ("rupee", "INR"),
    ("rupees", "INR"),
    ("yuan", "CNY"),
    ("renminbi", "CNY"),
];

// ---------------------------------------------------------------------------
// Rates
// ---------------------------------------------------------------------------

/// One day's reference rates: how many units of each currency one euro buys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rates {
    /// The ECB's publication date (`2026-10-02`), shown in the result.
    pub date: String,
    /// When Sevak downloaded them (Unix seconds); decides when to refresh.
    pub fetched_at: u64,
    rates: BTreeMap<String, f64>,
}

fn is_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())
}

fn is_date(date: &str) -> bool {
    let b = date.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// The value of `name='...'` (or `"..."`) inside one tag's attributes.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(found) = tag[from..].find(name) {
        let start = from + found;
        let after = start + name.len();
        let boundary = tag[..start]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        if boundary && tag[after..].starts_with('=') {
            let rest = &tag[after + 1..];
            let quote = rest.chars().next().filter(|q| matches!(q, '\'' | '"'))?;
            let value = &rest[1..];
            return value.find(quote).map(|end| &value[..end]);
        }
        from = after;
    }
    None
}

impl Rates {
    /// Parses the ECB daily XML. A small scanner is enough: the feed is flat
    /// `<Cube time='...'>` and `<Cube currency='USD' rate='1.1225'/>` tags.
    pub fn parse_ecb_xml(xml: &str, fetched_at: u64) -> Result<Self, String> {
        let mut date = None;
        let mut rates = BTreeMap::new();
        let mut rest = xml;
        while let Some(start) = rest.find("<Cube") {
            rest = &rest[start + "<Cube".len()..];
            let Some(end) = rest.find('>') else { break };
            let tag = &rest[..end];
            if date.is_none() {
                date = attribute(tag, "time");
            }
            if let (Some(code), Some(rate)) = (attribute(tag, "currency"), attribute(tag, "rate")) {
                let rate: f64 = rate
                    .trim()
                    .parse()
                    .map_err(|_| format!("bad rate `{rate}` for {code}"))?;
                if !is_code(code) || !rate.is_finite() || rate <= 0.0 {
                    return Err(format!("bad rate {rate} for `{code}`"));
                }
                rates.insert(code.to_owned(), rate);
            }
            rest = &rest[end..];
        }
        let date = date.ok_or("no publication date in the rates file")?;
        let parsed = Self {
            date: date.to_owned(),
            fetched_at,
            rates,
        };
        parsed.validate()?;
        Ok(parsed)
    }

    /// Rejects anything a cache file or download should not contain.
    fn validate(&self) -> Result<(), String> {
        if !is_date(&self.date) {
            return Err(format!("bad date `{}`", self.date));
        }
        if self.rates.is_empty() {
            return Err("no exchange rates in the file".to_owned());
        }
        if let Some((code, rate)) = self
            .rates
            .iter()
            .find(|(code, rate)| !is_code(code) || !rate.is_finite() || **rate <= 0.0)
        {
            return Err(format!("bad rate {rate} for `{code}`"));
        }
        Ok(())
    }

    /// Units of `code` per euro; the euro itself is `1`.
    fn per_euro(&self, code: &str) -> Option<f64> {
        if code == "EUR" {
            Some(1.0)
        } else {
            self.rates.get(code).copied()
        }
    }

    /// How many `to` one `from` buys (`1 USD = 0.89 EUR`).
    pub fn rate(&self, from: &str, to: &str) -> Option<f64> {
        Some(self.per_euro(to)? / self.per_euro(from)?)
    }

    /// `amount` of `from` expressed in `to`; `None` if either is not published.
    pub fn convert(&self, amount: f64, from: &str, to: &str) -> Option<f64> {
        let value = amount * self.rate(from, to)?;
        value.is_finite().then_some(value)
    }

    /// Currency codes this set can convert, euro included.
    pub fn codes(&self) -> Vec<&str> {
        let mut codes: Vec<&str> = self.rates.keys().map(String::as_str).collect();
        codes.push("EUR");
        codes.sort_unstable();
        codes
    }
}

// ---------------------------------------------------------------------------
// Downloading and caching
// ---------------------------------------------------------------------------

type Fetcher = Box<dyn Fn() -> Result<String, String> + Send + Sync>;

/// What a currency query can currently be answered with.
#[derive(Debug, Clone)]
pub enum Availability {
    Ready(Arc<Rates>),
    /// No rates yet; the first download is pending or running.
    Fetching,
    /// No rates, and the last download failed (offline?). Retried later.
    Unavailable,
}

#[derive(Default)]
struct State {
    rates: Option<Arc<Rates>>,
    cache_loaded: bool,
    fetching: bool,
    last_failure: Option<u64>,
}

/// Holds the current rates and keeps them fresh. Shared between the calculator
/// plugin (reads, on the typing path) and its `refresh` (writes, in the background).
pub struct RateService {
    cache_path: PathBuf,
    fetch: Fetcher,
    state: Mutex<State>,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl RateService {
    /// A service that downloads from the ECB and caches at `cache_path`
    /// (normally `<data dir>/currency-rates.json`).
    pub fn new(cache_path: PathBuf) -> Arc<Self> {
        Self::with_fetcher(cache_path, Box::new(download))
    }

    /// Like [`RateService::new`] with a custom download step (used by tests,
    /// which must never touch the network).
    pub fn with_fetcher(cache_path: PathBuf, fetch: Fetcher) -> Arc<Self> {
        Arc::new(Self {
            cache_path,
            fetch,
            state: Mutex::new(State::default()),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Cheap, non-blocking view for the typing path.
    pub fn availability(&self) -> Availability {
        let state = self.state();
        match &state.rates {
            Some(rates) => Availability::Ready(rates.clone()),
            None if state.fetching || !state.cache_loaded || state.last_failure.is_none() => {
                Availability::Fetching
            }
            None => Availability::Unavailable,
        }
    }

    /// Loads the cache on first use and refreshes stale rates on a background
    /// thread. Returns immediately.
    pub fn refresh(self: &Arc<Self>) {
        let now = unix_now();
        if !self.prepare(now) {
            return;
        }
        let service = self.clone();
        let spawned = std::thread::Builder::new()
            .name("sevak-rates".to_owned())
            .spawn(move || service.download_and_store(now));
        if let Err(err) = spawned {
            tracing::warn!("could not start the exchange rate download: {err}");
            self.finish(now, Err(err.to_string()));
        }
    }

    /// The same as [`RateService::refresh`] but runs the download on the calling
    /// thread, with an explicit clock.
    pub fn refresh_blocking(&self, now: u64) {
        if self.prepare(now) {
            self.download_and_store(now);
        }
    }

    /// Reads the cache once, then decides whether a download is due. If so,
    /// marks one as running and returns `true`.
    fn prepare(&self, now: u64) -> bool {
        let mut state = self.state();
        if !state.cache_loaded {
            state.cache_loaded = true;
            match self.load_cache() {
                Ok(Some(rates)) => state.rates = Some(Arc::new(rates)),
                Ok(None) => {}
                Err(err) => tracing::warn!("ignoring the exchange rate cache: {err}"),
            }
        }
        if state.fetching {
            return false;
        }
        let fresh = state
            .rates
            .as_ref()
            .is_some_and(|r| now.saturating_sub(r.fetched_at) < MAX_AGE_SECS);
        let backing_off = state
            .last_failure
            .is_some_and(|t| now.saturating_sub(t) < RETRY_AFTER_FAILURE_SECS);
        if fresh || backing_off {
            return false;
        }
        state.fetching = true;
        true
    }

    fn download_and_store(&self, now: u64) {
        // A panic must not leave `fetching` set forever.
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let xml = (self.fetch)()?;
            Rates::parse_ecb_xml(&xml, now)
        }))
        .unwrap_or_else(|_| Err("the download panicked".to_owned()));
        self.finish(now, outcome);
    }

    fn finish(&self, now: u64, outcome: Result<Rates, String>) {
        match outcome {
            Ok(rates) => {
                if let Err(err) = self.save_cache(&rates) {
                    tracing::warn!("could not save the exchange rates: {err}");
                }
                tracing::info!(date = %rates.date, "exchange rates updated");
                let mut state = self.state();
                state.rates = Some(Arc::new(rates));
                state.last_failure = None;
                state.fetching = false;
            }
            Err(err) => {
                tracing::warn!("could not download exchange rates: {err}");
                let mut state = self.state();
                state.last_failure = Some(now);
                state.fetching = false;
            }
        }
    }

    fn load_cache(&self) -> Result<Option<Rates>, String> {
        let text = match std::fs::read_to_string(&self.cache_path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.to_string()),
        };
        let rates: Rates = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        rates.validate()?;
        Ok(Some(rates))
    }

    /// Writes the cache atomically (temp file, then rename).
    fn save_cache(&self, rates: &Rates) -> Result<(), String> {
        let text = serde_json::to_string(rates).map_err(|e| e.to_string())?;
        if let Some(dir) = self.cache_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let temp = self.cache_path.with_extension("json.tmp");
        std::fs::write(&temp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&temp, &self.cache_path).map_err(|e| e.to_string())
    }
}

/// The real download: one `GET` of [`ECB_URL`], no cookies, no identifying
/// headers, a hard size limit and a timeout.
fn download() -> Result<String, String> {
    // reqwest builds its TLS config from the process-wide rustls provider; the
    // updater installs the same one lazily, so do it here too before it has run.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        // Like the gallery downloads (`crate::net`): a redirect may not leave
        // https, and there are not many of them.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" {
                attempt.error("a redirect to a non-https address")
            } else if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(ECB_URL)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| e.to_string())?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BODY_BYTES as u64)
    {
        return Err("the rates file is unexpectedly large".to_owned());
    }
    read_limited(&mut response, MAX_BODY_BYTES)
}

/// Reads `reader` as UTF-8 text, refusing more than `max` bytes. It reads at
/// most one byte over, because without a Content-Length header (chunked) the
/// whole body would otherwise be buffered before any check.
fn read_limited(reader: &mut impl Read, max: usize) -> Result<String, String> {
    let mut body = Vec::new();
    Read::take(reader, max as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() > max {
        return Err("the rates file is unexpectedly large".to_owned());
    }
    String::from_utf8(body).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Parsing and answering queries
// ---------------------------------------------------------------------------

/// A parsed `100 usd in eur`.
#[derive(Debug, Clone, PartialEq)]
struct Request {
    amount: f64,
    from: &'static str,
    to: &'static str,
    /// The query as typed, for the subtitle.
    shown: String,
}

/// The ISO code for `text` (`usd`, `€`, `euros`), if it names a currency the
/// ECB publishes.
fn currency_code(text: &str) -> Option<&'static str> {
    let text = text.trim().trim_end_matches('.');
    let lower = text.to_lowercase();
    if text.chars().count() == 3 && text.chars().all(char::is_alphabetic) {
        let upper = text.to_uppercase();
        if let Some((code, _)) = CURRENCIES.iter().find(|(code, _)| *code == upper) {
            return Some(code);
        }
    }
    SYMBOLS
        .iter()
        .chain(WORDS)
        .find(|(spelling, _)| *spelling == lower)
        .map(|(_, code)| *code)
}

/// `$100`, `€ 50`, `US$5`: a sign before the amount.
fn prefixed_amount(left: &str) -> Option<(f64, &'static str)> {
    let lower = left.to_lowercase();
    SYMBOLS.iter().find_map(|(sign, code)| {
        let rest = left.get(lower.strip_prefix(sign).map(|_| sign.len())?..)?;
        let amount = evaluate(rest.trim()).ok()?;
        Some((amount, *code))
    })
}

fn parse(input: &str) -> Option<Request> {
    for (left, right) in split_conversion(input) {
        let Some(to) = currency_code(right) else {
            continue;
        };
        let from = split_quantity(left)
            .into_iter()
            .find_map(|(amount, unit)| Some((amount, currency_code(unit)?)))
            .or_else(|| prefixed_amount(left));
        if let Some((amount, from)) = from {
            return Some(Request {
                amount,
                from,
                to,
                shown: format!("{left} → {right}"),
            });
        }
    }
    None
}

/// A row to show for a currency query.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub title: String,
    pub subtitle: String,
    /// `true` for a conversion (Enter copies the title); `false` for a status
    /// row ("Fetching exchange rates…") that has nothing to copy.
    pub copyable: bool,
}

/// Formats money: two decimals (halves round away from zero, as in a shop)
/// for amounts of at least one, four significant digits for smaller ones.
fn format_money(value: f64) -> String {
    if value.abs() >= 1.0 {
        format!("{:.2}", (value * 100.0).round() / 100.0)
    } else {
        format_number(round_significant(value, 4))
    }
}

/// Answers `<amount> <currency> (in|to|as|=) <currency>`; `None` for any
/// other query.
pub fn answer(input: &str, availability: &Availability) -> Option<Answer> {
    let request = parse(input)?;
    Some(match availability {
        Availability::Ready(rates) => {
            let (Some(value), Some(rate)) = (
                rates.convert(request.amount, request.from, request.to),
                rates.rate(request.from, request.to),
            ) else {
                return Some(Answer {
                    title: "Exchange rate not available".to_owned(),
                    subtitle: format!(
                        "{} · the ECB does not publish {} or {}",
                        request.shown, request.from, request.to
                    ),
                    copyable: false,
                });
            };
            Answer {
                title: format!("{} {}", format_money(value), request.to),
                subtitle: format!(
                    "{} · 1 {} = {} {} · ECB {}",
                    request.shown,
                    request.from,
                    format_number(round_significant(rate, 6)),
                    request.to,
                    rates.date
                ),
                copyable: true,
            }
        }
        Availability::Fetching => Answer {
            title: "Fetching exchange rates…".to_owned(),
            subtitle: format!(
                "{} · downloading the ECB's daily rates, try again in a moment",
                request.shown
            ),
            copyable: false,
        },
        Availability::Unavailable => Answer {
            title: "Exchange rates unavailable".to_owned(),
            subtitle: format!(
                "{} · could not download the ECB's daily rates; Sevak will retry",
                request.shown
            ),
            copyable: false,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn a_download_over_the_limit_is_refused_without_reading_it_all() {
        let mut exact = std::io::repeat(b'a').take(100);
        assert_eq!(read_limited(&mut exact, 100).unwrap().len(), 100);
        // An endless body ends at the limit instead of filling memory.
        let mut endless = std::io::repeat(b'a');
        assert!(read_limited(&mut endless, 100).is_err());
        assert!(read_limited(&mut &[0xff_u8, 0xfe][..], 100).is_err());
    }

    /// A trimmed copy of the real ECB feed (2026-10-02).
    pub(crate) const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01" xmlns="http://www.ecb.int/vocabulary/2002-08-01/eurofxref">
	<gesmes:subject>Reference rates</gesmes:subject>
	<gesmes:Sender>
		<gesmes:name>European Central Bank</gesmes:name>
	</gesmes:Sender>
	<Cube>
		<Cube time='2026-10-02'>
			<Cube currency='USD' rate='1.1225'/>
			<Cube currency='JPY' rate='176.99'/>
			<Cube currency='CZK' rate='24.470'/>
			<Cube currency='GBP' rate='0.85033'/>
			<Cube currency='PLN' rate='4.3775'/>
			<Cube currency='CHF' rate='0.9279'/>
			<Cube currency='CAD' rate='1.5984'/>
			<Cube currency='INR' rate='108.1245'/>
			<Cube currency='KRW' rate='1513.44'/>
		</Cube>
	</Cube>
</gesmes:Envelope>"#;

    fn rates() -> Arc<Rates> {
        Arc::new(Rates::parse_ecb_xml(FIXTURE, 1_000).unwrap())
    }

    fn ready() -> Availability {
        Availability::Ready(rates())
    }

    fn title(input: &str) -> String {
        answer(input, &ready())
            .unwrap_or_else(|| panic!("`{input}` should convert"))
            .title
    }

    #[test]
    fn parses_the_ecb_feed() {
        let r = rates();
        assert_eq!(r.date, "2026-10-02");
        assert_eq!(r.fetched_at, 1_000);
        assert_eq!(r.per_euro("USD"), Some(1.1225));
        assert_eq!(r.per_euro("CZK"), Some(24.47));
        assert_eq!(r.per_euro("EUR"), Some(1.0));
        assert_eq!(r.per_euro("XXX"), None);
        assert_eq!(
            r.codes(),
            ["CAD", "CHF", "CZK", "EUR", "GBP", "INR", "JPY", "KRW", "PLN", "USD"]
        );
    }

    #[test]
    fn parses_double_quoted_attributes_and_other_layouts() {
        let xml = r#"<Cube><Cube time="2026-01-02"><Cube rate="2" currency="USD"/></Cube></Cube>"#;
        let r = Rates::parse_ecb_xml(xml, 5).unwrap();
        assert_eq!(r.date, "2026-01-02");
        assert_eq!(r.per_euro("USD"), Some(2.0));
    }

    #[test]
    fn rejects_bad_feeds() {
        for xml in [
            "",
            "<html>Service unavailable</html>",
            "<Cube><Cube time='2026-10-02'></Cube></Cube>",
            "<Cube time='2026-10-02'><Cube currency='USD' rate='abc'/></Cube>",
            "<Cube time='2026-10-02'><Cube currency='USD' rate='-1'/></Cube>",
            "<Cube time='2026-10-02'><Cube currency='USD' rate='0'/></Cube>",
            "<Cube time='2026-10-02'><Cube currency='USD' rate='inf'/></Cube>",
            "<Cube time='2026-10-02'><Cube currency='usd' rate='1'/></Cube>",
            "<Cube time='yesterday'><Cube currency='USD' rate='1'/></Cube>",
            "<Cube currency='USD' rate='1'/>",
            "<Cube time='2026-10-02'><Cube currency='USD' rate='1'",
        ] {
            assert!(Rates::parse_ecb_xml(xml, 0).is_err(), "accepted: {xml}");
        }
    }

    #[test]
    fn currency_maths() {
        let r = rates();
        // USD -> EUR divides, EUR -> USD multiplies, crosses go through the euro.
        assert!((r.convert(100.0, "USD", "EUR").unwrap() - 100.0 / 1.1225).abs() < 1e-9);
        assert!((r.convert(100.0, "EUR", "USD").unwrap() - 112.25).abs() < 1e-9);
        assert!((r.convert(100.0, "USD", "GBP").unwrap() - 100.0 / 1.1225 * 0.85033).abs() < 1e-9);
        assert_eq!(r.convert(7.0, "EUR", "EUR"), Some(7.0));
        assert_eq!(r.convert(7.0, "USD", "USD"), Some(7.0));
        assert_eq!(r.convert(1.0, "USD", "XXX"), None);
        assert_eq!(r.convert(1.0, "XXX", "USD"), None);
        // Round trips come back.
        let there = r.convert(250.0, "GBP", "JPY").unwrap();
        assert!((r.convert(there, "JPY", "GBP").unwrap() - 250.0).abs() < 1e-9);
        assert!((r.rate("EUR", "USD").unwrap() - 1.1225).abs() < 1e-12);
        assert!((r.rate("USD", "EUR").unwrap() - 1.0 / 1.1225).abs() < 1e-12);
    }

    #[test]
    fn answers_with_rate_and_date() {
        let a = answer("100 usd in eur", &ready()).unwrap();
        assert_eq!(a.title, "89.09 EUR");
        assert_eq!(
            a.subtitle,
            "100 usd → eur · 1 USD = 0.890869 EUR · ECB 2026-10-02"
        );
        assert!(a.copyable);
        assert_eq!(title("100 eur in usd"), "112.25 USD");
        assert_eq!(title("100 EUR TO USD"), "112.25 USD");
        assert_eq!(title("100 usd = eur"), "89.09 EUR");
        assert_eq!(title("100 usd as eur"), "89.09 EUR");
        assert_eq!(title("1000 jpy to usd"), "6.34 USD");
        assert_eq!(title("1 usd to jpy"), "157.67 JPY");
        assert_eq!(title("0 usd to eur"), "0 EUR");
        assert_eq!(title("10 eur in eur"), "10.00 EUR");
        assert_eq!(title("(50 + 50) usd in eur"), "89.09 EUR");
        assert_eq!(title("2 * 50 usd in eur"), "89.09 EUR");
        assert_eq!(title("-100 usd in eur"), "-89.09 EUR");
        assert_eq!(title("1.5 eur in usd"), "1.68 USD");
    }

    #[test]
    fn small_amounts_keep_significant_digits() {
        assert_eq!(title("1 jpy in usd"), "0.006342 USD");
        assert_eq!(title("0.5 usd in eur"), "0.4454 EUR");
    }

    #[test]
    fn symbols_words_and_prefixes() {
        assert_eq!(title("50 € to $"), "56.13 USD");
        assert_eq!(title("€50 to usd"), "56.13 USD");
        assert_eq!(title("€ 50 to usd"), "56.13 USD");
        assert_eq!(title("$100 in eur"), "89.09 EUR");
        assert_eq!(title("US$100 in eur"), "89.09 EUR");
        assert_eq!(title("100 $ in €"), "89.09 EUR");
        assert_eq!(title("100 usd in £"), "75.75 GBP");
        assert_eq!(title("100 euros to dollars"), "112.25 USD");
        assert_eq!(title("100 dollars in euro"), "89.09 EUR");
        assert_eq!(title("100 pounds to eur"), "117.60 EUR");
        assert_eq!(title("100 zł in eur"), "22.84 EUR");
        assert_eq!(title("100 PLN in EUR"), "22.84 EUR");
        assert_eq!(title("1000 ₩ in eur"), "0.6607 EUR");
        assert_eq!(title("1000 ¥ in $"), "6.34 USD");
    }

    #[test]
    fn status_rows_when_there_are_no_rates() {
        let fetching = answer("100 usd in eur", &Availability::Fetching).unwrap();
        assert_eq!(fetching.title, "Fetching exchange rates…");
        assert!(!fetching.copyable);
        assert!(fetching.subtitle.starts_with("100 usd → eur"));
        let down = answer("100 usd in eur", &Availability::Unavailable).unwrap();
        assert_eq!(down.title, "Exchange rates unavailable");
        assert!(!down.copyable);
    }

    #[test]
    fn unpublished_currencies_are_reported() {
        let a = answer("100 usd in bgn", &ready()).unwrap();
        assert_eq!(a.title, "Exchange rate not available");
        assert!(!a.copyable);
    }

    #[test]
    fn ignores_everything_else() {
        for input in [
            "",
            "100",
            "100 usd",
            "usd in eur",
            "100 km in mi",
            "100 usd in km",
            "100 foo in bar",
            "100 usdd in eur",
            "2+2",
            "go to usd",
            "100 usd in",
        ] {
            assert!(
                answer(input, &ready()).is_none(),
                "`{input}` is not a currency query"
            );
        }
    }

    #[test]
    fn every_listed_currency_parses() {
        for (code, name) in CURRENCIES {
            assert_eq!(currency_code(code), Some(*code));
            assert_eq!(currency_code(&code.to_lowercase()), Some(*code));
            assert!(!name.is_empty());
        }
        assert_eq!(currency_code("btc"), None);
        assert_eq!(currency_code("us"), None);
    }

    // --- the service: cache, freshness, retries; the fetcher is a fake ---

    fn fixture_fetcher(calls: Arc<AtomicUsize>) -> Fetcher {
        Box::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(FIXTURE.to_owned())
        })
    }

    fn failing_fetcher(calls: Arc<AtomicUsize>) -> Fetcher {
        Box::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("offline".to_owned())
        })
    }

    fn cache_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("nested").join(CACHE_FILE)
    }

    #[test]
    fn first_refresh_downloads_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let service = RateService::with_fetcher(cache_path(&dir), fixture_fetcher(calls.clone()));
        assert!(matches!(service.availability(), Availability::Fetching));
        service.refresh_blocking(5_000);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let Availability::Ready(r) = service.availability() else {
            panic!("rates should be ready");
        };
        assert_eq!(r.date, "2026-10-02");
        assert_eq!(r.fetched_at, 5_000);
        assert!(cache_path(&dir).exists());
        assert!(!cache_path(&dir).with_extension("json.tmp").exists());
    }

    #[test]
    fn fresh_rates_are_not_downloaded_again() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let service = RateService::with_fetcher(cache_path(&dir), fixture_fetcher(calls.clone()));
        service.refresh_blocking(5_000);
        service.refresh_blocking(5_000 + MAX_AGE_SECS - 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        service.refresh_blocking(5_000 + MAX_AGE_SECS);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_restart_reuses_the_cache_without_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let first = RateService::with_fetcher(
            cache_path(&dir),
            fixture_fetcher(Arc::new(AtomicUsize::new(0))),
        );
        first.refresh_blocking(5_000);

        let calls = Arc::new(AtomicUsize::new(0));
        let second = RateService::with_fetcher(cache_path(&dir), fixture_fetcher(calls.clone()));
        second.refresh_blocking(5_000 + 3_600);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "the cache is still fresh");
        let Availability::Ready(r) = second.availability() else {
            panic!("the cache should be loaded");
        };
        assert_eq!(r.date, "2026-10-02");
    }

    #[test]
    fn stale_cache_is_used_while_refreshing_and_kept_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        RateService::with_fetcher(
            cache_path(&dir),
            fixture_fetcher(Arc::new(AtomicUsize::new(0))),
        )
        .refresh_blocking(5_000);

        let calls = Arc::new(AtomicUsize::new(0));
        let service = RateService::with_fetcher(cache_path(&dir), failing_fetcher(calls.clone()));
        let later = 5_000 + 3 * MAX_AGE_SECS;
        service.refresh_blocking(later);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "stale rates trigger a download"
        );
        let Availability::Ready(r) = service.availability() else {
            panic!("stale rates are better than none");
        };
        assert_eq!(r.fetched_at, 5_000);
    }

    #[test]
    fn failures_back_off_for_an_hour() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let service = RateService::with_fetcher(cache_path(&dir), failing_fetcher(calls.clone()));
        service.refresh_blocking(10_000);
        assert!(matches!(service.availability(), Availability::Unavailable));
        service.refresh_blocking(10_000 + RETRY_AFTER_FAILURE_SECS - 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        service.refresh_blocking(10_000 + RETRY_AFTER_FAILURE_SECS);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_bad_download_is_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let service = RateService::with_fetcher(
            cache_path(&dir),
            Box::new(|| Ok("<html>captive portal</html>".to_owned())),
        );
        service.refresh_blocking(1);
        assert!(matches!(service.availability(), Availability::Unavailable));
        assert!(!cache_path(&dir).exists());
    }

    #[test]
    fn a_panicking_download_does_not_wedge_the_service() {
        let dir = tempfile::tempdir().unwrap();
        let service = RateService::with_fetcher(cache_path(&dir), Box::new(|| panic!("boom")));
        service.refresh_blocking(1);
        assert!(matches!(service.availability(), Availability::Unavailable));
        assert!(!service.state().fetching);
    }

    #[test]
    fn a_corrupt_cache_is_ignored_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(cache_path(&dir).parent().unwrap()).unwrap();
        std::fs::write(cache_path(&dir), "{ not json").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let service = RateService::with_fetcher(cache_path(&dir), fixture_fetcher(calls.clone()));
        service.refresh_blocking(7);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(service.availability(), Availability::Ready(_)));
        let text = std::fs::read_to_string(cache_path(&dir)).unwrap();
        assert!(text.contains("2026-10-02"));
    }

    #[test]
    fn a_cache_with_nonsense_rates_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(cache_path(&dir).parent().unwrap()).unwrap();
        std::fs::write(
            cache_path(&dir),
            r#"{"date":"2026-10-02","fetched_at":1,"rates":{"USD":-3.0}}"#,
        )
        .unwrap();
        let service = RateService::with_fetcher(cache_path(&dir), failing_fetcher(Arc::default()));
        service.refresh_blocking(2);
        assert!(matches!(service.availability(), Availability::Unavailable));
    }

    /// Talks to the real ECB; run it by hand with
    /// `cargo test -p sevak-plugins -- --ignored real_ecb`.
    #[test]
    #[ignore = "needs the network"]
    fn real_ecb_download_parses() {
        let xml = download().expect("download");
        let rates = Rates::parse_ecb_xml(&xml, 1).expect("parse");
        assert!(rates.codes().len() >= 25, "{:?}", rates.codes());
        assert!(rates.rate("EUR", "USD").unwrap() > 0.0);
    }

    #[test]
    fn the_request_target_is_the_ecb() {
        assert_eq!(
            ECB_URL,
            "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml"
        );
    }
}
