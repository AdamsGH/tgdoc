use scraper::{ElementRef, Html, Node, Selector};
use crate::anchor_index::AnchorIndex;

/// Parse h2/h3/h4 headings in document order (single DOM traversal).
pub fn extract_headings(html: &str) -> Vec<(u8, String, String)> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("h2, h3, h4").unwrap();
    let anchor_sel = Selector::parse("a[name]").unwrap();
    doc.select(&sel).map(|el| {
        let level = heading_level(el.value().name()).unwrap_or(2);
        let id = el.value().attr("id")
            .map(|s| s.to_string())
            .or_else(|| el.select(&anchor_sel).next()
                .and_then(|a| a.value().attr("name"))
                .map(|s| s.to_string()))
            .unwrap_or_default();
        let text = el.text().collect::<String>().trim().to_string();
        (level, id, text)
    }).collect()
}


fn heading_level(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        _ => None,
    }
}

/// Recursively convert an element to Markdown text.
pub fn element_to_md(el: ElementRef, index: &AnchorIndex, doc_base: &str) -> String {
    let tag = el.value().name();
    match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" => {
            let level = heading_level(tag).unwrap() as usize;
            let text = inline_children(el, index, doc_base);
            let id = el.value().attr("id").unwrap_or("");
            if id.is_empty() {
                format!("{} {}\n", "#".repeat(level), text)
            } else {
                format!("{} {}\n", "#".repeat(level), text)
            }
        }
        "p" => {
            let text = inline_children(el, index, doc_base);
            if text.trim().is_empty() {
                String::new()
            } else {
                format!("{}\n\n", text)
            }
        }
        "br" => "\n".to_string(),
        "ul" => {
            let mut out = String::new();
            for child in el.children() {
                if let Some(li) = ElementRef::wrap(child) {
                    if li.value().name() == "li" {
                        let text = block_children(li, index, doc_base);
                        for (i, line) in text.lines().enumerate() {
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
                        for (i, line) in text.lines().enumerate() {
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
        "pre" | "code" if tag == "pre" => {
            // Look for inner code element
            let code_sel = Selector::parse("code").unwrap();
            let code_text = if let Some(code) = el.select(&code_sel).next() {
                code.text().collect::<String>()
            } else {
                el.text().collect::<String>()
            };
            // Detect language from class
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
            let quoted = inner.lines()
                .map(|l| format!("> {}", l))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{}\n\n", quoted)
        }
        "table" => table_to_md(el, index, doc_base),
        "a" => {
            let href = el.value().attr("href").unwrap_or("");
            let text = inline_children(el, index, doc_base);
            // Try to resolve internal link
            if href.contains("core.telegram.org") || href.starts_with('/') || href.starts_with('#') {
                if let Some(wiki) = index.resolve(href, doc_base) {
                    if text.trim() == wiki.trim_start_matches("[[").trim_end_matches("]]")
                        .split('#').last().unwrap_or("") {
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
            format!("**{}**", text)
        }
        "em" | "i" => {
            let text = inline_children(el, index, doc_base);
            format!("*{}*", text)
        }
        "img" => {
            let alt = el.value().attr("alt").unwrap_or("");
            let src = el.value().attr("src").unwrap_or("");
            format!("![{}]({})", alt, src)
        }
        "hr" => "\n---\n\n".to_string(),
        "div" | "section" | "article" | "aside" => {
            block_children(el, index, doc_base)
        }
        "span" => inline_children(el, index, doc_base),
        "li" => {
            block_children(el, index, doc_base)
        }
        _ => block_children(el, index, doc_base),
    }
}

fn inline_children(el: ElementRef, index: &AnchorIndex, doc_base: &str) -> String {
    let mut out = String::new();
    for child in el.children() {
        match child.value() {
            Node::Text(t) => {
                out.push_str(&t.to_string());
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
        let cells: Vec<String> = row.select(&th_sel).chain(row.select(&td_sel))
            .map(|cell| {
                inline_children(cell, index, doc_base)
                    .replace('\n', " ")
                    .replace('|', "\\|")
                    .trim()
                    .to_string()
            })
            .collect();
        cells
    }).collect();

    if rows.is_empty() {
        return String::new();
    }

    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return String::new();
    }

    let mut out = String::new();
    let header = &rows[0];
    let padded_header: Vec<String> = (0..col_count)
        .map(|i| header.get(i).cloned().unwrap_or_default())
        .collect();
    out.push_str(&format!("| {} |\n", padded_header.join(" | ")));
    out.push_str(&format!("|{}|\n", vec!["---|"; col_count].join("")));

    for row in rows.iter().skip(1) {
        let padded: Vec<String> = (0..col_count)
            .map(|i| row.get(i).cloned().unwrap_or_default())
            .collect();
        out.push_str(&format!("| {} |\n", padded.join(" | ")));
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
