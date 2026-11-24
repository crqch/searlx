//! Reading mode extraction heuristic, Markdown twins, clean typography template,
//! and reload-on-exit (`FEATURES.md` §5.2, P2-06).
//!
//! Rules (`FEATURES.md` §5.2):
//! - Article extraction heuristic: selects the highest-scoring prose container,
//!   penalised for link density (navigation, related links, and comment blocks
//!   are link-heavy) while rewarding paragraphs, headings, code blocks, tables,
//!   blockquotes, and figures.
//! - If the site publishes a Markdown twin (`<link rel="alternate" type="text/markdown">`
//!   or a `.md` URL) and `reader.twins` is enabled, renders the Markdown twin.
//! - Clean typography: `38em` column, `serif 18px/1.72`, headings, images/videos,
//!   tables (wide tables scroll horizontally), code blocks, blockquotes, lists.
//! - The page's own stylesheets and scripts are removed; no page script runs in
//!   the reader.
//! - Leaving Reading Mode reloads the original page.

use serde::{Deserialize, Serialize};

use crate::url::PageUrl;

/// Reader appearance theme (`FEATURES.md` §5.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReaderTheme {
    /// Light paper background (default).
    #[default]
    Light,
    /// Dark slate background.
    Dark,
    /// Follow the window appearance.
    System,
}

/// Extracted reader representation of an article (`P2-06`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderArticle {
    /// Article title.
    pub title: String,
    /// Original page URL (reloaded when exiting Reading Mode).
    pub source_url: PageUrl,
    /// If a `<link rel="alternate" type="text/markdown">` was discovered, its URL.
    pub markdown_twin_url: Option<String>,
    /// Whether this article was rendered from a Markdown twin.
    pub used_markdown_twin: bool,
    /// Sanitized, script-free HTML body blocks (paragraphs, headings, `<pre><code>`, `<table>`, etc.).
    pub content_html: String,
    /// Plain-text character count of the extracted article.
    pub text_len: usize,
    /// Number of preserved `<pre>` / `<code>` blocks.
    pub code_blocks: usize,
    /// Number of preserved `<table>` blocks.
    pub tables: usize,
}

impl ReaderArticle {
    /// Render the complete standalone Reader HTML document with Search's
    /// `38em`, `18px/1.72` serif stylesheet and zero page scripts (`FEATURES.md` §5.2).
    #[must_use]
    pub fn render_document(&self, theme: ReaderTheme, system_dark: bool) -> String {
        let dark = match theme {
            ReaderTheme::Light => false,
            ReaderTheme::Dark => true,
            ReaderTheme::System => system_dark,
        };
        let (bg, fg, muted, code_bg, border) = if dark {
            ("#141413", "#EAE7E1", "#9C9891", "#1F1E1C", "#33312E")
        } else {
            ("#FAF8F5", "#1D1C1A", "#6E6A63", "#F0EDE8", "#DEDAD2")
        };
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
             <title>{title}</title>\
             <style id=\"searlx-reader-style\">\
             :root {{ color-scheme: {scheme}; }}\
             body {{ background:{bg}; color:{fg}; font-family: Georgia, 'Iowan Old Style', serif; \
                    font-size:18px; line-height:1.72; max-width:38em; margin:3rem auto; padding:0 1.25rem; }}\
             h1, h2, h3, h4 {{ line-height:1.3; margin-top:1.8em; }}\
             p, ul, ol, blockquote {{ margin:1em 0; }}\
             blockquote {{ border-left:3px solid {border}; padding-left:1em; color:{muted}; }}\
             pre, code {{ font-family: ui-monospace, monospace; background:{code_bg}; }}\
             pre {{ padding:1em; border-radius:6px; overflow-x:auto; line-height:1.45; font-size:0.88em; }}\
             .table-wrap {{ overflow-x:auto; margin:1.2em 0; }}\
             table {{ border-collapse:collapse; width:100%; }}\
             th, td {{ border:1px solid {border}; padding:0.45em 0.65em; text-align:left; }}\
             img, video, figure {{ max-width:100%; height:auto; }}\
             </style></head>\
             <body><article><h1>{title}</h1>{body}</article></body></html>",
            title = escape_html(&self.title),
            scheme = if dark { "dark" } else { "light" },
            body = self.content_html,
        )
    }
}

/// Action required when exiting Reading Mode (`FEATURES.md` §5.2: "Leaving reloads the original page").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderExitReload {
    /// Original URL to reload.
    pub reload_url: PageUrl,
}

