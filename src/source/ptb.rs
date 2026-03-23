use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use anyhow::Result;
use regex::Regex;

use crate::anchor_index::AnchorIndex;
use crate::config::SourceConfig;
use crate::driver::RawData;


pub async fn run(cfg: &SourceConfig, raw: RawData, out_dir: &str, dry: bool) -> Result<()> {
    let repo = match raw {
        RawData::Repo(p) => p,
        _ => anyhow::bail!("ptb parser expects Repo data"),
    };

    let src = repo.join("src").join("telegram");
    let changes = repo.join("changes");
    let out = PathBuf::from(out_dir).join(&cfg.out);

    let mut index = AnchorIndex::new();

    // collect all class files first so we can build the index before writing
    let classes = collect_classes(&src)?;
    for cls in &classes {
        let doc_path = class_doc_path(&cls.namespace, &cls.name);
        index.register(&cls.name.to_lowercase(), &doc_path, &cls.name);
        for m in &cls.methods {
            let anchor = format!("{}.{}", cls.name.to_lowercase(), m.name.to_lowercase());
            let fragment = format!("{}#{}", doc_path, m.name);
            index.register(&anchor, &fragment, &m.name);
        }
    }

    if dry {
        for cls in &classes {
            println!("[ptb] {}.{} ({} methods, {} attrs)",
                cls.namespace, cls.name,
                cls.methods.len(), cls.attrs.len());
        }
        let entries = collect_changelog(&changes)?;
        println!("[ptb] {} changelog entries", entries.len());
        return Ok(());
    }

    // write class files
    for cls in &classes {
        let doc_path = class_doc_path(&cls.namespace, &cls.name);
        let path = out.join(format!("{}.md", doc_path));
        let md = render_class(cls, &index, cfg, &doc_path);
        write_file(&path, &md)?;
        println!("[write] {}", path.display());
    }

    // write namespace index files
    write_namespace_indexes(&classes, &out, cfg)?;

    // changelog
    let entries = collect_changelog(&changes)?;
    write_changelog(&entries, &out, cfg)?;

    println!("\nDone. PTB docs written to {}/", out.display());
    Ok(())
}


#[derive(Debug)]
struct ClassDef {
    /// "telegram" or "telegram.ext" etc.
    namespace: String,
    name: String,
    docstring: String,
    bases: Vec<String>,
    attrs: Vec<AttrDef>,
    methods: Vec<MethodDef>,
    /// source file relative to repo root
    source_file: String,
}

#[derive(Debug)]
struct AttrDef {
    name: String,
    type_hint: String,
    docstring: String,
}

#[derive(Debug)]
struct MethodDef {
    name: String,
    is_async: bool,
    signature: String,
    docstring: String,
}

#[derive(Debug)]
struct ChangelogEntry {
    version: String,
    date: String,
    sections: BTreeMap<String, Vec<String>>,
}


fn collect_classes(src: &Path) -> Result<Vec<ClassDef>> {
    let mut classes = Vec::new();
    collect_classes_in(src, "telegram", &mut classes)?;
    Ok(classes)
}

fn collect_classes_in(dir: &Path, namespace: &str, out: &mut Vec<ClassDef>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<std::result::Result<_, _>>()?;
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            // skip private/internal dirs
            if name.starts_with('_') || name == "__pycache__" {
                continue;
            }
            let child_ns = format!("{}.{}", namespace, name);
            collect_classes_in(&path, &child_ns, out)?;
        } else if path.extension().map(|e| e == "py").unwrap_or(false) {
            let fname = path.file_name().unwrap_or_default().to_string_lossy();
            if fname.starts_with("__") { continue; }
            let text = std::fs::read_to_string(&path)?;
            let mut parsed = parse_python_classes(&text, namespace, &path);
            out.append(&mut parsed);
        }
    }
    Ok(())
}

