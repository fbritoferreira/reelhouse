//! The signed-in client, kept in a context signal and in browser storage on web.

use dioxus::prelude::*;
use reelhouse_api::User;
use reelhouse_core::Client;

#[derive(Clone)]
pub struct Session {
    pub client: Client,
    pub user: Option<User>,
}

const TOKEN_KEY: &str = "reelhouse.token";
const SERVER_KEY: &str = "reelhouse.server";

#[cfg(target_arch = "wasm32")]
fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
fn load(key: &str) -> Option<String> {
    storage()?.get_item(key).ok()?
}

#[cfg(target_arch = "wasm32")]
fn save(key: &str, value: Option<&str>) {
    if let Some(s) = storage() {
        let _ = match value {
            Some(v) => s.set_item(key, v),
            None => s.remove_item(key),
        };
    }
}

/// Native apps keep one small file per key in the app's private directory.
#[cfg(not(target_arch = "wasm32"))]
fn key_path(key: &str) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "android")]
    let dir = crate::player::files_dir()?;
    #[cfg(not(target_os = "android"))]
    let dir = std::env::var("HOME").ok()?;
    Some(std::path::Path::new(&dir).join(format!(".{key}")))
}

#[cfg(not(target_arch = "wasm32"))]
fn load(key: &str) -> Option<String> {
    std::fs::read_to_string(key_path(key)?)
        .ok()
        .filter(|s| !s.is_empty())
}

#[cfg(not(target_arch = "wasm32"))]
fn save(key: &str, value: Option<&str>) {
    if let Some(path) = key_path(key) {
        let _ = match value {
            Some(v) => std::fs::write(path, v),
            None => std::fs::remove_file(path),
        };
    }
}

/// Native apps have no page origin; the user types the server address once.
pub fn set_server(base: &str) {
    let base = base.trim().trim_end_matches('/').to_string();
    save(SERVER_KEY, Some(&base));
    let mut s = use_session();
    let token = s.peek().client.token().map(str::to_owned);
    s.set(Session {
        client: Client::new(base).with_token(token),
        user: None,
    });
}

pub fn needs_server() -> bool {
    cfg!(not(target_arch = "wasm32")) && load(SERVER_KEY).is_none()
}

/// Web talks to the server that served it; native apps use the saved server address.
fn server_base() -> String {
    load(SERVER_KEY).unwrap_or_else(origin)
}

#[cfg(target_arch = "wasm32")]
fn origin() -> String {
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn origin() -> String {
    String::new()
}

pub fn provide() {
    use_context_provider(|| {
        Signal::new(Session {
            client: Client::new(server_base()).with_token(load(TOKEN_KEY)),
            user: None,
        })
    });
}

pub fn use_session() -> Signal<Session> {
    use_context::<Signal<Session>>()
}

pub fn sign_in(token: String, user: User) {
    save(TOKEN_KEY, Some(&token));
    let mut s = use_session();
    let client = Client::new(server_base()).with_token(Some(token));
    s.set(Session {
        client,
        user: Some(user),
    });
}

pub fn sign_out() {
    save(TOKEN_KEY, None);
    let mut s = use_session();
    s.set(Session {
        client: Client::new(server_base()),
        user: None,
    });
}

pub fn client() -> Client {
    use_session().read().client.clone()
}
