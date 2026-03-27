use scraper::{ElementRef, Selector, node::Node};
use crate::anchor_index::AnchorIndex;

/// Parse h2/h3/h4 headings in document order (single DOM traversal).
pub fn extract_headings(html: &str) -> Vec<(u8, String, String)> {
    let doc = scraper::Html::parse_document(html);
    let sel = Selector::parse("h2, h3, h4").unwrap();
    let anchor_sel = Selector::parse("a[name]").unwrap();
    let mut out = Vec::new();
    for el in doc.select(&sel) {
        let id = el.value().attr("id")
            .map(|s| s.to_string())
            .or_else(|| el.select(&anchor_sel).next()
                .and_then(|a| a.value().attr("name"))
                .map(|s| s.to_string()));
        let text = heading_text(el);
        if let (Some(id), false) = (id, text.is_empty()) {
            let level = heading_level(el.value().name()).unwrap();
            out.push((level, id, text));
        }
    }
    out
}

fn heading_level(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1), "h2" => Some(2), "h3" => Some(3),
        "h4" => Some(4), "h5" => Some(5), _ => None,
    }
}

/// Text content of a heading, skipping anchor-icon elements.
fn heading_text(el: ElementRef) -> String {
    let mut out = String::new();
    for child in el.children() {
        match child.value() {
            Node::Text(t) => out.push_str(&t),
            Node::Element(e) => {
                // skip <i class="anchor-icon"> and <a class="anchor"> wrappers that contain only icons
                let tag = e.name();
                let class = e.attr("class").unwrap_or("");
                if tag == "i" && class.contains("anchor") { continue; }
                if let Some(child_el) = ElementRef::wrap(child) {
                    // for anchor links used as heading anchors, recurse but skip empty results
                    let inner = heading_text(child_el);
                    // don't emit empty link wrappers
                    if !inner.trim().is_empty() {
                        out.push_str(&inner);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub fn element_to_md(el: ElementRef, index: &AnchorIndex, doc_base: &str) -> String {
    let tag = el.value().name();
    match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" => {
            let level = heading_level(tag).unwrap() as usize;
            let text = heading_text(el);
            format!("{} {}\n\n", "#".repeat(level), text.trim())
        }
        "p" => {
            let text = inline_children(el, index, doc_base);
            if text.trim().is_empty() {
                String::new()
            } else {
                format!("{}\n\n", text.trim_end())
            }
        }
        "br" => "\n".to_string(),
        "ul" => {
            let mut out = String::new();
            for child in el.children() {
                if let Some(li) = ElementRef::wrap(child) {
                    if li.value().name() == "li" {
                        let text = block_children(li, index, doc_base);
                        let trimmed = text.trim_end();
                        for (i, line) in trimmed.lines().enumerate() {
                            if i == 0 {
                                out.push_str(&format!("- {}\n", line));
                            } else if !line.trim().is_empty() {
                                out.push_str(&format!("  {}\n", line));
                            }
                        }
                    }
                }
            }
            out.push('\n');
            out
        }
        "ol" => {
            let mut out = String::new();
            let mut n = 1usize;
            for child in el.children() {
                if let Some(li) = ElementRef::wrap(child) {
                    if li.value().name() == "li" {
                        let text = block_children(li, index, doc_base);
                        let trimmed = text.trim_end();
                        for (i, line) in trimmed.lines().enumerate() {
                            if i == 0 {
                                out.push_str(&format!("{}. {}\n", n, line));
                            } else if !line.trim().is_empty() {
                                out.push_str(&format!("   {}\n", line));
                            }
                        }
                        n += 1;
                    }
                }
            }
            out.push('\n');
            out
        }
        "pre" => {
            let code_sel = Selector::parse("code").unwrap();
            let code_text = if let Some(code) = el.select(&code_sel).next() {
                code.text().collect::<String>()
            } else {
                el.text().collect::<String>()
            };
            let lang = el.select(&code_sel).next()
                .and_then(|c| c.value().attr("class"))
                .and_then(|cls| cls.split_whitespace()
                    .find(|c| c.starts_with("language-"))
                    .map(|c| c.trim_start_matches("language-")))
                .unwrap_or("");
            format!("```{}\n{}\n```\n\n", lang, code_text.trim_end())
        }
        "code" => {
            let text = el.text().collect::<String>();
            format!("`{}`", text)
        }
        "blockquote" => {
            let inner = block_children(el, index, doc_base);
            let quoted = inner.trim_end().lines()
                .map(|l| format!("> {}", l))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{}\n\n", quoted)
        }
        "table" => table_to_md(el, index, doc_base),
        "a" => {
            let href = el.value().attr("href").unwrap_or("");
            let class = el.value().attr("class").unwrap_or("");

            // anchor-only links (heading anchors) - skip, parent heading handles text
            if class.contains("anchor") {
                let text = heading_text(el);
                return text;
            }

            let text = inline_children(el, index, doc_base);

            // empty link (icon-only anchor) - discard
            if text.trim().is_empty() {
                return String::new();
            }

            if href.contains("core.telegram.org") || href.starts_with('/') || href.starts_with('#') {
                if let Some(wiki) = index.resolve(href, doc_base) {
                    let wiki_label = wiki
                        .trim_start_matches("[[")
                        .trim_end_matches("]]")
                        .split('#')
                        .last()
                        .unwrap_or("");
                    if text.trim() == wiki_label {
                        return wiki;
                    }
                    return format!("[{}]({})", text, wiki);
                }
            }
            if href.is_empty() || href == text {
                text
            } else {
                format!("[{}]({})", text, href)
            }
        }
        "strong" | "b" => {
            let text = inline_children(el, index, doc_base);
            if text.trim().is_empty() { return String::new(); }
            format!("**{}**", text)
        }
        "em" | "i" => {
            let class = el.value().attr("class").unwrap_or("");
            if class.contains("anchor") { return String::new(); }
            let text = inline_children(el, index, doc_base);
            if text.trim().is_empty() { return String::new(); }
            format!("*{}*", text)
        }
        "img" => {
            let alt = el.value().attr("alt").unwrap_or("");
            let src = el.value().attr("src").unwrap_or("");
            format!("![{}]({})", alt, src)
        }
        "hr" => "\n---\n\n".to_string(),
        "div" | "section" | "article" | "aside" => block_children(el, index, doc_base),
        "span" => inline_children(el, index, doc_base),
        "li" => block_children(el, index, doc_base),
        _ => block_children(el, index, doc_base),
    }
}

fn inline_children(el: ElementRef, index: &AnchorIndex, doc_base: &str) -> String {
    let mut out = String::new();
    for child in el.children() {
        match child.value() {
            Node::Text(t) => out.push_str(&t),
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

fn block_children(el: ElementRef, index: &AnchorIndex, doc_base: &str) -> String {
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

fn table_to_md(el: ElementRef, index: &AnchorIndex, doc_base: &str) -> String {
    let tr_sel = Selector::parse("tr").unwrap();
    let th_sel = Selector::parse("th").unwrap();
    let td_sel = Selector::parse("td").unwrap();

    let rows: Vec<Vec<String>> = el.select(&tr_sel).map(|row| {
        row.select(&th_sel).chain(row.select(&td_sel))
            .map(|cell| {
                inline_children(cell, index, doc_base)
                    .replace('\n', " ")
                    .replace('|', "\\|")
                    .trim()
                    .to_string()
            })
            .collect()
    }).collect();

    if rows.is_empty() { return String::new(); }

    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 { return String::new(); }

    let mut out = String::new();

    // header row
    let header: Vec<String> = (0..col_count)
        .map(|i| rows[0].get(i).cloned().unwrap_or_default())
        .collect();
    out.push_str(&format!("| {} |\n", header.join(" | ")));

    // separator - use "---" per column, join with " | ", wrap in outer pipes
    let sep: Vec<&str> = vec!["---"; col_count];
    out.push_str(&format!("| {} |\n", sep.join(" | ")));

    // data rows
    for row in rows.iter().skip(1) {
        let cells: Vec<String> = (0..col_count)
            .map(|i| row.get(i).cloned().unwrap_or_default())
            .collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out.push('\n');
    out
}

/// Build frontmatter YAML for a doc file.
pub fn frontmatter(title: &str, source_url: &str, tags: &[&str]) -> String {
    let tag_list = tags.iter().map(|t| format!("  - {}", t)).collect::<Vec<_>>().join("\n");
    format!(
        "---\ntitle: {}\nsource: {}\ntags:\n{}\n---\n\n",
        title, source_url, tag_list
    )
}
