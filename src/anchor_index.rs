use std::collections::HashMap;

/// Maps anchor (lowercase) -> relative doc path (e.g. "api/types.md#Message")
pub struct AnchorIndex {
    map: HashMap<String, String>,
}

impl AnchorIndex {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    /// Register all anchors that belong to a given output file.
    /// heading_text is the display name (used for the fragment after #).
    pub fn register(&mut self, anchor: &str, doc_path: &str, heading_text: &str) {
        // anchor is already lowercase id from HTML
        // doc_path is like "api/types" (no .md)
        // heading_text is the human name, e.g. "Message"
        let key = anchor.to_lowercase();
        let value = format!("{}#{}", doc_path, heading_text);
        self.map.insert(key, value);
    }

    /// Resolve an href like "/bots/api#message" or "#sendmessage" to an
    /// Obsidian wiki-link like [[api/types#Message]], or None if unknown.
    pub fn resolve(&self, href: &str, current_page_base: &str) -> Option<String> {
        // Extract the anchor part
        let anchor = if let Some(pos) = href.rfind('#') {
            href[pos + 1..].to_lowercase()
        } else {
            return None;
        };

        if anchor.is_empty() {
            return None;
        }

        if let Some(target) = self.map.get(&anchor) {
            // Same file - just use #heading
            let target_file = target.split('#').next().unwrap_or("");
            if target_file == current_page_base {
                let heading = target.split('#').nth(1).unwrap_or(&anchor);
                return Some(format!("[[#{}]]", heading));
            }
            return Some(format!("[[{}]]", target));
        }
        None
    }
}
