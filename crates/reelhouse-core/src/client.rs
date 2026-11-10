//! Typed client for the Reelhouse server API.

use reelhouse_api::*;
use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("not signed in or session expired")]
    Unauthorized,
    #[error("server returned {0}: {1}")]
    Status(u16, String),
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
}

#[derive(Clone, Debug)]
pub struct Client {
    base: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl Client {
    /// `base` is the server origin, e.g. `http://192.168.1.10:8420`; empty for same-origin web.
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            token: None,
            http: reqwest::Client::new(),
        }
    }

    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token = token;
        self
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// Absolute URL for a server path, with the token as a query parameter for media players.
    pub fn media_url(&self, path: &str) -> String {
        let sep = if path.contains('?') { '&' } else { '?' };
        match &self.token {
            Some(t) => format!("{}{path}{sep}token={t}", self.base),
            None => format!("{}{path}", self.base),
        }
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let r = self.http.request(method, format!("{}{path}", self.base));
        match &self.token {
            Some(t) => r.bearer_auth(t),
            None => r,
        }
    }

    async fn send<T: DeserializeOwned>(
        &self,
        r: reqwest::RequestBuilder,
    ) -> Result<T, ClientError> {
        let res = r.send().await?;
        match res.status().as_u16() {
            401 => Err(ClientError::Unauthorized),
            s if s >= 400 => Err(ClientError::Status(s, res.text().await.unwrap_or_default())),
            _ => Ok(res.json().await?),
        }
    }

    async fn send_empty(&self, r: reqwest::RequestBuilder) -> Result<(), ClientError> {
        let res = r.send().await?;
        match res.status().as_u16() {
            401 => Err(ClientError::Unauthorized),
            s if s >= 400 => Err(ClientError::Status(s, res.text().await.unwrap_or_default())),
            _ => Ok(()),
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        self.send(self.req(reqwest::Method::GET, path)).await
    }

    async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        self.send(self.req(reqwest::Method::POST, path).json(body))
            .await
    }

    pub async fn server_info(&self) -> Result<ServerInfo, ClientError> {
        self.get("/api/v1/server").await
    }

    pub async fn setup(&self, user: &NewUser) -> Result<User, ClientError> {
        self.post("/api/v1/setup", user).await
    }

    pub async fn login(&self, name: &str, password: &str) -> Result<LoginResponse, ClientError> {
        self.post(
            "/api/v1/auth/login",
            &LoginRequest {
                name: name.into(),
                password: password.into(),
            },
        )
        .await
    }

    pub async fn me(&self) -> Result<User, ClientError> {
        self.get("/api/v1/me").await
    }

    pub async fn users(&self) -> Result<Vec<User>, ClientError> {
        self.get("/api/v1/users").await
    }

    pub async fn create_user(&self, user: &NewUser) -> Result<User, ClientError> {
        self.post("/api/v1/users", user).await
    }

    pub async fn libraries(&self) -> Result<Vec<Library>, ClientError> {
        self.get("/api/v1/libraries").await
    }

    pub async fn create_library(&self, lib: &NewLibrary) -> Result<Library, ClientError> {
        self.post("/api/v1/libraries", lib).await
    }

    pub async fn scan(&self, library: i64) -> Result<ScanReport, ClientError> {
        self.post(&format!("/api/v1/libraries/{library}/scan"), &())
            .await
    }

    pub async fn library_items(&self, library: i64) -> Result<Vec<Item>, ClientError> {
        self.get(&format!("/api/v1/libraries/{library}/items"))
            .await
    }

    pub async fn item(&self, id: i64) -> Result<Item, ClientError> {
        self.get(&format!("/api/v1/items/{id}")).await
    }

    pub async fn children(&self, id: i64) -> Result<Vec<Item>, ClientError> {
        self.get(&format!("/api/v1/items/{id}/children")).await
    }

    pub async fn delete_library(&self, id: i64) -> Result<(), ClientError> {
        self.send_empty(self.req(reqwest::Method::DELETE, &format!("/api/v1/libraries/{id}")))
            .await
    }

    pub async fn delete_tv_source(&self, id: i64) -> Result<(), ClientError> {
        self.send_empty(self.req(
            reqwest::Method::DELETE,
            &format!("/api/v1/livetv/sources/{id}"),
        ))
        .await
    }

    /// Every group of a source, with whether Live TV shows it.
    pub async fn source_groups(&self, id: i64) -> Result<Vec<SourceGroup>, ClientError> {
        self.get(&format!("/api/v1/livetv/sources/{id}/groups"))
            .await
    }

    pub async fn hide_groups(&self, id: i64, hidden: &HiddenGroups) -> Result<(), ClientError> {
        self.send_empty(
            self.req(
                reqwest::Method::PUT,
                &format!("/api/v1/livetv/sources/{id}/groups"),
            )
            .json(hidden),
        )
        .await
    }

    pub async fn tv_sources(&self) -> Result<Vec<TvSource>, ClientError> {
        self.get("/api/v1/livetv/sources").await
    }

    pub async fn create_tv_source(&self, source: &NewTvSource) -> Result<TvSource, ClientError> {
        self.post("/api/v1/livetv/sources", source).await
    }

    pub async fn refresh_tv_source(&self, id: i64) -> Result<RefreshReport, ClientError> {
        self.post(&format!("/api/v1/livetv/sources/{id}/refresh"), &())
            .await
    }

    pub async fn channel_groups(&self) -> Result<Vec<ChannelGroup>, ClientError> {
        self.get("/api/v1/livetv/groups").await
    }

    /// One group's channels with what is on now and next; `""` asks for the ungrouped.
    pub async fn guide(&self, group: &str) -> Result<Vec<GuideChannel>, ClientError> {
        self.send(
            self.req(reqwest::Method::GET, "/api/v1/livetv/guide")
                .query(&[("group", group), ("per_channel", "2")]),
        )
        .await
    }

    /// Channels whose name contains `q`, at most 50.
    pub async fn search_channels(&self, q: &str) -> Result<Vec<Channel>, ClientError> {
        self.send(
            self.req(reqwest::Method::GET, "/api/v1/livetv/channels")
                .query(&[("q", q)]),
        )
        .await
    }

    pub async fn play_channel(&self, id: i64) -> Result<PlaybackPlan, ClientError> {
        self.post(&format!("/api/v1/livetv/channels/{id}/play"), &())
            .await
    }

    pub async fn continue_watching(&self) -> Result<Vec<Item>, ClientError> {
        self.get("/api/v1/continue").await
    }

    /// How to play `id`; an HLS conversion begins `start_s` seconds in.
    pub async fn playback(
        &self,
        id: i64,
        caps: &Capabilities,
        start_s: f64,
    ) -> Result<PlaybackPlan, ClientError> {
        self.post(
            &format!("/api/v1/items/{id}/playback?start={start_s:.3}"),
            caps,
        )
        .await
    }

    /// Starts sign-in by code for this device.
    pub async fn device_start(&self) -> Result<DeviceCode, ClientError> {
        self.post("/api/v1/auth/device", &()).await
    }

    /// Checks whether the code was approved. `Ok(None)` while still waiting.
    pub async fn device_poll(
        &self,
        poll_token: &str,
    ) -> Result<Option<LoginResponse>, ClientError> {
        let res = self
            .req(reqwest::Method::POST, "/api/v1/auth/device/poll")
            .json(&DevicePoll {
                poll_token: poll_token.into(),
            })
            .send()
            .await?;
        match res.status().as_u16() {
            200 => Ok(Some(res.json().await?)),
            202 => Ok(None),
            s => Err(ClientError::Status(s, res.text().await.unwrap_or_default())),
        }
    }

    /// Approves a code shown on another device, signing it in as the current user.
    pub async fn device_approve(&self, code: &str) -> Result<(), ClientError> {
        self.send_empty(
            self.req(reqwest::Method::POST, "/api/v1/auth/device/approve")
                .json(&DeviceApprove { code: code.into() }),
        )
        .await
    }

    pub async fn put_progress(&self, id: i64, progress: &Progress) -> Result<(), ClientError> {
        self.send_empty(
            self.req(
                reqwest::Method::PUT,
                &format!("/api/v1/items/{id}/progress"),
            )
            .json(progress),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn media_url_appends_token() {
        let c = Client::new("http://s:8420/").with_token(Some("t".into()));
        assert_eq!(
            c.media_url("/api/v1/items/1/file"),
            "http://s:8420/api/v1/items/1/file?token=t"
        );
        assert_eq!(c.media_url("/x?a=1"), "http://s:8420/x?a=1&token=t");
    }

    #[tokio::test]
    async fn sends_bearer_and_maps_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/me"))
            .and(header("authorization", "Bearer good"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"id": 1, "name": "a", "is_admin": true})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/me"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let ok = Client::new(server.uri())
            .with_token(Some("good".into()))
            .me()
            .await
            .unwrap();
        assert_eq!(ok.name, "a");
        let err = Client::new(server.uri())
            .with_token(Some("bad".into()))
            .me()
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Unauthorized));
    }
}
