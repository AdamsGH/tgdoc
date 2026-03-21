use std::collections::HashMap;
use std::path::Path;
use anyhow::Result;
use scraper::{Html, Selector, ElementRef, Node};

use crate::anchor_index::AnchorIndex;
use crate::convert::{element_to_md, frontmatter, extract_headings};
use crate::fetch::{build_client, get_html};

const BASE: &str = "https://core.telegram.org";

struct PageDef {
    url: &'static str,
    out: &'static str,
    split_by: SplitMode,
    tags: &'static [&'static str],
}

#[derive(Clone)]
enum SplitMode {
    /// One file as-is
    Single,
    /// Split by h3 sections, each into a subdirectory file
    ByH3 { dir: &'static str },
    /// Split changelog by year (h3=year, h4=entry)
    Changelog { dir: &'static str },
}

static PAGES: &[PageDef] = &[
    PageDef {
        url: "/bots/api",
        out: "api",
        split_by: SplitMode::ByH3 { dir: "api" },
        tags: &["bot-api"],
    },
    PageDef {
        url: "/bots/api-changelog",
        out: "changelog",
        split_by: SplitMode::Changelog { dir: "changelog" },
        tags: &["changelog"],
    },
    PageDef {
        url: "/bots/webapps",
        out: "webapps",
        split_by: SplitMode::ByH3 { dir: "webapps" },
        tags: &["webapps", "mini-apps"],
    },
    PageDef {
        url: "/bots/payments",
        out: "payments-guide",
        split_by: SplitMode::ByH3 { dir: "payments-guide" },
        tags: &["payments"],
    },
    PageDef {
        url: "/bots",
        out: "bots.md",
        split_by: SplitMode::Single,
        tags: &["bots", "intro"],
    },
    PageDef {
        url: "/bots/faq",
        out: "faq.md",
        split_by: SplitMode::Single,
        tags: &["faq"],
    },
    PageDef {
        url: "/bots/inline",
        out: "inline.md",
        split_by: SplitMode::Single,
        tags: &["inline"],
    },
    PageDef {
        url: "/bots/webhooks",
        out: "webhooks.md",
        split_by: SplitMode::Single,
        tags: &["webhooks"],
    },
    PageDef {
        url: "/bots/self-signed",
        out: "self-signed.md",
        split_by: SplitMode::Single,
        tags: &["webhooks", "ssl"],
    },
    PageDef {
        url: "/stickers",
        out: "stickers.md",
        split_by: SplitMode::Single,
        tags: &["stickers"],
    },
    PageDef {
        url: "/passport",
        out: "passport.md",
        split_by: SplitMode::Single,
        tags: &["passport"],
    },
    PageDef {
        url: "/widgets/login",
        out: "widgets-login.md",
        split_by: SplitMode::Single,
        tags: &["widgets", "login"],
    },
];

/// h3 id -> slug for file name
fn h3_to_slug(id: &str) -> String {
    id.to_lowercase()
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

/// Pretty title from h3 text

pub async fn run(proxy: &str, out_dir: &str, dry: bool) -> Result<()> {
    let client = build_client(proxy)?;

    // Phase 1: fetch all pages
    let mut html_cache: HashMap<&'static str, String> = HashMap::new();
    for page in PAGES {
        let url = format!("{}{}", BASE, page.url);
        println!("[fetch] {}", url);
        let html = get_html(&client, &url).await?;
        println!("  {} bytes", html.len());
        html_cache.insert(page.url, html);
    }

    if dry {
        // Just print heading trees
        for page in PAGES {
            println!("\n=== {} ===", page.url);
            let html = &html_cache[page.url];
            let headings = extract_headings(html);
            for (level, id, text) in &headings {
                let indent = "  ".repeat((*level as usize).saturating_sub(2));
                println!("{}[h{}] #{} {}", indent, level, id, text);
            }
        }
        return Ok(());
    }

    // Phase 2: build anchor index across all pages
    let mut index = AnchorIndex::new();
    for page in PAGES {
        let html = &html_cache[page.url];
        let headings = extract_headings(html);
        for (_level, id, text) in &headings {
            if id.is_empty() { continue; }
            let doc_path = resolve_anchor_path(page, &id, &headings);
            index.register(&id, &doc_path, &text);
        }
    }

    // Phase 3: write files
    for page in PAGES {
        let html = &html_cache[page.url];
        let source_url = format!("{}{}", BASE, page.url);
        match &page.split_by.clone() {
            SplitMode::Single => {
                let path = format!("{}/{}", out_dir, page.out);
                let doc_base = page.out.trim_end_matches(".md");
                println!("[write] {}", path);
                let md = page_to_md(html, &index, doc_base, &source_url, page.tags);
                write_file(&path, &md)?;
            }
            SplitMode::ByH3 { dir } => {
                let sections = split_by_h3(html, &index, dir);
                for (slug, title, content) in &sections {
                    let path = format!("{}/{}/{}.md", out_dir, dir, slug);
                    let fm = frontmatter(title, &source_url, page.tags);
                    println!("[write] {}", path);
                    write_file(&path, &format!("{}{}", fm, content))?;
                }
                // Write index file
                let idx_path = format!("{}/{}/index.md", out_dir, dir);
                let idx_content = build_index(dir, &sections, &source_url, page.tags);
                write_file(&idx_path, &idx_content)?;
            }
            SplitMode::Changelog { dir } => {
                let entries = split_changelog(html, &index, dir);
                for entry in &entries {
                    let path = format!("{}/{}/{}/{}.md", out_dir, dir, entry.year, entry.slug);
                    println!("[write] {}", path);
                    let source_anchor = format!("{}#{}", source_url,
                        entry.slug.to_lowercase().replace('.', "-").replace("botapi-", ""));
                    let fm = changelog_frontmatter(&entry.version, &entry.date, &source_anchor);
                    write_file(&path, &format!("{}{}", fm, entry.content))?;
                }
                // Index per year
                let mut by_year: std::collections::BTreeMap<&str, Vec<&ChangelogEntry>> = Default::default();
                for e in &entries {
                    by_year.entry(&e.year).or_default().push(e);
                }
                for (year, year_entries) in &by_year {
                    let idx_path = format!("{}/{}/{}/index.md", out_dir, dir, year);
                    let idx = build_year_index(dir, year, year_entries, &source_url);
                    write_file(&idx_path, &idx)?;
                }
                // Top-level changelog index
                let idx_path = format!("{}/{}/index.md", out_dir, dir);
                let idx = build_changelog_index(dir, &by_year, &source_url);
                write_file(&idx_path, &idx)?;
            }
        }
    }

    println!("\nDone. Files written to {}/", out_dir);
    Ok(())
}

/// Determine the output doc path for an anchor given the page's split mode.
fn resolve_anchor_path(page: &PageDef, anchor: &str, all_headings: &[(u8, String, String)]) -> String {
    match &page.split_by {
        SplitMode::Single => page.out.trim_end_matches(".md").to_string(),
        SplitMode::ByH3 { dir } => {
            // Find the h3 section this anchor belongs to
            let mut current_h3 = "index".to_string();
            for (level, id, _text) in all_headings {
                if *level == 3 {
                    current_h3 = h3_to_slug(id);
                }
                if id == anchor {
                    return format!("{}/{}", dir, current_h3);
                }
            }
            format!("{}/index", dir)
        }
        SplitMode::Changelog { dir } => {
            // Anchors live in changelog/<year>/<slug> files.
            // For the anchor index we just point to year/index since we
            // don't have version slugs at this stage - good enough for
            // cross-page link resolution.
            let mut current_year = "index".to_string();
            for (level, id, text) in all_headings {
                if *level == 3 && text.chars().all(|c| c.is_ascii_digit()) {
                    current_year = text.clone();
                }
                if id == anchor {
                    return format!("{}/{}/index", dir, current_year);
                }
            }
            format!("{}/index", dir)
        }
    }
}

/// Convert a full page to a single Markdown string.
fn page_to_md(html: &str, index: &AnchorIndex, doc_base: &str, source_url: &str, tags: &[&str]) -> String {
    let doc = Html::parse_document(html);
    let content_sel = Selector::parse(
        "div.page-body, div#dev_page_content, div.dev_page_content, div#page-content, article, main"
    ).unwrap();

    let title_sel = Selector::parse("h1").unwrap();
    let title = doc.select(&title_sel).next()
        .map(|el| el.text().collect::<String>())
        .unwrap_or_else(|| doc_base.to_string());

    let fm = frontmatter(title.trim(), source_url, tags);

    let body = if let Some(root) = doc.select(&content_sel).next() {
        node_to_md(root, index, doc_base)
    } else {
        "[content not found]".to_string()
    };

    format!("{}{}", fm, body)
}

/// Convert all children of an element to MD, recursively.
fn node_to_md(el: ElementRef, index: &AnchorIndex, doc_base: &str) -> String {
    let mut out = String::new();
    for child in el.children() {
        match child.value() {
            Node::Text(t) => {
                let s = t.to_string();
                if !s.trim().is_empty() {
                    out.push_str(&s);
                }
            }
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    out.push_str(&element_to_md(child_el, index, doc_base));
                }
            }
            _ => {}
        }
    }
    out
}

/// Split page by h3 sections, return Vec<(slug, title, markdown_content)>
fn split_by_h3(html: &str, index: &AnchorIndex, dir: &str) -> Vec<(String, String, String)> {
    let doc = Html::parse_document(html);
    let content_sel = Selector::parse(
        "div.page-body, div#dev_page_content, div.dev_page_content, div#page-content, article, main"
    ).unwrap();

    let root = match doc.select(&content_sel).next() {
        Some(r) => r,
        None => return vec![],
    };

    let mut sections: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut current_id = "intro".to_string();
    let mut current_title = "Introduction".to_string();
    let mut buf: Vec<String> = Vec::new();

    for child in root.children() {
        match child.value() {
            Node::Element(el_val) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    let tag = el_val.name();
                    if tag == "h3" {
                        // Save previous
                        if !buf.is_empty() {
                            sections.push((current_id.clone(), current_title.clone(), buf.clone()));
                            buf.clear();
                        }
                        let anchor_sel = Selector::parse("a[name]").unwrap();
                        let raw_id = child_el.value().attr("id")
                            .map(|s| s.to_string())
                            .or_else(|| child_el.select(&anchor_sel).next()
                                .and_then(|a| a.value().attr("name"))
                                .map(|s| s.to_string()))
                            .unwrap_or_default();
                        current_id = if raw_id.is_empty() { "section".to_string() } else { h3_to_slug(&raw_id) };
                        current_title = child_el.text().collect::<String>().trim().to_string();
                        buf.push(format!("# {}\n", current_title));
                    } else {
                        let _doc_base = format!("{}/{}", dir, current_id);
                        buf.push(element_to_md(child_el, index, &_doc_base));
                    }
                }
            }
            Node::Text(t) => {
                if !t.trim().is_empty() {
                    buf.push(t.to_string());
                }
            }
            _ => {}
        }
    }
    if !buf.is_empty() {
        sections.push((current_id, current_title, buf));
    }

    sections.into_iter()
        .map(|(id, title, lines)| (id, title, lines.join("\n")))
        .collect()
}