fn parse_python_classes(src: &str, namespace: &str, path: &Path) -> Vec<ClassDef> {
    let mut classes = Vec::new();

    // find class definitions (handles multi-line bases)
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        if line.starts_with("class ") && line.contains('(') {
            // collect until closing '):'
            let mut class_header = line.to_string();
            let mut j = i;
            while !class_header.contains("):") && j + 1 < lines.len() {
                j += 1;
                class_header.push(' ');
                class_header.push_str(lines[j].trim());
            }

            // pass j (last line of class header) so body starts at j+1
            if let Some(cls) = parse_class_header(&class_header, src, &lines, j, namespace, path) {
                classes.push(cls);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    classes
}

fn parse_class_header(
    header: &str,
    src: &str,
    lines: &[&str],
    class_line: usize,
    namespace: &str,
    path: &Path,
) -> Option<ClassDef> {
    let re_class = Regex::new(r"class\s+(\w+)\s*\(([^)]*)\)").ok()?;
    let caps = re_class.captures(header)?;
    let name = caps[1].to_string();

    // skip private/test classes
    if name.starts_with('_') { return None; }

    let bases_raw = &caps[2];
    let bases: Vec<String> = bases_raw.split(',')
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty())
        .collect();

    // docstring: first triple-quoted string after class def
    let class_docstring = extract_docstring_after(lines, class_line);

    // source_file relative to repo root (best-effort)
    let source_file = path.to_string_lossy().to_string();

    let mut attrs = Vec::new();
    let mut methods = Vec::new();

    // parse members at class body indent (4 spaces typically)
    parse_class_body(src, &name, lines, class_line, &mut attrs, &mut methods);

    Some(ClassDef {
        namespace: namespace.to_string(),
        name,
        docstring: class_docstring,
        bases,
        attrs,
        methods,
        source_file,
    })
}

fn parse_class_body(
    _src: &str,
    _class_name: &str,
    lines: &[&str],
    class_line: usize,
    attrs: &mut Vec<AttrDef>,
    methods: &mut Vec<MethodDef>,
) {
    // class body members sit at class_indent + 4
    let class_indent = lines[class_line].len() - lines[class_line].trim_start().len();
    let member_indent = class_indent + 4;

    // match just "def name(" - signature collected separately via collect_signature
    let re_method = Regex::new(r"^(async\s+)?def\s+(\w+)\s*\(").unwrap();
    let re_attr = Regex::new(r"^(\w+):\s*(.+)").unwrap();

    let mut i = class_line + 1;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() { i += 1; continue; }

        let indent = line.len() - line.trim_start().len();

        // back to class level or higher = end of class body
        if indent <= class_indent && !line.trim().is_empty() { break; }

        // only look at direct class members, skip nested blocks
        if indent != member_indent { i += 1; continue; }

        let stripped = line.trim_start();

        if let Some(caps) = re_method.captures(stripped) {
            let is_async = caps.get(1).is_some();
            let mname = caps[2].to_string();
            let sig = collect_signature(lines, i);
            let docstring = extract_docstring_after(lines, i);
            if !mname.starts_with('_') || mname.starts_with("__") {
                methods.push(MethodDef {
                    name: mname,
                    is_async,
                    signature: sig,
                    docstring,
                });
            }
            // skip past method body: stop only when we hit the next member-level def/class
            let body_start = i + 1;
            i = body_start;
            while i < lines.len() {
                let l = lines[i];
                if l.trim().is_empty() { i += 1; continue; }
                let ind = l.len() - l.trim_start().len();
                if ind < member_indent { break; } // back to class or module level
                if ind == member_indent {
                    let s = l.trim_start();
                    if s.starts_with("def ") || s.starts_with("async def ")
                        || s.starts_with("class ") || s.starts_with('@')
                    {
                        break;
                    }
                }
                i += 1;
            }
            continue;
        }

        // annotated class-level attribute
        if let Some(caps) = re_attr.captures(stripped) {
            let aname = caps[1].to_string();
            if !aname.starts_with('_') && aname != "def" && aname != "class" {
                let type_hint = caps[2].split('=').next().unwrap_or("").trim().to_string();
                attrs.push(AttrDef { name: aname, type_hint, docstring: String::new() });
            }
        }

        i += 1;
    }
}

/// Collect the full parameter list for a def, handling multi-line signatures.
fn collect_signature(lines: &[&str], def_line: usize) -> String {
    let mut buf = lines[def_line].trim_start().to_string();
    let mut i = def_line + 1;
    // if opening paren not closed on first line, keep appending
    while !buf.contains("):") && !buf.ends_with("):") && i < lines.len() {
        buf.push(' ');
        buf.push_str(lines[i].trim());
        i += 1;
        if i - def_line > 30 { break; } // safety limit
    }
    // extract just the params between first ( and last )
    if let Some(start) = buf.find('(') {
        if let Some(end) = buf.rfind(')') {
            return buf[start + 1..end].trim().to_string();
        }
    }
    String::new()
}

