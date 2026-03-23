use std::collections::HashMap;
use anyhow::Result;
use crate::config::SourceConfig;
use crate::driver::RawData;
use crate::fetch::{build_client, get_html};

pub async fn fetch(cfg: &SourceConfig) -> Result<RawData> {
    let http = cfg.http.as_ref().expect("http config missing");
    let proxy = http.proxy.as_deref().unwrap_or("");

    let client = if proxy.is_empty() {
        reqwest::Client::builder().gzip(true).build()?
    } else {
        build_client(proxy)?
    };

    let mut pages: HashMap<String, String> = HashMap::new();
    for (path, _) in crate::source::tg_bot_api::PAGE_DEFS {
        let url = format!("{}{}", http.base_url, path);
        println!("[http] fetch {}", url);
        let html = get_html(&client, &url).await?;
        println!("  {} bytes", html.len());
        pages.insert(path.to_string(), html);
    }

    Ok(RawData::Html(pages))
}
