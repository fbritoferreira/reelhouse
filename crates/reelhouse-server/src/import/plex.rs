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

use std::collections::{BTreeSet, HashMap};

use anyhow::Result;
use reelhouse_api::PathRewrite;
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

/// A Reelhouse movie or episode, as far as matching needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalItem {
    pub id: i64,
    pub kind: Kind,
    pub file_path: Option<String>,
    pub tmdb_id: Option<i64>,
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<i64>,
}

#[derive(Debug, PartialEq)]
pub struct Matches<'a> {
    /// Reelhouse item id and the Plex entry it matched.
    pub matched: Vec<(i64, &'a PlexEntry)>,
    pub unmatched: Vec<String>,
}

/// Applies the first rewrite whose `from` is a whole-component prefix of `path`.
pub fn rewrite(path: &str, rules: &[PathRewrite]) -> String {
    for r in rules {
        if let Some(rest) = path.strip_prefix(r.from.trim_end_matches('/'))
            && (rest.is_empty() || rest.starts_with('/'))
        {
            return format!("{}{rest}", r.to.trim_end_matches('/'));
        }
    }
    path.to_owned()
}

fn id_matches(guid: &str, item: &LocalItem) -> bool {
    match guid.split_once("://") {
        Some(("tmdb", v)) => item.tmdb_id.is_some_and(|id| v.parse() == Ok(id)),
        Some(("tvdb", v)) => item.tvdb_id.is_some_and(|id| v.parse() == Ok(id)),
        Some(("imdb", v)) => item.imdb_id.as_deref() == Some(v),
        _ => false,
    }
}

/// Matches by rewritten file path, then by TMDB/IMDb/TVDB id within the same kind.
/// No match, or ids shared by several items, lands in `unmatched`: never guess.
pub fn match_entries<'a>(
    entries: &'a [PlexEntry],
    rules: &[PathRewrite],
    items: &[LocalItem],
) -> Matches<'a> {
    let by_path: HashMap<&str, i64> = items
        .iter()
        .filter_map(|i| Some((i.file_path.as_deref()?, i.id)))
        .collect();
    let mut out = Matches {
        matched: Vec::new(),
        unmatched: Vec::new(),
    };
    for e in entries {
        let files: Vec<String> = e.files.iter().map(|f| rewrite(f, rules)).collect();
        if let Some(id) = files.iter().find_map(|f| by_path.get(f.as_str())) {
            out.matched.push((*id, e));
            continue;
        }
        let ids: BTreeSet<i64> = items
            .iter()
            .filter(|i| i.kind == e.kind && e.guids.iter().any(|g| id_matches(g, i)))
            .map(|i| i.id)
            .collect();
        match ids.len() {
            1 => out.matched.extend(ids.first().map(|id| (*id, e))),
            0 => out.unmatched.push(format!(
                "{}: no item with path {} or ids [{}]",
                e.label,
                files.first().map_or("(none)", String::as_str),
                e.guids.join(", ")
            )),
            n => out
                .unmatched
                .push(format!("{}: ids match {n} items, skipped", e.label)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(from: &str, to: &str) -> PathRewrite {
        PathRewrite {
            from: from.into(),
            to: to.into(),
        }
    }

    fn entry(kind: Kind, file: &str, guids: &[&str]) -> PlexEntry {
        PlexEntry {
            kind,
            label: "X".into(),
            files: vec![file.into()],
            guids: guids.iter().map(|g| (*g).into()).collect(),
            view_offset_ms: 1000,
            view_count: 0,
            last_viewed_at: None,
        }
    }

    fn item(id: i64, kind: Kind, file: &str) -> LocalItem {
        LocalItem {
            id,
            kind,
            file_path: Some(file.into()),
            tmdb_id: None,
            imdb_id: None,
            tvdb_id: None,
        }
    }

    #[test]
    fn rewrite_respects_component_boundaries() {
        let rules = [rule("/data/", "/media"), rule("/", "/root")];
        assert_eq!(rewrite("/data/a.mkv", &rules), "/media/a.mkv");
        assert_eq!(rewrite("/database/a.mkv", &rules), "/root/database/a.mkv");
        assert_eq!(rewrite("/data/a.mkv", &[]), "/data/a.mkv");
        assert_eq!(rewrite("rel/a.mkv", &[rule("/data", "/m")]), "rel/a.mkv");
    }

    #[test]
    fn matches_by_rewritten_path_first() {
        let entries = [entry(Kind::Movie, "/data/Heat.mkv", &["tmdb://949"])];
        let mut by_id = item(2, Kind::Movie, "/other/Heat.mkv");
        by_id.tmdb_id = Some(949);
        let items = [item(1, Kind::Movie, "/media/Heat.mkv"), by_id];
        let m = match_entries(&entries, &[rule("/data", "/media")], &items);
        assert_eq!(m.matched, vec![(1, &entries[0])]);
        assert!(m.unmatched.is_empty());
    }

    #[test]
    fn falls_back_to_each_id_kind() {
        let entries = [
            entry(Kind::Movie, "/a", &["plex://movie/x", "tmdb://949"]),
            entry(Kind::Movie, "/b", &["imdb://tt0083658"]),
            entry(Kind::Episode, "/c", &["tvdb://42"]),
        ];
        let mut a = item(1, Kind::Movie, "/x1");
        a.tmdb_id = Some(949);
        let mut b = item(2, Kind::Movie, "/x2");
        b.imdb_id = Some("tt0083658".into());
        let mut c = item(3, Kind::Episode, "/x3");
        c.tvdb_id = Some(42);
        let m = match_entries(&entries, &[], &[a, b, c]);
        let ids: Vec<i64> = m.matched.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn ids_only_match_the_same_kind() {
        let entries = [entry(Kind::Episode, "/a", &["tvdb://42"])];
        let mut show_level = item(1, Kind::Movie, "/x");
        show_level.tvdb_id = Some(42);
        let m = match_entries(&entries, &[], &[show_level]);
        assert!(m.matched.is_empty());
        assert_eq!(m.unmatched.len(), 1);
        assert!(m.unmatched[0].contains("/a"));
    }

    #[test]
    fn ambiguous_ids_are_not_guessed() {
        let entries = [entry(Kind::Movie, "/a", &["tmdb://949"])];
        let mut one = item(1, Kind::Movie, "/x1");
        one.tmdb_id = Some(949);
        let mut two = item(2, Kind::Movie, "/x2");
        two.tmdb_id = Some(949);
        let m = match_entries(&entries, &[], &[one, two]);
        assert!(m.matched.is_empty());
        assert!(m.unmatched[0].contains("2 items"));
    }
}
