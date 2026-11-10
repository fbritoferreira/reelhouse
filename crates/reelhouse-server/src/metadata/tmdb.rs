//! TMDB lookups for new items.

use anyhow::Result;
use serde::Deserialize;
use url::Url;

const IMAGES: &str = "https://image.tmdb.org/t/p/";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub tmdb_id: Option<i64>,
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<i64>,
    pub title: Option<String>,
    pub year: Option<i64>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
}

pub struct Tmdb {
    base: Url,
    /// `None` when talking to the Reelhouse metadata proxy, which adds the key itself.
    key: Option<String>,
    images: Url,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct Search {
    results: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    id: i64,
    title: Option<String>,
    name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    overview: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
}

#[derive(Deserialize, Default)]
struct External {
    imdb_id: Option<String>,
    tvdb_id: Option<i64>,
}

#[derive(Deserialize)]
struct Episode {
    name: Option<String>,
    overview: Option<String>,
    still_path: Option<String>,
}

fn year(date: Option<&str>) -> Option<i64> {
    date?.get(..4)?.parse().ok()
}

impl Tmdb {
    pub fn new(base: Url, key: Option<String>) -> Self {
        Self {
            base,
            key,
            images: Url::parse(IMAGES).expect("valid TMDB image URL"),
            client: reqwest::Client::new(),
        }
    }

    /// Downloads artwork from `images` instead of TMDB's CDN (tests).
    pub fn with_image_base(mut self, images: Url) -> Self {
        self.images = images;
        self
    }

    fn image(&self, size: &str, path: Option<String>) -> Option<String> {
        Some(
            self.images
                .join(&format!("{size}{}", path?))
                .ok()?
                .to_string(),
        )
    }

    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Option<T>> {
        let mut url = self.base.join(path)?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(key) = &self.key {
                pairs.append_pair("api_key", key);
            }
            pairs.extend_pairs(query);
        }
        let res = self.client.get(url).send().await?;
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(res.error_for_status()?.json().await?))
    }

    async fn first(&self, kind: &str, query: &[(&str, &str)]) -> Result<Option<Meta>> {
        let Some(search) = self
            .get::<Search>(&format!("3/search/{kind}"), query)
            .await?
        else {
            return Ok(None);
        };
        let Some(hit) = search.results.into_iter().next() else {
            return Ok(None);
        };
        let ext = self
            .get::<External>(&format!("3/{kind}/{}/external_ids", hit.id), &[])
            .await?
            .unwrap_or_default();
        Ok(Some(Meta {
            tmdb_id: Some(hit.id),
            imdb_id: ext.imdb_id,
            tvdb_id: ext.tvdb_id,
            year: year(
                hit.release_date
                    .as_deref()
                    .or(hit.first_air_date.as_deref()),
            ),
            title: hit.title.or(hit.name),
            overview: hit.overview.filter(|o| !o.is_empty()),
            poster_url: self.image("w500", hit.poster_path),
            backdrop_url: self.image("w1280", hit.backdrop_path),
        }))
    }

    pub async fn movie(&self, title: &str, year: Option<u16>) -> Result<Option<Meta>> {
        let y = year.map(|y| y.to_string());
        let mut q = vec![("query", title)];
        if let Some(y) = &y {
            q.push(("year", y));
        }
        self.first("movie", &q).await
    }

    pub async fn show(&self, title: &str) -> Result<Option<Meta>> {
        self.first("tv", &[("query", title)]).await
    }

    pub async fn episode(&self, show_id: i64, season: u16, episode: u16) -> Result<Option<Meta>> {
        let path = format!("3/tv/{show_id}/season/{season}/episode/{episode}");
        Ok(self.get::<Episode>(&path, &[]).await?.map(|e| Meta {
            title: e.name,
            overview: e.overview.filter(|o| !o.is_empty()),
            backdrop_url: self.image("w1280", e.still_path),
            ..Default::default()
        }))
    }
}
