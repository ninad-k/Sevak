//! Keyword-triggered web search (`g rust traits`).

use std::sync::Arc;

use sevak_core::config::WebSearchEngine;
use sevak_core::model::score;
use sevak_core::{Action, IconSource, Modifier, Plugin, PluginResult, ResultItem};
use sevak_platform::PlatformProvider;

use crate::actions::execute_action;

const PLACEHOLDER: &str = "{query}";

/// Percent-encodes `input` for use inside a URL query component: everything
/// except the unreserved characters `A-Za-z0-9-._~` becomes `%XX` (uppercase
/// hex) over its UTF-8 bytes. Spaces become `%20`.
pub fn percent_encode(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }
    out
}

/// One search engine from `[[web_search]]`.
pub struct WebSearchPlugin {
    id: String,
    name: String,
    description: String,
    keyword: String,
    url_template: String,
    platform: Arc<dyn PlatformProvider>,
}

impl WebSearchPlugin {
    pub fn new(engine: &WebSearchEngine, platform: Arc<dyn PlatformProvider>) -> Self {
        let keyword = engine.keyword.trim().to_owned();
        Self {
            id: format!("web:{keyword}"),
            name: engine.name.clone(),
            description: format!("Search {} by typing `{keyword} <terms>`.", engine.name),
            keyword,
            url_template: engine.url.clone(),
            platform,
        }
    }

    fn url_for(&self, terms: &str) -> String {
        self.url_template
            .replace(PLACEHOLDER, &percent_encode(terms))
    }

    /// The row for a keyword with no terms yet; Enter opens the engine's page.
    fn home_row(&self, subtitle: &str) -> ResultItem {
        ResultItem::new(
            &self.id,
            "home",
            format!("Search {}", self.name),
            Action::OpenUrl {
                url: self.url_for(""),
            },
        )
        .with_subtitle(subtitle)
        .with_icon(IconSource::builtin("web"))
        .with_score(score::KEYWORD)
    }
}

