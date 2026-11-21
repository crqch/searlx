//! A page URL with searlx's identity, display, and redaction rules.
//!
//! The engine boundary only accepts [`PageUrl`], so no caller can hand a web
//! view a `javascript:` URL by accident. Constructing one always strips
//! userinfo: credentials in an address are never displayed, copied, stored, or
//! logged.

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use url::{Host, Url};

/// Schemes that may be loaded into a page.
const PAGE_SCHEMES: &[&str] = &[
    "http",
    "https",
    "file",
    "about",
    "data",
    "search",
    "view-source",
    "blob",
];

/// A URL that is safe to hand to an engine.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PageUrl(Url);

impl PageUrl {
    /// Parse an absolute page URL.
    ///
    /// Userinfo is removed. Fragments are retained for display but ignored by
    /// [`PageUrl::identity`].
    ///
    /// # Errors
    /// Returns [`CoreError::InvalidUrl`] when the input is not absolute, uses a
    /// scheme a page may not load, or is an internal `search://` route that is
    /// not a known page.
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(CoreError::InvalidUrl("empty address".into()));
        }
        let mut url =
            Url::parse(trimmed).map_err(|error| CoreError::InvalidUrl(error.to_string()))?;
        if !PAGE_SCHEMES.contains(&url.scheme()) {
            return Err(CoreError::InvalidUrl(format!(
                "scheme {:?} is not a page",
                url.scheme()
            )));
        }
        if url.scheme() == "search" && !crate::url::is_known_internal_route(&url) {
            return Err(CoreError::InvalidUrl(format!(
                "unknown internal page {trimmed:?}"
            )));
        }
        url.set_username("").ok();
        url.set_password(None).ok();
        Ok(Self(url))
    }

    /// Wrap an already parsed URL, applying the same scheme and userinfo rules.
    ///
    /// # Errors
    /// See [`PageUrl::parse`].
    pub fn from_url(url: &Url) -> Result<Self> {
        Self::parse(url.as_str())
    }

    /// The URL as a string, including any fragment.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The parsed URL.
    #[must_use]
    pub const fn as_url(&self) -> &Url {
        &self.0
    }

    /// Consume the wrapper and return the parsed URL.
    #[must_use]
    pub fn into_url(self) -> Url {
        self.0
    }

    /// The scheme, lower-case.
    #[must_use]
    pub fn scheme(&self) -> &str {
        self.0.scheme()
    }

    /// The host, if the scheme has one.
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        self.0.host_str()
    }

    /// The `scheme://host[:port]` origin, or `None` for opaque origins.
    #[must_use]
    pub fn origin(&self) -> Option<String> {
        self.0.origin().ascii_serialization().into()
    }

    /// Whether this is an internal `search://` page.
    #[must_use]
    pub fn is_internal(&self) -> bool {
        self.0.scheme() == "search"
    }

    /// Whether the page is served over plain HTTP.
    #[must_use]
    pub fn is_insecure(&self) -> bool {
        self.0.scheme() == "http"
    }

    /// A stable history/session key.
    ///
    /// Fragments are ignored; the scheme and host are lower-case and the
    /// default port is absent because `url` normalises them. Path and query
    /// spelling are preserved.
    #[must_use]
    pub fn identity(&self) -> String {
        let mut url = self.0.clone();
        url.set_fragment(None);
        url.as_str().to_owned()
    }

    /// A short display label, with `www.` hidden and no userinfo.
    #[must_use]
    pub fn display(&self) -> String {
        match self.0.scheme() {
            "search" => format!("search://{}", self.0.path().trim_start_matches('/')),
            "file" => self
                .0
                .path_segments()
                .and_then(|mut segments| segments.next_back().map(ToOwned::to_owned))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| self.0.path().to_owned()),
            _ => {
                let host = self.0.host_str().map_or_else(String::new, |host| {
                    host.trim_start_matches("www.").to_owned()
                });
                let mut label = host;
                if let Some(port) = self.0.port() {
                    label.push(':');
                    label.push_str(&port.to_string());
                }
                label.push_str(self.0.path());
                if let Some(query) = self.0.query() {
                    label.push('?');
                    label.push_str(query);
                }
                label
            }
        }
    }

    /// A form safe for logs and error messages.
    ///
    /// The path is kept; userinfo, query values, and fragments are removed.
    /// Query keys survive as `?key=…` markers so support can identify a route
    /// without seeing a search term, token, or session identifier.
    #[must_use]
    pub fn redacted(&self) -> String {
        let mut out = String::new();
        out.push_str(self.0.scheme());
        out.push(':');
        if let Some(host) = self.0.host_str() {
            out.push_str("//");
            out.push_str(host);
            if let Some(port) = self.0.port() {
                out.push(':');
                out.push_str(&port.to_string());
            }
        }
        out.push_str(self.0.path());
        if let Some(query) = self.0.query() {
            out.push('?');
            let mut first = true;
            for pair in query.split('&') {
                let key = pair.split('=').next().unwrap_or_default();
                if !first {
                    out.push('&');
                }
                first = false;
                out.push_str(key);
                out.push('=');
                out.push('…');
            }
        }
        if self.0.fragment().is_some() {
            out.push_str("#…");
        }
        out
    }

    /// The registrable-ish host used as a display label: the last two labels,
    /// keeping a two-label public suffix such as `co.uk` intact.
    #[must_use]
    pub fn site_label(&self) -> String {
        let Some(host) = self.0.host_str() else {
            return self.display();
        };
        let host = host.trim_start_matches("www.");
        if matches!(self.0.host(), Some(Host::Ipv4(_) | Host::Ipv6(_))) {
            return host.to_owned();
        }
        let labels: Vec<&str> = host.split('.').collect();
        let take = if labels.len() >= 3 && is_two_part_suffix(labels[labels.len() - 2]) {
            3
        } else {
            2
        };
        labels[labels.len().saturating_sub(take)..].join(".")
    }

    /// Whether two URLs share an origin, following `url`'s rules.
    #[must_use]
    pub fn same_origin(&self, other: &Self) -> bool {
        self.0.origin() == other.0.origin()
    }
}