/// A single changelog release entry.
struct ChangelogEntry {
    /// Year, e.g. "2026"
    year: String,
    /// File slug, e.g. "BotAPI-9.5"
    slug: String,
    /// Human version title, e.g. "Bot API 9.5"
    version: String,
    /// ISO date for frontmatter, e.g. "2026-03-01"
    date: String,
    /// Markdown body
    content: String,
}

/// Extract "Bot API X.Y" from the first paragraph text of a section.
fn extract_version(first_p: &str) -> Option<String> {
    let re = regex::Regex::new(r"Bot API\s+(\d+\.\d+)").unwrap();
    re.find(first_p).map(|m| m.as_str().to_string())
}

/// Convert h4 date text like "March 1, 2026" to ISO "2026-03-01".
fn date_to_iso(text: &str) -> String {
    let months = [
        ("January", "01"), ("February", "02"), ("March", "03"),
        ("April", "04"), ("May", "05"), ("June", "06"),
        ("July", "07"), ("August", "08"), ("September", "09"),
        ("October", "10"), ("November", "11"), ("December", "12"),
    ];
    // Formats: "March 1, 2026" or "November, 2015" or "July 2015"
    let text = text.trim();
    for (name, num) in &months {
        if text.starts_with(name) {
            let rest = text[name.len()..].trim().trim_start_matches(',').trim();
            // "1, 2026" or "2026"
            let parts: Vec<&str> = rest.splitn(2, ',').collect();
            if parts.len() == 2 {
                let day = parts[0].trim().parse::<u32>().unwrap_or(1);
                let year = parts[1].trim();
                return format!("{}-{}-{:02}", year, num, day);
            } else {
                // "2026" only (no day) or "2015"
                let year = rest.trim();
                return format!("{}-{}-01", year, num);
            }
        }
    }
    text.to_string()
}