/// Find the line index just past the end of the indented block starting at `start`.

/// Extract the first triple-quoted docstring following line `after`.
fn extract_docstring_after(lines: &[&str], after: usize) -> String {
    let mut i = after + 1;
    // skip blank lines
    while i < lines.len() && lines[i].trim().is_empty() { i += 1; }
    if i >= lines.len() { return String::new(); }

    let first = lines[i].trim();
    let delim = if first.starts_with("\"\"\"") { "\"\"\"" }
                else if first.starts_with("'''") { "'''" }
                else { return String::new(); };

    let start_content = first.trim_start_matches(delim);
    // single-line docstring
    if start_content.ends_with(delim) && start_content.len() > delim.len() {
        return start_content.trim_end_matches(delim).trim().to_string();
    }

    let mut buf = vec![start_content.to_string()];
    i += 1;
    while i < lines.len() {
        let l = lines[i].trim();
        if l.ends_with(delim) {
            buf.push(l.trim_end_matches(delim).to_string());
            break;
        }
        buf.push(l.to_string());
        i += 1;
    }
    buf.join("\n").trim().to_string()
}


fn collect_changelog(changes_dir: &Path) -> Result<Vec<ChangelogEntry>> {
    let mut entries = Vec::new();

    if !changes_dir.exists() { return Ok(entries); }

    // versioned dirs: "22.7_2026-03-16"
    let re_dir = Regex::new(r"^(\d+\.\d+(?:\.\d+)?)_(\d{4}-\d{2}-\d{2})$").unwrap();
    let mut dirs: Vec<_> = std::fs::read_dir(changes_dir)?
        .flatten()
        .filter(|e| e.path().is_dir())
        .collect();
    dirs.sort_by_key(|e| e.path());

    for dir_entry in dirs {
        let dname = dir_entry.file_name().to_string_lossy().to_string();
        if let Some(caps) = re_dir.captures(&dname) {
            let version = caps[1].to_string();
            let date = caps[2].to_string();
            let sections = collect_toml_sections(&dir_entry.path())?;
            entries.push(ChangelogEntry { version, date, sections });
        }
    }

    // LEGACY.rst - versions < 22.0
    let legacy = changes_dir.join("LEGACY.rst");
    if legacy.exists() {
        let mut legacy_entries = parse_legacy_rst(&legacy)?;
        legacy_entries.extend(entries);
        entries = legacy_entries;
    }

    Ok(entries)
}

/// Read all .toml files in a version directory and group by section key.
fn collect_toml_sections(dir: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let mut sections: BTreeMap<String, Vec<String>> = BTreeMap::new();

    // section display order matches changes/config.py
    let section_order = [
        "highlights", "breaking", "security", "deprecations",
        "features", "bugfixes", "dependencies", "other",
        "documentation", "internal",
    ];

    for section in &section_order {
        sections.insert(section.to_string(), Vec::new());
    }

    let toml_re = Regex::new(r"\.toml$").unwrap();
    let mut files: Vec<_> = std::fs::read_dir(dir)?.flatten()
        .filter(|e| toml_re.is_match(&e.file_name().to_string_lossy()))
        .collect();
    files.sort_by_key(|e| e.path());

    for file in files {
        let text = std::fs::read_to_string(file.path())?;
        let value: toml::Value = toml::from_str(&text)?;
        if let toml::Value::Table(table) = &value {
            for key in &section_order {
                if let Some(toml::Value::String(s)) = table.get(*key) {
                    let item = s.trim().to_string();
                    if !item.is_empty() {
                        sections.entry(key.to_string()).or_default().push(item);
                    }
                }
            }
        }
    }

    // remove empty sections
    sections.retain(|_, v| !v.is_empty());
    Ok(sections)
}

