//! Xtream Codes panels: `player_api.php` for channels, `xmltv.php` for the guide.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;
use url::Url;

use super::SourceChannel;

pub struct Xtream {
    base: String,
    username: String,
    password: String,
    client: reqwest::Client,
}

/// Panels send ids and numbers as JSON numbers or strings, depending on the vendor.
fn text(v: &Value, key: &str) -> Option<String> {
    match &v[key] {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

impl Xtream {
    pub fn new(base: &str, username: &str, password: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            username: username.to_string(),
            password: password.to_string(),
            client: reqwest::Client::new(),
        }
    }

    fn url(&self, file: &str, action: Option<&str>) -> Result<Url> {
        let mut url = Url::parse(&format!("{}/{file}", self.base))?;
        url.query_pairs_mut()
            .append_pair("username", &self.username)
            .append_pair("password", &self.password)
            .extend_pairs(action.map(|a| ("action", a)));
        Ok(url)
    }

    pub fn xmltv_url(&self) -> String {
        self.url("xmltv.php", None)
            .map(String::from)
            .unwrap_or_default()
    }

    async fn api(&self, action: &str) -> Result<Vec<Value>> {
        let url = self.url("player_api.php", Some(action))?;
        Ok(self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn channels(&self) -> Result<Vec<SourceChannel>> {
        let categories: HashMap<String, String> = self
            .api("get_live_categories")
            .await?
            .iter()
            .filter_map(|c| Some((text(c, "category_id")?, text(c, "category_name")?)))
            .collect();
        Ok(self
            .api("get_live_streams")
            .await?
            .iter()
            .filter_map(|s| {
                let id = text(s, "stream_id")?;
                Some(SourceChannel {
                    number: text(s, "num").and_then(|n| n.parse().ok()),
                    name: text(s, "name")?,
                    logo_url: text(s, "stream_icon"),
                    stream_url: format!(
                        "{}/live/{}/{}/{id}.ts",
                        self.base, self.username, self.password
                    ),
                    epg_channel_id: text(s, "epg_channel_id"),
                    group_title: text(s, "category_id").and_then(|c| categories.get(&c).cloned()),
                })
            })
            .collect())
    }
}