/// Split changelog into per-release entries: Vec<ChangelogEntry>
fn split_changelog(html: &str, index: &AnchorIndex, dir: &str) -> Vec<ChangelogEntry> {
    let doc = Html::parse_document(html);
    let content_sel = Selector::parse(
        "div.page-body, div#dev_page_content, div.dev_page_content, div#page-content, article, main"
    ).unwrap();
    let root = match doc.select(&content_sel).next() {
        Some(r) => r,
        None => return vec![],
    };

    let mut entries: Vec<ChangelogEntry> = Vec::new();
    let mut current_year = "misc".to_string();

    // State for current h4 entry
    let mut entry_date_text = String::new();
    let mut entry_id = String::new();
    let mut buf: Vec<String> = Vec::new();
    let mut first_p_seen = false;
    let mut current_version: Option<String> = None;

    let flush = |entries: &mut Vec<ChangelogEntry>,
                 year: &str,
                 date_text: &str,
                 version: &Option<String>,
                 buf: &[String]| {
        if date_text.is_empty() { return; }
        let date = date_to_iso(date_text);
        let ver = version.clone().unwrap_or_else(|| date_text.to_string());
        // slug: "BotAPI-9.5" or sanitised date fallback
        let slug = if ver.starts_with("Bot API") {
            format!("BotAPI-{}", ver.trim_start_matches("Bot API").trim())
        } else {
            date.replace('-', "-")
        };
        let doc_base = format!("{}/{}/{}", dir, year, slug);
        entries.push(ChangelogEntry {
            year: year.to_string(),
            slug,
            version: ver,
            date,
            content: buf.join("\n"),
        });
        let _ = doc_base;
    };

    for child in root.children() {
        match child.value() {
            Node::Element(el_val) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    let tag = el_val.name();
                    match tag {
                        "h3" => {
                            let text = child_el.text().collect::<String>().trim().to_string();
                            let is_year = text.len() == 4 && text.chars().all(|c| c.is_ascii_digit());
                            if is_year {
                                // Flush current entry before switching year
                                flush(&mut entries, &current_year, &entry_date_text, &current_version, &buf);
                                buf.clear();
                                entry_date_text.clear();
                                entry_id.clear();
                                current_version = None;
                                first_p_seen = false;
                                current_year = text;
                            }
                            // Skip non-year h3 headings (e.g. "Recent changes")
                        }
                        "h4" => {
                            // Flush previous entry
                            flush(&mut entries, &current_year, &entry_date_text, &current_version, &buf);
                            buf.clear();
                            current_version = None;
                            first_p_seen = false;

                            entry_date_text = child_el.text().collect::<String>().trim().to_string();
                            let anchor_sel = Selector::parse("a[name]").unwrap();
                            entry_id = child_el.value().attr("id")
                                .map(|s| s.to_string())
                                .or_else(|| child_el.select(&anchor_sel).next()
                                    .and_then(|a| a.value().attr("name"))
                                    .map(|s| s.to_string()))
                                .unwrap_or_default();
                            // h4 heading itself goes into content as h1
                            buf.push(format!("# {}\n", entry_date_text));
                        }
                        "p" if !entry_date_text.is_empty() => {
                            let p_text = child_el.text().collect::<String>();
                            // Extract version from first paragraph
                            if !first_p_seen {
                                first_p_seen = true;
                                if let Some(ver) = extract_version(&p_text) {
                                    current_version = Some(ver.clone());
                                    // Rewrite h1 to include version
                                    if let Some(first) = buf.first_mut() {
                                        *first = format!("# {}\n", ver);
                                    }
                                }
                            }
                            let doc_base = format!("{}/{}", dir, current_year);
                            buf.push(element_to_md(child_el, index, &doc_base));
                        }
                        _ if !entry_date_text.is_empty() => {
                            let doc_base = format!("{}/{}", dir, current_year);
                            buf.push(element_to_md(child_el, index, &doc_base));
                        }
                        _ => {}
                    }
                }
            }
            Node::Text(t) => {
                if !t.trim().is_empty() && !entry_date_text.is_empty() {
                    buf.push(t.to_string());
                }
            }
            _ => {}
        }
    }
    // Flush last entry
    flush(&mut entries, &current_year, &entry_date_text, &current_version, &buf);

    entries
}

