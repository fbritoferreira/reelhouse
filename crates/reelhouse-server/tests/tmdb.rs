use reelhouse_server::metadata::tmdb::Tmdb;
use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn tmdb() -> (MockServer, Tmdb) {
    let server = MockServer::start().await;
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    (server, Tmdb::new(base, "k".into()))
}

#[tokio::test]
async fn movie_match_by_title_and_year() {
    let (server, tmdb) = tmdb().await;
    Mock::given(method("GET"))
        .and(path("/3/search/movie"))
        .and(query_param("query", "Blade Runner"))
        .and(query_param("year", "1982"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": [{
            "id": 78, "title": "Blade Runner", "release_date": "1982-06-25",
            "overview": "A blade runner...", "poster_path": "/p.jpg", "backdrop_path": "/b.jpg"}]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/3/movie/78/external_ids"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"imdb_id": "tt0083658"})))
        .mount(&server)
        .await;
    let meta = tmdb
        .movie("Blade Runner", Some(1982))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.tmdb_id, Some(78));
    assert_eq!(meta.imdb_id.as_deref(), Some("tt0083658"));
    assert_eq!(meta.year, Some(1982));
    assert_eq!(
        meta.poster_url.as_deref(),
        Some("https://image.tmdb.org/t/p/w500/p.jpg")
    );
}

#[tokio::test]
async fn no_result_is_none() {
    let (server, tmdb) = tmdb().await;
    Mock::given(method("GET"))
        .and(path("/3/search/movie"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
        .mount(&server)
        .await;
    assert_eq!(tmdb.movie("Nothing Here", None).await.unwrap(), None);
}

#[tokio::test]
async fn episode_fills_overview() {
    let (server, tmdb) = tmdb().await;
    Mock::given(method("GET"))
        .and(path("/3/tv/136315/season/2/episode/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"name": "Forks", "overview": "Richie trains.", "still_path": "/s.jpg"}),
        ))
        .mount(&server)
        .await;
    let meta = tmdb.episode(136315, 2, 7).await.unwrap().unwrap();
    assert_eq!(meta.title.as_deref(), Some("Forks"));
    assert_eq!(meta.overview.as_deref(), Some("Richie trains."));
}
