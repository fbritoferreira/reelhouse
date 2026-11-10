//! The Reelhouse app for web, Android and Google TV.

mod pages;
mod player;
mod session;
mod widgets;

use dioxus::prelude::*;
use pages::{Admin, Home, ItemPage, LibraryPage, LinkDevice, Login, Play, Setup};

const STYLE: Asset = asset!("/assets/main.css");

#[derive(Routable, Clone, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
        #[route("/")]
        Home {},
        #[route("/library/:id")]
        LibraryPage { id: i64 },
        #[route("/item/:id")]
        ItemPage { id: i64 },
        #[route("/admin")]
        Admin {},
        #[route("/link?:code")]
        LinkDevice { code: String },
    #[end_layout]
    #[route("/login")]
    Login {},
    #[route("/setup")]
    Setup {},
    #[route("/play/:id")]
    Play { id: i64 },
}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    session::provide();
    rsx! {
        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link {
            rel: "stylesheet",
            href: "https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:opsz,wght@12..96,500;12..96,700&family=Instrument+Sans:wght@400;500;600&display=swap",
        }
        document::Stylesheet { href: STYLE }
        Router::<Route> {}
    }
}

/// Signed-in chrome: top bar and the page.
#[component]
fn Shell() -> Element {
    let mut session = session::use_session();
    let nav = navigator();
    if session.read().client.token().is_none() {
        nav.replace(Route::Login {});
        return rsx! {};
    }
    // After a reload only the token survives; fetch who it belongs to, or sign out if it expired.
    use_future(move || async move {
        if session.peek().user.is_none() {
            match session::client().me().await {
                Ok(user) => session.write().user = Some(user),
                Err(reelhouse_core::ClientError::Unauthorized) => {
                    session::sign_out();
                    navigator().replace(Route::Login {});
                }
                Err(_) => {}
            }
        }
    });
    let user = session.read().user.clone();
    let route = use_route::<Route>();
    let here = |r: Route| {
        if route == r {
            "nav-link current"
        } else {
            "nav-link"
        }
    };
    rsx! {
        header { class: "bar",
            Link { class: "brand", to: Route::Home {}, "Reelhouse" }
            nav { class: "bar-links", "aria-label": "Main",
                Link { class: here(Route::Home {}), to: Route::Home {}, "Home" }
                Link { class: here(Route::LinkDevice { code: String::new() }), to: Route::LinkDevice { code: String::new() }, "Link a device" }
                if user.as_ref().is_some_and(|u| u.is_admin) {
                    Link { class: here(Route::Admin {}), to: Route::Admin {}, "Manage" }
                }
            }
            if let Some(u) = user.clone() {
                button {
                    class: "user-chip",
                    title: "Switch user",
                    onclick: move |_| {
                        session::sign_out();
                        nav.replace(Route::Login {});
                    },
                    span { class: "avatar", "{u.name.chars().next().unwrap_or('?').to_uppercase()}" }
                    span { class: "user-chip-text", "Switch user" }
                }
            }
        }
        main { Outlet::<Route> {} }
    }
}
