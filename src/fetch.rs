use anyhow::Result;
use reqwest::Client;

pub async fn get_html(client: &Client, url: &str) -> Result<String> {
    let resp = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (X11; Linux x86_64)")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(resp)
}

pub fn build_client(proxy_url: &str) -> Result<Client> {
    let proxy = reqwest::Proxy::all(proxy_url)?;
    let client = Client::builder()
        .proxy(proxy)
        .gzip(true)
        .build()?;
    Ok(client)
}