fn build_index(dir: &str, sections: &[(String, String, String)], source_url: &str, tags: &[&str]) -> String {
    let fm = frontmatter(&format!("{} - Index", dir), source_url, tags);
    let mut body = format!("# {}\n\n", dir);
    for (slug, title, _) in sections {
        body.push_str(&format!("- [[{}/{}|{}]]\n", dir, slug, title));
    }
    format!("{}{}", fm, body)
}

fn changelog_frontmatter(version: &str, date: &str, source_url: &str) -> String {
    format!(
        "---\ntitle: \"{}\"\ndate: {}\nsource: {}\ntags:\n  - changelog\n---\n\n",
        version, date, source_url
    )
}

fn build_year_index(dir: &str, year: &str, entries: &[&ChangelogEntry], source_url: &str) -> String {
    let fm = frontmatter(&format!("Changelog {}", year), source_url, &["changelog"]);
    let mut body = format!("# Changelog {}\n\n", year);
    for e in entries {
        body.push_str(&format!("- [[{}/{}/{}|{}]] - {}\n", dir, year, e.slug, e.version, e.date));
    }
    format!("{}{}", fm, body)
}

fn build_changelog_index(
    dir: &str,
    by_year: &std::collections::BTreeMap<&str, Vec<&ChangelogEntry>>,
    source_url: &str,
) -> String {
    let fm = frontmatter("Changelog", source_url, &["changelog"]);
    let mut body = "# Changelog\n\n".to_string();
    for year in by_year.keys().rev() {
        body.push_str(&format!("- [[{}/{}/index|{}]]\n", dir, year, year));
    }
    format!("{}{}", fm, body)
}

fn write_file(path: &str, content: &str) -> Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}