/// Parse LEGACY.rst into ChangelogEntry items.
fn parse_legacy_rst(path: &Path) -> Result<Vec<ChangelogEntry>> {
    let text = std::fs::read_to_string(path)?;
    let mut entries = Vec::new();

    // version header: "Version 21.11" or "Version 21.11.1" on its own line
    let re_ver = Regex::new(r"^Version (\d+\.\d+(?:\.\d+)?)").unwrap();
    // date line: "*Released YYYY-MM-DD*"
    let re_date = Regex::new(r"\*Released (\d{4}-\d{2}-\d{2})\*").unwrap();
    // section header underlined with dashes: "Bug Fixes\n---------"
    let re_section_underline = Regex::new(r"^-+$").unwrap();

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        if let Some(caps) = re_ver.captures(lines[i]) {
            let version = caps[1].to_string();
            let mut date = String::new();
            let mut sections: BTreeMap<String, Vec<String>> = BTreeMap::new();
            let mut current_section = "other".to_string();

            i += 1;
            // skip the underline (===)
            if i < lines.len() && lines[i].starts_with('=') { i += 1; }

            while i < lines.len() {
                let line = lines[i];

                // next version block
                if re_ver.is_match(line) { break; }

                // date
                if let Some(caps) = re_date.captures(line) {
                    date = caps[1].to_string();
                    i += 1;
                    continue;
                }

                // section heading (text on prev line, this line is dashes)
                if re_section_underline.is_match(line) && i > 0 {
                    let heading = lines[i - 1].trim();
                    if !heading.is_empty() && !heading.starts_with('*') {
                        current_section = heading.to_lowercase()
                            .replace(' ', "-")
                            .replace([',', '&'], "");
                        i += 1;
                        continue;
                    }
                }

                // bullet item
                let trimmed = line.trim();
                if trimmed.starts_with('-') || trimmed.starts_with('*') {
                    let item = trimmed.trim_start_matches(|c| c == '-' || c == '*').trim().to_string();
                    if !item.is_empty() {
                        sections.entry(current_section.clone()).or_default().push(item);
                    }
                }

                i += 1;
            }

            if !date.is_empty() || !sections.is_empty() {
                entries.push(ChangelogEntry { version, date, sections });
            }
        } else {
            i += 1;
        }
    }

    Ok(entries)
}


fn class_doc_path(namespace: &str, class_name: &str) -> String {
    // "telegram" -> "telegram/Bot"
    // "telegram.ext" -> "telegram.ext/Application"
    format!("{}/{}", namespace.replace('.', "/"), class_name)
}

fn render_class(cls: &ClassDef, index: &AnchorIndex, cfg: &SourceConfig, doc_path: &str) -> String {
    let repo_rel = cls.source_file
        .split_once("src/telegram")
        .map(|(_, rest)| format!("src/telegram{}", rest))
        .unwrap_or_else(|| cls.source_file.clone());
    let source_url = format!(
        "https://github.com/python-telegram-bot/python-telegram-bot/blob/master/{}",
        repo_rel
    );
    let ns_tag = cfg.out.clone() + "-" + &cls.namespace.replace('.', "-");
    let mut md = format!(
        "---\ntitle: {}\nsource: {}\ntags:\n  - ptb\n  - {}\n---\n\n",
        cls.name, source_url, ns_tag
    );

    md.push_str(&format!("# {}\n\n", cls.name));

    if !cls.bases.is_empty() {
        let bases_str = cls.bases.iter()
            .map(|b| {
                // strip module prefix for display, keep full for potential link
                let short = b.split('.').last().unwrap_or(b);
                format!("`{}`", short)
            })
            .collect::<Vec<_>>()
            .join(", ");
        md.push_str(&format!("**Inherits:** {}\n\n", bases_str));
    }

    if !cls.docstring.is_empty() {
        md.push_str(&rst_to_md_with_links(&cls.docstring, index, doc_path, &cfg.out));
        md.push_str("\n\n");
    }

    if !cls.attrs.is_empty() {
        md.push_str("## Attributes\n\n");
        for attr in &cls.attrs {
            md.push_str(&format!("### {}\n\n", attr.name));
            if !attr.type_hint.is_empty() {
                md.push_str(&format!("**Type:** `{}`\n\n", attr.type_hint));
            }
            if !attr.docstring.is_empty() {
                md.push_str(&rst_to_md_with_links(&attr.docstring, index, doc_path, &cfg.out));
                md.push('\n');
            }
        }
    }

    if !cls.methods.is_empty() {
        md.push_str("## Methods\n\n");
        for method in &cls.methods {
            md.push_str(&format!("### {}\n\n", method.name));
            let async_kw = if method.is_async { "async " } else { "" };
            md.push_str(&format!("```python\n{}def {}({})\n```\n\n",
                async_kw, method.name, method.signature));
            if !method.docstring.is_empty() {
                md.push_str(&rst_to_md_with_links(&method.docstring, index, doc_path, &cfg.out));
                md.push('\n');
            }
        }
    }

    md
}

