//! Fixtures follow the `MediaContainer` shapes in the community Plex OpenAPI spec
//! (github.com/LukeHagar/plex-api-spec, `plex-api-spec.yaml`).

use reelhouse_server::import::plex::{Kind, Plex, PlexEntry};
use serde_json::{Value, json};
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const TOKEN: &str = "plex-secret";

pub fn sections() -> Value {
    json!({"MediaContainer": {"size": 3, "Directory": [
        {"key": "1", "type": "movie", "title": "Movies"},
        {"key": "2", "type": "show", "title": "TV Shows"},
        {"key": "3", "type": "artist", "title": "Music"}
    ]}})
}

pub fn movies() -> Value {
    json!({"MediaContainer": {"size": 2, "Metadata": [
        {"ratingKey": "10", "type": "movie", "title": "Heat", "year": 1995,
         "viewOffset": 4_500_000, "lastViewedAt": 1_700_000_000,
         "Guid": [{"id": "imdb://tt0113277"}, {"id": "tmdb://949"}],
         "Media": [{"id": 1, "Part": [{"id": 1, "file": "/data/movies/Heat (1995)/Heat.mkv"}]}]},
        {"ratingKey": "11", "type": "movie", "title": "Unwatched", "year": 2001,
         "Media": [{"id": 2, "Part": [{"id": 2, "file": "/data/movies/Unwatched.mkv"}]}]}
    ]}})
}

pub fn episodes() -> Value {
    json!({"MediaContainer": {"size": 1, "Metadata": [
        {"ratingKey": "150", "type": "episode", "title": "The Illusion of Truth",
         "grandparentTitle": "Babylon 5", "parentIndex": 4, "index": 8, "viewCount": 1,
         "Media": [{"id": 376, "Part": [{"id": 872,
            "file": "/data/tv/Babylon 5/Season 04/Babylon 5 S04E08.mkv"}]}]}
    ]}})
}

/// Mounts sections plus one movie and one TV library, each requiring the token and JSON.
pub async fn mount_library(server: &MockServer) {
    for (p, body, ty) in [
        ("/library/sections", sections(), None),
        ("/library/sections/1/all", movies(), Some("1")),
        ("/library/sections/2/all", episodes(), Some("4")),
    ] {
        let mut mock = Mock::given(method("GET"))
            .and(path(p))
            .and(header("X-Plex-Token", TOKEN))
            .and(header("Accept", "application/json"));
        if let Some(ty) = ty {
            mock = mock
                .and(query_param("type", ty))
                .and(query_param("includeGuids", "1"));
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }
}

fn plex(server: &MockServer) -> Plex {
    Plex::new(Url::parse(&server.uri()).unwrap(), TOKEN.into())
}

#[tokio::test]
async fn reads_movies_and_episodes() {
    let server = MockServer::start().await;
    mount_library(&server).await;
    let entries = plex(&server).entries().await.unwrap();
    assert_eq!(
        entries,
        vec![
            PlexEntry {
                kind: Kind::Movie,
                label: "Heat (1995)".into(),
                files: vec!["/data/movies/Heat (1995)/Heat.mkv".into()],
                guids: vec!["imdb://tt0113277".into(), "tmdb://949".into()],
                view_offset_ms: 4_500_000,
                view_count: 0,
                last_viewed_at: Some(1_700_000_000),
            },
            PlexEntry {
                kind: Kind::Movie,
                label: "Unwatched (2001)".into(),
                files: vec!["/data/movies/Unwatched.mkv".into()],
                guids: vec![],
                view_offset_ms: 0,
                view_count: 0,
                last_viewed_at: None,
            },
            PlexEntry {
                kind: Kind::Episode,
                label: "Babylon 5 S04E08".into(),
                files: vec!["/data/tv/Babylon 5/Season 04/Babylon 5 S04E08.mkv".into()],
                guids: vec![],
                view_offset_ms: 0,
                view_count: 1,
                last_viewed_at: None,
            },
        ]
    );
}

#[tokio::test]
async fn bad_token_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let err = plex(&server).entries().await.unwrap_err();
    assert!(!format!("{err:#}").contains(TOKEN));
}

#[tokio::test]
async fn counts_live_tv_objects() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/livetv/dvrs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"MediaContainer": {"size": 1, "DVR": [{"key": "1", "uuid": "abc"}]}}),
        ))
        .mount(&server)
        .await;
    assert_eq!(plex(&server).count("livetv/dvrs", "DVR").await.unwrap(), 1);
    Mock::given(method("GET"))
        .and(path("/media/subscriptions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"MediaContainer": {"size": 0}})),
        )
        .mount(&server)
        .await;
    assert_eq!(
        plex(&server)
            .count("media/subscriptions", "MediaSubscription")
            .await
            .unwrap(),
        0
    );
}