impl Plugin for WebSearchPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn keyword(&self) -> Option<&str> {
        Some(&self.keyword)
    }

    fn global(&self) -> bool {
        false
    }

    fn keyword_row(&self) -> Option<ResultItem> {
        // Tab turns the bare keyword (`g`) into `g `, ready for search terms.
        Some(
            self.home_row("Press Tab to type your search terms")
                .with_autocomplete(format!("{} ", self.keyword)),
        )
    }

    fn query(&self, input: &str) -> Vec<ResultItem> {
        let terms = input.trim();

        if terms.is_empty() {
            return vec![self.home_row("Type your search terms")];
        }

        let url = self.url_for(terms);
        vec![ResultItem::new(
            &self.id,
            "search",
            format!("Search {} for \u{201c}{terms}\u{201d}", self.name),
            Action::OpenUrl { url: url.clone() },
        )
        .with_secondary(
            "Copy URL",
            Some(Modifier::Shift),
            Action::CopyText { text: url.clone() },
        )
        .with_subtitle(url)
        .with_icon(IconSource::builtin("web"))
        .with_score(score::KEYWORD)]
    }

    fn execute(&self, item: &ResultItem) -> PluginResult<()> {
        execute_action(self.platform.as_ref(), &item.action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::MockPlatform;

    fn google(platform: Arc<MockPlatform>) -> WebSearchPlugin {
        WebSearchPlugin::new(
            &WebSearchEngine {
                keyword: "g".into(),
                name: "Google".into(),
                url: "https://www.google.com/search?q={query}".into(),
            },
            platform,
        )
    }

    #[test]
    fn encodes_spaces_and_reserved_characters() {
        assert_eq!(percent_encode("rust traits"), "rust%20traits");
        assert_eq!(percent_encode("a&b"), "a%26b");
        assert_eq!(percent_encode("c#"), "c%23");
        assert_eq!(percent_encode("1+1=2"), "1%2B1%3D2");
        assert_eq!(percent_encode("100%"), "100%25");
        assert_eq!(percent_encode("a/b?c"), "a%2Fb%3Fc");
        assert_eq!(percent_encode("\"quoted\""), "%22quoted%22");
    }

    #[test]
    fn keeps_unreserved_characters() {
        assert_eq!(percent_encode("AZaz09-._~"), "AZaz09-._~");
        assert_eq!(percent_encode(""), "");
    }

    #[test]
    fn encodes_unicode_as_uppercase_utf8_bytes() {
        assert_eq!(percent_encode("日本"), "%E6%97%A5%E6%9C%AC");
        assert_eq!(percent_encode("€"), "%E2%82%AC");
        assert_eq!(percent_encode("café"), "caf%C3%A9");
        assert_eq!(percent_encode("😀"), "%F0%9F%98%80");
    }

    #[test]
    fn plugin_metadata() {
        let plugin = google(MockPlatform::empty());
        assert_eq!(plugin.id(), "web:g");
        assert_eq!(plugin.name(), "Google");
        assert_eq!(plugin.keyword(), Some("g"));
        assert!(!plugin.global());
    }

    #[test]
    fn builds_search_result() {
        let plugin = google(MockPlatform::empty());
        let results = plugin.query("  rust & traits ");
        assert_eq!(results.len(), 1);
        let item = &results[0];
        let url = "https://www.google.com/search?q=rust%20%26%20traits";
        assert_eq!(item.id, "web:g:search");
        assert_eq!(
            item.title,
            "Search Google for \u{201c}rust & traits\u{201d}"
        );
        assert_eq!(item.subtitle, url);
        assert_eq!(item.icon, Some(IconSource::builtin("web")));
        assert_eq!(item.score, score::KEYWORD);
        assert_eq!(item.action, Action::OpenUrl { url: url.into() });
    }

    #[test]
    fn search_result_can_copy_its_url() {
        let platform = MockPlatform::empty();
        let plugin = google(platform.clone());
        let item = plugin.query("rust").remove(0);
        assert_eq!(item.secondary.len(), 1);
        assert_eq!(item.secondary[0].label, "Copy URL");
        assert_eq!(item.secondary[0].modifier, Some(Modifier::Shift));
        assert_eq!(
            item.secondary[0].action,
            Action::CopyText {
                text: "https://www.google.com/search?q=rust".into()
            }
        );
        // The Ctrl+C text is the URL as well.
        assert_eq!(
            item.copy_text().as_deref(),
            Some("https://www.google.com/search?q=rust")
        );
    }

    #[test]
    fn empty_input_gives_a_hint_that_opens_the_homepage() {
        let plugin = google(MockPlatform::empty());
        for input in ["", "   "] {
            let results = plugin.query(input);
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].title, "Search Google");
            assert_eq!(results[0].subtitle, "Type your search terms");
            assert_eq!(
                results[0].action,
                Action::OpenUrl {
                    url: "https://www.google.com/search?q=".into()
                }
            );
        }
    }

    #[test]
    fn bare_keyword_row_completes_to_the_keyword_and_a_space() {
        let plugin = google(MockPlatform::empty());
        let row = plugin.keyword_row().unwrap();
        assert_eq!(row.id, "web:g:home");
        assert_eq!(row.autocomplete.as_deref(), Some("g "));
        assert_eq!(row.action, plugin.query("")[0].action);
        // Rows typed with terms need no completion.
        assert_eq!(plugin.query("rust")[0].autocomplete, None);
    }

    #[test]
    fn template_may_repeat_the_placeholder() {
        let plugin = WebSearchPlugin::new(
            &WebSearchEngine {
                keyword: "x".into(),
                name: "X".into(),
                url: "https://x.test/{query}?q={query}".into(),
            },
            MockPlatform::empty(),
        );
        let item = &plugin.query("a b")[0];
        assert_eq!(
            item.action,
            Action::OpenUrl {
                url: "https://x.test/a%20b?q=a%20b".into()
            }
        );
    }

    #[test]
    fn execute_opens_the_url() {
        let platform = MockPlatform::empty();
        let plugin = google(platform.clone());
        let results = plugin.query("日本");
        plugin.execute(&results[0]).unwrap();
        assert_eq!(
            *platform.opened_urls.lock().unwrap(),
            vec!["https://www.google.com/search?q=%E6%97%A5%E6%9C%AC".to_owned()]
        );
    }
}