/// RST → MD with Obsidian wiki-link resolution for :class:/:meth: refs.
fn rst_to_md_with_links(rst: &str, index: &AnchorIndex, current_doc: &str, source_prefix: &str) -> String {
    let _text = rst_to_md(rst); // kept for reference; actual processing below uses original RST

    // after rst_to_md, :class:`telegram.Bot` becomes `Bot` (backtick inline code)
    // but before that substitution we can intercept the original RST refs
    // Re-process the original RST for link candidates
    let re_class = Regex::new(r":class:`([^`]+)`").unwrap();
    let re_meth  = Regex::new(r":meth:`([^`]+)`").unwrap();

    let mut out = rst.to_string();

    // Replace :class:`telegram.Foo` with wiki-link if in index, else plain `Foo`
    out = re_class.replace_all(&out, |caps: &regex::Captures| {
        let full = &caps[1];
        let short = full.split('.').last().unwrap_or(full);
        let key = short.to_lowercase();
        if let Some(wiki) = index.resolve(&format!("#{}", key), current_doc) {
            format!("[{}]({})", short, wiki)
        } else {
            // try constructing path directly from namespace
            let ns_path = full.replace('.', "/");
            format!("[[{}/{}|{}]]", source_prefix, ns_path, short)
        }
    }).to_string();

    out = re_meth.replace_all(&out, |caps: &regex::Captures| {
        let full = &caps[1];
        let parts: Vec<&str> = full.rsplitn(2, '.').collect();
        let (method, class_path) = if parts.len() == 2 {
            (parts[0], Some(parts[1]))
        } else {
            (parts[0], None)
        };
        let key = format!("{}.{}", 
            class_path.unwrap_or("").split('.').last().unwrap_or("").to_lowercase(),
            method.to_lowercase());
        if let Some(wiki) = index.resolve(&format!("#{}", key), current_doc) {
            format!("[{}]({})", method, wiki)
        } else {
            format!("`{}`", method)
        }
    }).to_string();

    // now apply the rest of rst_to_md (everything except the :class:/:meth: we already handled)
    rst_to_md_no_class_meth(&out)
}

/// Same as rst_to_md but skips :class: and :meth: (already resolved by caller).
fn rst_to_md_no_class_meth(rst: &str) -> String {
    let mut out = rst.to_string();
    let re_attr = Regex::new(r":attr:`([^`]+)`").unwrap();
    out = re_attr.replace_all(&out, "`$1`").to_string();
    let re_obj = Regex::new(r":obj:`([^`]+)`").unwrap();
    out = re_obj.replace_all(&out, "`$1`").to_string();
    let re_any = Regex::new(r":(?:any|ref|wiki):`([^<`]+)(?:\s*<[^>]+>)?`").unwrap();
    out = re_any.replace_all(&out, "`$1`").to_string();
    out = convert_rst_code_blocks(&out);
    let re_version = Regex::new(r"\.\. version(?:added|changed|deprecated)::\s*(\S+)([^\n]*)").unwrap();
    out = re_version.replace_all(&out, "> *$0*").to_string();
    let re_seealso = Regex::new(r"\.\. seealso::([^\n]*)").unwrap();
    out = re_seealso.replace_all(&out, "**See also:**$1").to_string();
    let re_admonition = Regex::new(r"\.\. (note|tip|warning|caution)::([^\n]*)").unwrap();
    out = re_admonition.replace_all(&out, "> **$1:**$2").to_string();
    out
}