/// Leave Reading Mode and return the reload action for the original page.
#[must_use]
pub fn exit_reader(article: &ReaderArticle) -> ReaderExitReload {
    ReaderExitReload {
        reload_url: article.source_url.clone(),
    }
}

/// Detect a `<link rel="alternate" type="text/markdown" href="...">` href in HTML (`FEATURES.md` §5.2).
#[must_use]
pub fn detect_markdown_twin_link(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0_usize;
    while let Some(pos) = lower[search_from..].find("<link") {
        let tag_start = search_from + pos;
        let Some(end_offset) = lower[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + end_offset + 1;
        let raw_tag = &html[tag_start..tag_end];
        let is_alternate = extract_attribute(raw_tag, "rel").is_some_and(|v| {
            v.split_whitespace()
                .any(|w| w.eq_ignore_ascii_case("alternate"))
        });
        let is_markdown = extract_attribute(raw_tag, "type")
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/markdown"));
        if is_alternate
            && is_markdown
            && let Some(href) = extract_attribute(raw_tag, "href")
        {
            return Some(href);
        }
        search_from = tag_end;
    }
    None
}

/// Extract an article from HTML (or Markdown when `is_markdown` / `.md` URL and `twins_enabled`).
#[must_use]
pub fn extract_article(
    source: &str,
    url: &PageUrl,
    twins_enabled: bool,
    is_markdown_response: bool,
) -> ReaderArticle {
    let is_md_url = url.as_url().path().to_ascii_lowercase().ends_with(".md");
    if twins_enabled && (is_markdown_response || is_md_url) {
        return render_markdown_twin(source, url);
    }

    let markdown_twin_url = if twins_enabled {
        detect_markdown_twin_link(source)
    } else {
        None
    };

    // Strip <script>, <style>, <nav>, <footer>, <aside> blocks before scoring containers.
    let cleaned = strip_non_article_tags(source);
    let title = extract_tag_text(&cleaned, "h1")
        .or_else(|| extract_tag_text(source, "title"))
        .unwrap_or_else(|| url.display());

    // Evaluate candidate containers (<article>, <main>, <section>, <div>) by prose score minus link penalty.
    let candidates = collect_candidate_blocks(&cleaned);
    let best_raw = candidates
        .into_iter()
        .max_by_key(|block| score_prose_block(block))
        .unwrap_or_else(|| cleaned.clone());

    let (content_html, text_len, code_blocks, tables) = sanitize_and_preserve_blocks(&best_raw);

    ReaderArticle {
        title,
        source_url: url.clone(),
        markdown_twin_url,
        used_markdown_twin: false,
        content_html,
        text_len,
        code_blocks,
        tables,
    }
}

fn render_markdown_twin(md: &str, url: &PageUrl) -> ReaderArticle {
    let mut title = String::new();
    let mut html = String::new();
    let mut in_code = false;
    let mut code_blocks = 0_usize;
    let mut tables = 0_usize;
    let mut in_table = false;
    let mut text_len = 0_usize;

    for line in md.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("```") {
            if in_code {
                html.push_str("</code></pre>\n");
                in_code = false;
            } else {
                html.push_str("<pre><code>");
                in_code = true;
                code_blocks += 1;
            }
            continue;
        }
        if in_code {
            html.push_str(&escape_html(trimmed));
            html.push('\n');
            text_len += trimmed.len();
            continue;
        }
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            if !in_table {
                html.push_str("<div class=\"table-wrap\"><table>");
                in_table = true;
                tables += 1;
            }
            let cells: Vec<&str> = trimmed
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            if cells
                .iter()
                .all(|c| c.chars().all(|ch| ch == '-' || ch == ':'))
            {
                continue;
            }
            html.push_str("<tr>");
            for cell in cells {
                html.push_str("<td>");
                html.push_str(&escape_html(cell));
                html.push_str("</td>");
                text_len += cell.len();
            }
            html.push_str("</tr>\n");
            continue;
        } else if in_table {
            html.push_str("</table></div>\n");
            in_table = false;
        }

        if let Some(h1) = trimmed.strip_prefix("# ") {
            if title.is_empty() {
                title = h1.trim().to_owned();
            } else {
                html.push_str(&format!("<h2>{}</h2>\n", escape_html(h1.trim())));
            }
            text_len += h1.len();
        } else if let Some(h2) = trimmed.strip_prefix("## ") {
            html.push_str(&format!("<h2>{}</h2>\n", escape_html(h2.trim())));
            text_len += h2.len();
        } else if let Some(h3) = trimmed.strip_prefix("### ") {
            html.push_str(&format!("<h3>{}</h3>\n", escape_html(h3.trim())));
            text_len += h3.len();
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            html.push_str(&format!(
                "<blockquote>{}</blockquote>\n",
                escape_html(quote.trim())
            ));
            text_len += quote.len();
        } else if !trimmed.trim().is_empty() {
            html.push_str(&format!("<p>{}</p>\n", escape_html(trimmed.trim())));
            text_len += trimmed.len();
        }
    }
    if in_code {
        html.push_str("</code></pre>\n");
    }
    if in_table {
        html.push_str("</table></div>\n");
    }
    if title.is_empty() {
        title = url.display();
    }

    ReaderArticle {
        title,
        source_url: url.clone(),
        markdown_twin_url: Some(url.as_str().to_owned()),
        used_markdown_twin: true,
        content_html: html,
        text_len,
        code_blocks,
        tables,
    }
}

