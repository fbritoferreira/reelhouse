use reelhouse_server::livetv::SourceChannel;
use reelhouse_server::livetv::xtream::Xtream;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const GUIDE: &str = r#"<tv>
  <channel id="news.uk"><display-name>News</display-name></channel>
  <programme start="20261110200000 +0000" stop="20261110210000 +0000" channel="news.uk">
    <title>Evening News</title>
  </programme>
</tv>"#;

/// Mounts an Xtream panel with one category and two live streams.
pub async fn mount_panel(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/player_api.php"))
        .and(query_param("username", "u"))
        .and(query_param("password", "p"))
        .and(query_param("action", "get_live_streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"num": 1, "name": "News HD", "stream_id": 101, "stream_icon": "http://logo/n.png",
             "epg_channel_id": "news.uk", "category_id": "7"},
            {"num": "2", "name": "Movies", "stream_id": "102", "stream_icon": "",
             "epg_channel_id": null, "category_id": "8"}
        ])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/player_api.php"))
        .and(query_param("action", "get_live_categories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"category_id": "7", "category_name": "UK News", "parent_id": 0}
        ])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/xmltv.php"))
        .and(query_param("username", "u"))
        .and(query_param("password", "p"))
        .respond_with(ResponseTemplate::new(200).set_body_string(GUIDE))
        .mount(server)
        .await;
}

#[tokio::test]
async fn lists_live_streams_as_channels() {
    let server = MockServer::start().await;
    mount_panel(&server).await;
    let xc = Xtream::new(&format!("{}/", server.uri()), "u", "p");
    let chans = xc.channels().await.unwrap();
    assert_eq!(
        chans,
        vec![
            SourceChannel {
                number: Some(1),
                name: "News HD".into(),
                logo_url: Some("http://logo/n.png".into()),
                stream_url: format!("{}/live/u/p/101.ts", server.uri()),
                epg_channel_id: Some("news.uk".into()),
                group_title: Some("UK News".into()),
            },
            SourceChannel {
                number: Some(2),
                name: "Movies".into(),
                logo_url: None,
                stream_url: format!("{}/live/u/p/102.ts", server.uri()),
                epg_channel_id: None,
                group_title: None,
            },
        ]
    );
}

#[tokio::test]
async fn guide_comes_from_xmltv_php() {
    let server = MockServer::start().await;
    mount_panel(&server).await;
    let xc = Xtream::new(&server.uri(), "u", "p");
    assert_eq!(
        xc.xmltv_url(),
        format!("{}/xmltv.php?username=u&password=p", server.uri())
    );
    let text = reelhouse_server::livetv::fetch_text(&xc.xmltv_url())
        .await
        .unwrap();
    let guide = reelhouse_server::livetv::xmltv::parse(&text).unwrap();
    assert_eq!(guide.programmes[0].title, "Evening News");
}

#[tokio::test]
async fn bad_credentials_are_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/player_api.php"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let xc = Xtream::new(&server.uri(), "u", "wrong");
    assert!(xc.channels().await.is_err());
}