/// Minimal RST -> Markdown conversion for docstrings.
fn rst_to_md(rst: &str) -> String {
    let mut out = rst.to_string();

    // :class:`telegram.Bot` -> `Bot`
    let re_class = Regex::new(r":class:`([^`]+)`").unwrap();
    out = re_class.replace_all(&out, |caps: &regex::Captures| {
        let name = caps[1].split('.').last().unwrap_or(&caps[1]).to_string();
        format!("`{}`", name)
    }).to_string();

    // :meth:`telegram.Bot.send_message` -> `send_message`
    let re_meth = Regex::new(r":meth:`([^`]+)`").unwrap();
    out = re_meth.replace_all(&out, |caps: &regex::Captures| {
        let name = caps[1].split('.').last().unwrap_or(&caps[1]).to_string();
        format!("`{}`", name)
    }).to_string();

    // :attr:`foo` -> `foo`
    let re_attr = Regex::new(r":attr:`([^`]+)`").unwrap();
    out = re_attr.replace_all(&out, "`$1`").to_string();

    // :obj:`True` -> `True`
    let re_obj = Regex::new(r":obj:`([^`]+)`").unwrap();
    out = re_obj.replace_all(&out, "`$1`").to_string();

    // :any:`label <target>` -> `label`
    let re_any = Regex::new(r":(?:any|ref|wiki):`([^<`]+)(?:\s*<[^>]+>)?`").unwrap();
    out = re_any.replace_all(&out, "`$1`").to_string();

    // .. code:: python / .. code-block:: python -> fenced code block
    out = convert_rst_code_blocks(&out);

    // .. versionadded/changed/deprecated -> blockquote note
    let re_version = Regex::new(r"\.\. version(?:added|changed|deprecated)::\s*(\S+)([^\n]*)").unwrap();
    out = re_version.replace_all(&out, "> *$0*").to_string();

    // .. seealso:: -> **See also:**
    let re_seealso = Regex::new(r"\.\. seealso::([^\n]*)").unwrap();
    out = re_seealso.replace_all(&out, "**See also:**$1").to_string();

    // .. note:: / .. tip:: / .. warning:: -> blockquote
    let re_admonition = Regex::new(r"\.\. (note|tip|warning|caution)::([^\n]*)").unwrap();
    out = re_admonition.replace_all(&out, "> **$1:**$2").to_string();

    out
}

fn render_changelog_entry(entry: &ChangelogEntry, out_prefix: &str) -> String {
    let source_url = format!(
        "https://github.com/python-telegram-bot/python-telegram-bot/blob/master/CHANGES.rst"
    );
    let date = if entry.date.is_empty() { "unknown".to_string() } else { entry.date.clone() };
    let mut md = format!(
        "---\ntitle: \"PTB v{}\"\ndate: {}\nsource: {}\ntags:\n  - ptb\n  - changelog\n---\n\n",
        entry.version, date, source_url
    );

    md.push_str(&format!("# PTB v{}\n\n", entry.version));
    if !date.is_empty() && date != "unknown" {
        md.push_str(&format!("*Released: {}*\n\n", date));
    }

    let section_titles = [
        ("highlights",    "Highlights"),
        ("breaking",      "Breaking Changes"),
        ("security",      "Security"),
        ("deprecations",  "Deprecations"),
        ("features",      "New Features"),
        ("bugfixes",      "Bug Fixes"),
        ("dependencies",  "Dependencies"),
        ("other",         "Other Changes"),
        ("documentation", "Documentation"),
        ("internal",      "Internal"),
    ];

    for (key, title) in &section_titles {
        if let Some(items) = entry.sections.get(*key) {
            if !items.is_empty() {
                md.push_str(&format!("## {}\n\n", title));
                for item in items {
                    md.push_str(&format!("- {}\n", item.replace('\n', "\n  ")));
                }
                md.push('\n');
            }
        }
    }

    let _ = out_prefix;
    md
}