fn is_two_part_suffix(label: &str) -> bool {
    matches!(
        label,
        "co" | "com" | "org" | "net" | "gov" | "edu" | "ac" | "or" | "ne" | "go"
    )
}

impl fmt::Display for PageUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display().as_str())
    }
}

impl fmt::Debug for PageUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

impl TryFrom<String> for PageUrl {
    type Error = CoreError;

    fn try_from(value: String) -> Result<Self> {
        Self::parse(&value)
    }
}

impl From<PageUrl> for String {
    fn from(value: PageUrl) -> Self {
        value.0.into()
    }
}

/// Internal pages are the only `search://` routes a page may load.
pub(crate) fn is_known_internal_route(url: &Url) -> bool {
    let route = if url.path().trim_start_matches('/').is_empty() {
        url.host_str().unwrap_or_default()
    } else {
        url.path().trim_start_matches('/')
    };
    matches!(
        route,
        "" | "settings"
            | "history"
            | "downloads"
            | "bookmarks"
            | "passwords"
            | "archive"
            | "import"
            | "release-notes"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_userinfo() {
        let url = PageUrl::parse("https://user:secret@example.com/a?q=1#frag").expect("parse");
        assert_eq!(url.as_str(), "https://example.com/a?q=1#frag");
        assert!(!url.as_str().contains("secret"));
    }

    #[test]
    fn identity_ignores_fragments_and_normalises_hosts() {
        let a = PageUrl::parse("HTTPS://Example.COM:443/Path?Q=1#one").expect("parse");
        let b = PageUrl::parse("https://example.com/Path?Q=1#two").expect("parse");
        assert_eq!(a.identity(), b.identity());
        assert_eq!(a.identity(), "https://example.com/Path?Q=1");
    }

    #[test]
    fn display_hides_scheme_and_www() {
        let url = PageUrl::parse("https://www.example.com/a").expect("parse");
        assert_eq!(url.display(), "example.com/a");
    }

    #[test]
    fn display_keeps_plain_http_host_and_nondefault_port() {
        let url = PageUrl::parse("http://example.com:8080/a").expect("parse");
        assert_eq!(url.display(), "example.com:8080/a");
        assert!(url.is_insecure());
    }

    #[test]
    fn redaction_drops_values_and_fragments() {
        let url = PageUrl::parse("https://example.com/search?q=private+words&token=abc#section")
            .expect("parse");
        let redacted = url.redacted();
        assert_eq!(redacted, "https://example.com/search?q=…&token=…#…");
        assert!(!redacted.contains("private"));
        assert!(!redacted.contains("abc"));
    }

    #[test]
    fn rejects_script_and_unknown_schemes() {
        assert!(PageUrl::parse("javascript:alert(1)").is_err());
        assert!(PageUrl::parse("chrome://settings").is_err());
        assert!(PageUrl::parse("search://settings").is_ok());
        assert!(PageUrl::parse("search://made-up").is_err());
    }

    #[test]
    fn site_label_keeps_two_part_suffixes() {
        let url = PageUrl::parse("https://news.bbc.co.uk/story").expect("parse");
        assert_eq!(url.site_label(), "bbc.co.uk");
        let url = PageUrl::parse("https://a.b.example.com/").expect("parse");
        assert_eq!(url.site_label(), "example.com");
    }

    #[test]
    fn same_origin_uses_scheme_host_port() {
        let a = PageUrl::parse("https://example.com/a").expect("parse");
        let b = PageUrl::parse("https://example.com:443/b").expect("parse");
        let c = PageUrl::parse("http://example.com/a").expect("parse");
        assert!(a.same_origin(&b));
        assert!(!a.same_origin(&c));
    }

    #[test]
    fn serde_round_trip_uses_string_form() {
        let url = PageUrl::parse("https://example.com/a").expect("parse");
        let json = serde_json::to_string(&url).expect("serialize");
        assert_eq!(json, "\"https://example.com/a\"");
        let back: PageUrl = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(url, back);
    }
}
