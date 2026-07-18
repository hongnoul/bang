//! Search backends. Bangs are never interpreted client-side: SearXNG
//! resolves engine bangs server-side and still returns unified JSON, while
//! DuckDuckGo resolves navigational bangs into a redirect whose destination
//! we surface. Unknown bangs degrade to a normal search.

use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Serialize, Debug)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub enum Outcome {
    /// A navigational bang resolved to a destination URL.
    Destination(String),
    /// Ranked search results.
    Results(Vec<SearchResult>),
}

pub fn run(engine: &str, query: &str, num: usize, config: &Config) -> Result<Outcome> {
    match engine {
        "searxng" => searxng(query, num, config),
        "duckduckgo" | "ddg" => duckduckgo(query, num),
        other => Err(anyhow!(
            "unknown engine '{other}' (expected duckduckgo or searxng)"
        )),
    }
}

/// DuckDuckGo: resolve bangs via the server-side redirect, otherwise scrape
/// the HTML results endpoint.
fn duckduckgo(query: &str, num: usize) -> Result<Outcome> {
    if has_bang(query) {
        if let Some(dest) = resolve_ddg_bang(query)? {
            return Ok(Outcome::Destination(dest));
        }
    }

    let body = ureq::post("https://html.duckduckgo.com/html/")
        .set("User-Agent", USER_AGENT)
        .set("Accept", "text/html,application/xhtml+xml")
        .send_form(&[("q", query), ("kl", "us-en")])
        .context("DuckDuckGo request failed")?
        .into_string()?;

    Ok(Outcome::Results(parse_ddg_results(&body, num)))
}

/// SearXNG: full pass-through. Engine bangs (`!go`, `!bi`, `!wp`, ...) are
/// resolved server-side and the response shape stays identical.
fn searxng(query: &str, num: usize, config: &Config) -> Result<Outcome> {
    let base = config
        .searxng_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "searxng engine selected but no instance configured; set \
                 `searxng_url` in ~/.config/bang/config.toml or BANG_SEARXNG_URL"
            )
        })?;

    #[derive(Deserialize)]
    struct Response {
        #[serde(default)]
        results: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        #[serde(default)]
        title: String,
        #[serde(default)]
        url: String,
        #[serde(default)]
        content: Option<String>,
    }

    let response: Response = ureq::get(&format!("{}/search", base.trim_end_matches('/')))
        .query("q", query)
        .query("format", "json")
        .set("User-Agent", USER_AGENT)
        .call()
        .context("SearXNG request failed (does the instance allow JSON format?)")?
        .into_json()?;

    Ok(Outcome::Results(
        response
            .results
            .into_iter()
            .filter(|r| !r.url.trim().is_empty())
            .take(num)
            .map(|r| SearchResult {
                title: if r.title.trim().is_empty() {
                    r.url.clone()
                } else {
                    r.title
                },
                url: r.url,
                snippet: r.content.unwrap_or_default(),
            })
            .collect(),
    ))
}

/// A query "has a bang" when its first or last whitespace token is
/// `!`-prefixed alphanumerics. Detection only; interpretation stays with
/// the engine.
fn has_bang(query: &str) -> bool {
    let is_bang = |w: &str| {
        w.len() > 1 && w.starts_with('!') && w[1..].chars().all(|c| c.is_ascii_alphanumeric())
    };
    let mut words = query.split_whitespace();
    let first = words.next();
    let last = query.split_whitespace().next_back();
    first.is_some_and(is_bang) || last.is_some_and(is_bang)
}

/// DDG answers a recognized bang with a meta-refresh page pointing at
/// `/l/?uddg=<percent-encoded destination>`. Absent marker means the bang
/// was not recognized and the caller should do a normal search.
fn resolve_ddg_bang(query: &str) -> Result<Option<String>> {
    let response = ureq::get("https://duckduckgo.com/")
        .query("q", query)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "text/html,application/xhtml+xml")
        .call()
        .context("DuckDuckGo bang resolution failed")?;
    let body = response.into_string()?;
    Ok(parse_bang_redirect(&body))
}

fn parse_bang_redirect(body: &str) -> Option<String> {
    let marker = body.find("/l/?uddg=")?;
    let after = &body[marker + "/l/?uddg=".len()..];
    let encoded = after
        .split(['&', '\'', '"', '<'])
        .next()
        .unwrap_or_default();
    let url = urlencoding::decode(encoded).ok()?.to_string();
    (url.starts_with("http://") || url.starts_with("https://")).then_some(url)
}

/// Minimal, dependency-free extraction of DDG HTML results.
fn parse_ddg_results(body: &str, num: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    for block in body.split(r#"class="result__a""#).skip(1) {
        if results.len() >= num {
            break;
        }
        let Some(href) = attr_after(block, "href=\"") else {
            continue;
        };
        let url = decode_uddg(&href);
        if !url.starts_with("http") || url.contains("duckduckgo.com") {
            continue;
        }
        let title = text_between(block, ">", "</a>");
        let snippet = block
            .split(r#"class="result__snippet""#)
            .nth(1)
            .map(|s| text_between(s, ">", "</a>"))
            .unwrap_or_default();
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }
    results
}

fn attr_after(block: &str, marker: &str) -> Option<String> {
    let start = block.find(marker)? + marker.len();
    let end = block[start..].find('"')? + start;
    Some(block[start..end].to_string())
}

fn text_between(block: &str, open: &str, close: &str) -> String {
    let Some(start) = block.find(open).map(|i| i + open.len()) else {
        return String::new();
    };
    let end = block[start..]
        .find(close)
        .map(|i| start + i)
        .unwrap_or(block.len());
    strip_tags(&block[start..end])
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

fn decode_uddg(url: &str) -> String {
    if let Some(start) = url.find("uddg=") {
        let start = start + 5;
        let end = url[start..]
            .find('&')
            .map(|i| start + i)
            .unwrap_or(url.len());
        urlencoding::decode(&url[start..end])
            .map(|s| s.to_string())
            .unwrap_or_else(|_| url.to_string())
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bangs_at_either_end() {
        assert!(has_bang("!gh ripgrep"));
        assert!(has_bang("rust lifetimes !w"));
        assert!(has_bang("!wp"));
        assert!(!has_bang("plain query"));
        assert!(!has_bang("not!a bang"));
        assert!(!has_bang("mid !gh word"));
        assert!(!has_bang(""));
        assert!(!has_bang("!"));
    }

    #[test]
    fn parses_bang_redirect_page() {
        let body = "<meta http-equiv='refresh' content='0; \
                    url=/l/?uddg=https%3A%2F%2Fen.wikipedia.org%2Fwiki%2FRust&rut=abc'>";
        assert_eq!(
            parse_bang_redirect(body).as_deref(),
            Some("https://en.wikipedia.org/wiki/Rust")
        );
    }

    #[test]
    fn normal_page_is_not_a_redirect() {
        assert_eq!(parse_bang_redirect("<html>results</html>"), None);
    }

    #[test]
    fn parses_ddg_result_markup() {
        let body = r##"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fdocs&rut=x">Example <b>Docs</b></a>
                      <a class="result__snippet" href="#">A &amp; B</a>"##;
        let results = parse_ddg_results(body, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/docs");
        assert_eq!(results[0].title, "Example Docs");
        assert_eq!(results[0].snippet, "A & B");
    }
}