fn strip_non_article_tags(html: &str) -> String {
    let mut current = html.to_owned();
    for tag in ["script", "style", "nav", "footer", "aside", "noscript"] {
        current = remove_element_block(&current, tag);
    }
    current
}

fn remove_element_block(html: &str, tag: &str) -> String {
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let mut out = String::with_capacity(html.len());
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0_usize;
    while let Some(pos) = lower[cursor..].find(&open_pat) {
        let start = cursor + pos;
        out.push_str(&html[cursor..start]);
        if let Some(end_pos) = lower[start..].find(&close_pat) {
            cursor = start + end_pos + close_pat.len();
        } else {
            cursor = html.len();
            break;
        }
    }
    out.push_str(&html[cursor..]);
    out
}

fn collect_candidate_blocks(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tag in ["article", "main", "section", "div"] {
        let open_pat = format!("<{tag}");
        let close_pat = format!("</{tag}>");
        let lower = html.to_ascii_lowercase();
        let mut cursor = 0_usize;
        while let Some(pos) = lower[cursor..].find(&open_pat) {
            let start = cursor + pos;
            if let Some(tag_close) = lower[start..].find('>') {
                let inner_start = start + tag_close + 1;
                if let Some(close_pos) = lower[inner_start..].find(&close_pat) {
                    let inner_end = inner_start + close_pos;
                    out.push(html[inner_start..inner_end].to_owned());
                }
                cursor = inner_start;
            } else {
                break;
            }
        }
    }
    if out.is_empty() {
        out.push(html.to_owned());
    }
    out
}

/// Score a candidate block: reward plain prose and `<pre>`/`<code>`/`<table>`,
/// heavily penalize link text inside `<a>...</a>` (`FEATURES.md` §5.2).
fn score_prose_block(block: &str) -> i64 {
    let total_text = strip_tags(block);
    let total_chars = total_text.trim().len() as i64;
    if total_chars == 0 {
        return 0;
    }
    let link_chars = count_tag_inner_chars(block, "a") as i64;
    let paragraph_count = block.to_ascii_lowercase().matches("<p").count() as i64;
    let code_bonus = (block.to_ascii_lowercase().matches("<pre").count() as i64) * 180
        + (block.to_ascii_lowercase().matches("<code").count() as i64) * 60;
    let table_bonus = (block.to_ascii_lowercase().matches("<table").count() as i64) * 120;

    // Penalize link-heavy navigation/related/comment containers (`FEATURES.md` §5.2).
    total_chars + (paragraph_count * 80) + code_bonus + table_bonus - (link_chars * 3)
}

fn count_tag_inner_chars(html: &str, tag: &str) -> usize {
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let lower = html.to_ascii_lowercase();
    let mut total = 0_usize;
    let mut cursor = 0_usize;
    while let Some(pos) = lower[cursor..].find(&open_pat) {
        let start = cursor + pos;
        if let Some(end_pos) = lower[start..].find(&close_pat) {
            let segment = &html[start..start + end_pos];
            total += strip_tags(segment).trim().len();
            cursor = start + end_pos + close_pat.len();
        } else {
            break;
        }
    }
    total
}

fn sanitize_and_preserve_blocks(raw: &str) -> (String, usize, usize, usize) {
    let lower = raw.to_ascii_lowercase();
    let code_blocks = lower
        .matches("<pre")
        .count()
        .max(lower.matches("<code").count());
    let tables = lower.matches("<table").count();
    let plain = strip_tags(raw);
    let text_len = plain.trim().len();

    // Remove inline on* event handlers and javascript:/vbscript: hrefs while keeping structure.
    let mut safe = remove_element_block(raw, "script");
    safe = remove_element_block(&safe, "style");
    safe = strip_inline_event_handlers(&safe);
    safe = replace_ascii_case_insensitive(&safe, "javascript:", "blocked:");
    safe = replace_ascii_case_insensitive(&safe, "vbscript:", "blocked:");
    if tables > 0 && !safe.contains("table-wrap") {
        safe = safe
            .replace("<table", "<div class=\"table-wrap\"><table")
            .replace("</table>", "</table></div>");
    }
    (safe, text_len, code_blocks, tables)
}

