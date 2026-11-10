//! Import watch state from a Plex Media Server.
//!
//! Plex API shapes come from the community OpenAPI spec
//! (<https://github.com/LukeHagar/plex-api-spec>, `plex-api-spec.yaml`):
//! `GET /library/sections` returns `MediaContainer.Directory[] {key, type}`;
//! `GET /library/sections/{sectionId}/all` takes `type` (`MediaType`: 1 = movie, 4 = episode)
//! and `includeGuids` and returns `MediaContainer.Metadata[]` with `viewOffset` (ms),
//! `viewCount`, `lastViewedAt`, `Guid[].id` and `Media[].Part[].file`.
//! Plex's own docs (<https://developer.plex.tv/pms/>) confirm the `X-Plex-Token` header,
//! that JSON needs `Accept: application/json`, and the `provider://id` Guid format
//! (`imdb://tt0088763`, `tmdb://105`).

use anyhow::Result;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Movie,
    Episode,
}

/// One Plex movie or episode with the token owner's watch state.
#[derive(Debug, Clone, PartialEq)]
pub struct PlexEntry {
    pub kind: Kind,
    pub label: String,
    pub files: Vec<String>,
    pub guids: Vec<String>,
    pub view_offset_ms: i64,
    pub view_count: i64,
    pub last_viewed_at: Option<i64>,
}

#[derive(Deserialize)]
struct Container<T> {
    #[serde(rename = "MediaContainer")]
    container: T,
}

#[derive(Deserialize)]
struct Sections {
    #[serde(rename = "Directory", default)]
    directory: Vec<Section>,
}

#[derive(Deserialize)]
struct Section {
    key: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Items {
    #[serde(rename = "Metadata", default)]
    metadata: Vec<Metadata>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    title: String,
    year: Option<i64>,
    grandparent_title: Option<String>,
    parent_index: Option<i64>,
    index: Option<i64>,
    #[serde(default)]
    view_offset: i64,
    #[serde(default)]
    view_count: i64,
    last_viewed_at: Option<i64>,
    #[serde(rename = "Guid", default)]
    guid: Vec<Id>,
    #[serde(rename = "Media", default)]
    media: Vec<Media>,
}

#[derive(Deserialize)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
struct Media {
    #[serde(rename = "Part", default)]
    part: Vec<Part>,
}

#[derive(Deserialize)]
struct Part {
    file: Option<String>,
}

impl Metadata {
    fn into_entry(self, kind: Kind) -> PlexEntry {
        let label = match kind {
            Kind::Movie => match self.year {
                Some(y) => format!("{} ({y})", self.title),
                None => self.title,
            },
            Kind::Episode => format!(
                "{} S{:02}E{:02}",
                self.grandparent_title.unwrap_or(self.title),
                self.parent_index.unwrap_or(0),
                self.index.unwrap_or(0)
            ),
        };
        PlexEntry {
            kind,
            label,
            files: self
                .media
                .into_iter()
                .flat_map(|m| m.part)
                .filter_map(|p| p.file)
                .collect(),
            guids: self.guid.into_iter().map(|g| g.id).collect(),
            view_offset_ms: self.view_offset,
            view_count: self.view_count,
            last_viewed_at: self.last_viewed_at,
        }
    }
}

/// Never logs or stores the token; it only travels in the `X-Plex-Token` header.
pub struct Plex {
    base: Url,
    token: String,
    client: reqwest::Client,
}

impl Plex {
    pub fn new(mut base: Url, token: String) -> Self {
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        Self {
            base,
            token,
            client: reqwest::Client::new(),
        }
    }

    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        let mut url = self.base.join(path)?;
        url.query_pairs_mut().extend_pairs(query);
        Ok(self
            .client
            .get(url)
            .header("X-Plex-Token", &self.token)
            .header("X-Plex-Client-Identifier", "reelhouse-import")
            .header("Accept", "application/json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Every movie and episode in the server's movie and show libraries.
    // ponytail: one request per library, no X-Plex-Container-Size paging; page if huge libraries time out.
    pub async fn entries(&self) -> Result<Vec<PlexEntry>> {
        let sections: Container<Sections> = self.get("library/sections", &[]).await?;
        let mut out = Vec::new();
        for s in sections.container.directory {
            let (kind, ty) = match s.kind.as_str() {
                "movie" => (Kind::Movie, "1"),
                "show" => (Kind::Episode, "4"),
                _ => continue,
            };
            let items: Container<Items> = self
                .get(
                    &format!("library/sections/{}/all", s.key),
                    &[("type", ty), ("includeGuids", "1")],
                )
                .await?;
            out.extend(
                items
                    .container
                    .metadata
                    .into_iter()
                    .map(|m| m.into_entry(kind)),
            );
        }
        Ok(out)
    }

    /// Length of `MediaContainer.{field}` at `path`, e.g. `livetv/dvrs` / `DVR`.
    pub async fn count(&self, path: &str, field: &str) -> Result<usize> {
        let v: serde_json::Value = self.get(path, &[]).await?;
        Ok(v["MediaContainer"][field].as_array().map_or(0, Vec::len))
    }
}