fn write_changelog(entries: &[ChangelogEntry], out: &Path, cfg: &SourceConfig) -> Result<()> {
    let cl_dir = out.join("changelog");

    // group by year for the index
    let mut by_year: BTreeMap<String, Vec<&ChangelogEntry>> = BTreeMap::new();
    for e in entries {
        let year = e.date.get(..4).unwrap_or("misc").to_string();
        by_year.entry(year).or_default().push(e);
    }

    for e in entries {
        let year = e.date.get(..4).unwrap_or("misc");
        let slug = format!("v{}", e.version);
        let path = cl_dir.join(year).join(format!("{}.md", slug));
        let md = render_changelog_entry(e, &cfg.out);
        write_file(&path, &md)?;
        println!("[write] {}", path.display());
    }

    // per-year index
    let source_url = "https://github.com/python-telegram-bot/python-telegram-bot/blob/master/CHANGES.rst";
    for (year, year_entries) in &by_year {
        let idx_path = cl_dir.join(year).join("index.md");
        let mut md = format!("---\ntitle: PTB Changelog {year}\nsource: {source_url}\ntags:\n  - ptb\n  - changelog\n---\n\n# PTB Changelog {year}\n\n");
        for e in year_entries.iter().rev() {
            let slug = format!("v{}", e.version);
            md.push_str(&format!("- [[{}/changelog/{}/{}|PTB v{}]] - {}\n",
                cfg.out, year, slug, e.version, e.date));
        }
        write_file(&idx_path, &md)?;
    }

    // top-level changelog index
    let idx_path = cl_dir.join("index.md");
    let mut md = format!("---\ntitle: PTB Changelog\nsource: {source_url}\ntags:\n  - ptb\n  - changelog\n---\n\n# PTB Changelog\n\n");
    for year in by_year.keys().rev() {
        md.push_str(&format!("- [[{}/changelog/{}/index|{}]]\n", cfg.out, year, year));
    }
    write_file(&idx_path, &md)?;

    Ok(())
}

fn write_namespace_indexes(classes: &[ClassDef], out: &Path, cfg: &SourceConfig) -> Result<()> {
    let mut by_ns: BTreeMap<String, Vec<&ClassDef>> = BTreeMap::new();
    for cls in classes {
        by_ns.entry(cls.namespace.clone()).or_default().push(cls);
    }

    for (ns, ns_classes) in &by_ns {
        let ns_path = ns.replace('.', "/");
        let idx_path = out.join(&ns_path).join("index.md");
        let source_url = "https://github.com/python-telegram-bot/python-telegram-bot";
        let mut md = format!(
            "---\ntitle: {ns}\nsource: {source_url}\ntags:\n  - ptb\n  - {}\n---\n\n# {ns}\n\n",
            ns.replace('.', "-")
        );
        for cls in ns_classes.iter() {
            md.push_str(&format!("- [[{}/{}/{}|{}]]\n",
                cfg.out, ns_path, cls.name, cls.name));
        }
        write_file(&idx_path, &md)?;
    }

    // top-level ptb index
    let idx_path = out.join("index.md");
    let source_url = "https://github.com/python-telegram-bot/python-telegram-bot";
    let mut md = format!("---\ntitle: python-telegram-bot\nsource: {source_url}\ntags:\n  - ptb\n---\n\n# python-telegram-bot\n\n");
    for ns in by_ns.keys() {
        let ns_path = ns.replace('.', "/");
        md.push_str(&format!("- [[{}/{}/index|{}]]\n", cfg.out, ns_path, ns));
    }
    md.push_str(&format!("- [[{}/changelog/index|Changelog]]\n", cfg.out));
    write_file(&idx_path, &md)?;

    Ok(())
}


fn convert_rst_code_blocks(text: &str) -> String {
    let re_directive = Regex::new(r"^\s*\.\. code(?:-block)?::?\s*(\w*)$").unwrap();
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();

    while let Some(line) = lines.next() {
        if let Some(caps) = re_directive.captures(line) {
            let lang = caps[1].trim().to_string();
            // skip blank line after directive
            if lines.peek().map(|l| l.trim().is_empty()).unwrap_or(false) {
                lines.next();
            }
            // collect indented body
            let mut body = Vec::new();
            while let Some(next) = lines.peek() {
                if next.trim().is_empty() || next.starts_with("    ") || next.starts_with('\t') {
                    let raw = lines.next().unwrap();
                    let stripped = raw.strip_prefix("    ").unwrap_or(raw).to_string();
                    body.push(stripped);
                } else {
                    break;
                }
            }
            // trim trailing blank lines
            while body.last().map(|l: &String| l.trim().is_empty()).unwrap_or(false) {
                body.pop();
            }
            out.push(format!("```{}", lang));
            out.extend(body);
            out.push("```".to_string());
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}