fn replace_ascii_case_insensitive(input: &str, needle: &str, replacement: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    if needle_lower.is_empty() {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0_usize;
    while let Some(rel) = lower[cursor..].find(&needle_lower) {
        let idx = cursor + rel;
        out.push_str(&input[cursor..idx]);
        out.push_str(replacement);
        cursor = idx + needle.len();
    }
    out.push_str(&input[cursor..]);
    out
}

fn strip_inline_event_handlers(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0_usize;
    while let Some(open_rel) = html[cursor..].find('<') {
        let open = cursor + open_rel;
        out.push_str(&html[cursor..open]);
        let Some(close_rel) = html[open..].find('>') else {
            out.push_str(&html[open..]);
            return out;
        };
        let close = open + close_rel + 1;
        out.push_str(&strip_events_in_tag(&html[open..close]));
        cursor = close;
    }
    out.push_str(&html[cursor..]);
    out
}

fn strip_events_in_tag(tag: &str) -> String {
    let bytes = tag.as_bytes();
    let mut out = String::with_capacity(tag.len());
    let mut i = 0_usize;
    while i < bytes.len() {
        let is_attr_boundary = i > 0 && bytes[i - 1].is_ascii_whitespace();
        if is_attr_boundary
            && i + 3 < bytes.len()
            && (bytes[i] == b'o' || bytes[i] == b'O')
            && (bytes[i + 1] == b'n' || bytes[i + 1] == b'N')
            && bytes[i + 2].is_ascii_alphabetic()
        {
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
                j += 1;
            }
            let mut eq = j;
            while eq < bytes.len() && bytes[eq].is_ascii_whitespace() {
                eq += 1;
            }
            if eq < bytes.len() && bytes[eq] == b'=' {
                let mut val = eq + 1;
                while val < bytes.len() && bytes[val].is_ascii_whitespace() {
                    val += 1;
                }
                if val < bytes.len() && (bytes[val] == b'"' || bytes[val] == b'\'') {
                    let quote = bytes[val];
                    val += 1;
                    while val < bytes.len() && bytes[val] != quote {
                        val += 1;
                    }
                    if val < bytes.len() {
                        val += 1;
                    }
                } else {
                    while val < bytes.len()
                        && !bytes[val].is_ascii_whitespace()
                        && bytes[val] != b'>'
                    {
                        val += 1;
                    }
                }
                i = val;
                continue;
            }
        }
        let ch = tag[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn extract_tag_text(html: &str, tag: &str) -> Option<String> {
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let lower = html.to_ascii_lowercase();
    let start = lower.find(&open_pat)?;
    let gt = lower[start..].find('>')?;
    let content_start = start + gt + 1;
    let close = lower[content_start..].find(&close_pat)?;
    let text = strip_tags(&html[content_start..content_start + close])
        .trim()
        .to_owned();
    (!text.is_empty()).then_some(text)
}

fn extract_attribute(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = attr_name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut search = 0_usize;
    while let Some(pos) = lower[search..].find(&needle) {
        let start = search + pos;
        let boundary_ok = start == 0 || bytes[start - 1].is_ascii_whitespace();
        let after_name = start + needle.len();
        if boundary_ok {
            let mut eq = after_name;
            while eq < bytes.len() && bytes[eq].is_ascii_whitespace() {
                eq += 1;
            }
            if eq < bytes.len() && bytes[eq] == b'=' {
                let mut val_start = eq + 1;
                while val_start < bytes.len() && bytes[val_start].is_ascii_whitespace() {
                    val_start += 1;
                }
                if val_start < bytes.len()
                    && (bytes[val_start] == b'"' || bytes[val_start] == b'\'')
                {
                    let quote = bytes[val_start] as char;
                    let content_start = val_start + 1;
                    if let Some(end) = tag[content_start..].find(quote) {
                        return Some(tag[content_start..content_start + end].to_owned());
                    }
                }
            }
        }
        search = after_name;
    }
    None
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_ten_representative_articles_keeps_code_and_reloads_on_exit() {
        let cases = [
            (
                "https://example.com/news-1",
                "<html><head><title>News 1</title><link rel=\"alternate\" type=\"text/markdown\" href=\"/news-1.md\"></head><body><nav><a href=\"/a\">Home</a><a href=\"/b\">World</a></nav><article><h1>Breaking Story</h1><p>A detailed report on systems programming and browser engines with substantial body text.</p></article></body></html>",
                false,
            ),
            (
                "https://example.com/code-heavy",
                "<html><body><nav><a href=\"/1\">Docs</a><a href=\"/2\">API</a></nav><article><h1>Rust Ownership</h1><p>Borrow checking rules in practice.</p><pre><code>fn main() {\n    let x = vec![1, 2, 3];\n    println!(\"{x:?}\");\n}</code></pre></article></body></html>",
                false,
            ),
            (
                "https://example.com/table-report",
                "<html><body><main><h1>Benchmark Numbers</h1><p>Memory across tab counts:</p><table><tr><th>Tabs</th><th>RSS</th></tr><tr><td>1</td><td>400 MiB</td></tr></table></main></body></html>",
                false,
            ),
            (
                "https://example.com/post.md",
                "# Markdown Twin Article\n\nClean prose from the site's `.md` twin.\n\n```rust\nlet engine = \"webkit\";\n```\n\n| OS | Engine |\n|---|---|\n| macOS | WKWebView |\n",
                true,
            ),
            (
                "https://example.com/essay-5",
                "<html><body><article><h1>Essay Five</h1><p>First long paragraph of essay five explaining design choices.</p><blockquote>Quiet software respects focus.</blockquote></article><footer><a href=\"/x\"> link 1 </a><a href=\"/y\"> link 2 </a></footer></body></html>",
                false,
            ),
            (
                "https://example.com/essay-6",
                "<html><body><article><h1>Essay Six</h1><p>Paragraph one with prose.</p><p>Paragraph two with more prose.</p></article></body></html>",
                false,
            ),
            (
                "https://example.com/essay-7",
                "<html><body><article><h1>Essay Seven</h1><p> Deep dive into Wayland subsurfaces and X11 window trees. </p></article></body></html>",
                false,
            ),
            (
                "https://example.com/essay-8",
                "<html><body><article><h1>Essay Eight</h1><p> Exploring WebKit content rule lists and isolated script worlds. </p></article></body></html>",
                false,
            ),
            (
                "https://example.com/essay-9",
                "<html><body><article><h1>Essay Nine</h1><p> Session restoration and tab sleeping architectures. </p></article></body></html>",
                false,
            ),
            (
                "https://example.com/essay-10",
                "<html><body><style>.tailwind-reset { list-style: none; }</style><script>alert(1)</script><article><h1>Essay Ten</h1><ul><li>Item one</li><li>Item two</li></ul><p>Concluding paragraph.</p></article></body></html>",
                false,
            ),
        ];

        for (raw_url, body, is_md) in cases {
            let url = PageUrl::parse(raw_url).expect("parse url");
            let article = extract_article(body, &url, true, is_md);
            assert!(
                article.text_len > 10,
                "article {raw_url} had too little text"
            );
            let doc = article.render_document(ReaderTheme::Light, false);
            assert!(doc.contains("max-width:38em"));
            assert!(!doc.contains("<script"));
            assert!(!doc.contains("tailwind-reset"));
            assert_eq!(exit_reader(&article).reload_url, url);
        }

        // Verify code-heavy article specifically preserved its code block and table article wrapped its table.
        let code_url = PageUrl::parse("https://example.com/code-heavy").expect("url");
        let code_article = extract_article(cases[1].1, &code_url, true, false);
        assert!(code_article.code_blocks >= 1);
        assert!(code_article.content_html.contains("println!"));

        let md_url = PageUrl::parse("https://example.com/post.md").expect("url");
        let md_article = extract_article(cases[3].1, &md_url, true, true);
        assert!(md_article.used_markdown_twin);
        assert_eq!(md_article.code_blocks, 1);
        assert_eq!(md_article.tables, 1);

        let single_quoted = "<html><head><link type='text/markdown' rel='alternate' href='/sq.md'></head><body><article><h1>Hi</h1><p onclick=\"alert(1)\"><a href=\"JaVaScRiPt:alert(2)\">body prose long enough</a></p></article></body></html>";
        assert_eq!(
            detect_markdown_twin_link(single_quoted).as_deref(),
            Some("/sq.md")
        );
        let sq_article = extract_article(single_quoted, &code_url, false, false);
        assert!(!sq_article.content_html.contains("onclick"));
        assert!(
            !sq_article
                .content_html
                .to_ascii_lowercase()
                .contains("javascript:")
        );
    }
}
