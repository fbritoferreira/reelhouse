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

// ponytail: native builds keep the session in memory only; persist to a file when the TV app needs it.
#[cfg(not(target_arch = "wasm32"))]
fn load(_key: &str) -> Option<String> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn save(_key: &str, _value: Option<&str>) {}

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
