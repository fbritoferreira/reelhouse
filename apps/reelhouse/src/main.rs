//! The Reelhouse app for web, Android and Google TV.

#[cfg(not(target_arch = "wasm32"))]
mod art;
mod download_ui;
#[cfg(not(target_arch = "wasm32"))]
mod downloads;
mod live_cache;
mod pages;
mod player;
mod session;
mod tv;
mod widgets;

use dioxus::prelude::*;
use pages::{
    Admin, Downloads, Home, ItemPage, LibraryPage, LinkDevice, LiveTv, Login, Play, Search, Setup,
    Watch,
};

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
        #[route("/downloads")]
        Downloads {},
        #[route("/live")]
        LiveTv {},
        #[route("/search")]
        Search {},
    #[end_layout]
    #[route("/login")]
    Login {},
    #[route("/setup")]
    Setup {},
    #[route("/play/:id")]
    Play { id: i64 },
    #[route("/watch/:id?:group")]
    Watch { id: i64, group: String },
}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    session::provide();
    #[cfg(feature = "mobile")]
    art::use_art_handler();
    tv::use_tv_layout();
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

/// The round profile button and its menu: who is signed in, and the browser-only tasks.
#[component]
fn Profile(name: String, is_admin: bool) -> Element {
    let mut open = use_signal(|| false);
    let nav = navigator();
    let initial = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    rsx! {
        div { class: "profile",
            button {
                class: if open() { "bar-round avatar-button current" } else { "bar-round avatar-button" },
                "aria-haspopup": "menu",
                "aria-expanded": open(),
                "aria-label": "{name}",
                title: "{name}",
                onclick: move |_| open.toggle(),
                "{initial}"
            }
            if open() {
                // Clicking anywhere else closes the menu.
                div { class: "menu-scrim", onclick: move |_| open.set(false) }
                div { class: "profile-menu", role: "menu",
                    p { class: "profile-name", "{name}" }
                    // Linking devices and managing the server are browser tasks; the TV app keeps to watching.
                    if cfg!(target_arch = "wasm32") {
                        Link { role: "menuitem", to: Route::LinkDevice { code: String::new() }, onclick: move |_| open.set(false), "Link a device" }
                        if is_admin {
                            Link { role: "menuitem", to: Route::Admin {}, onclick: move |_| open.set(false), "Manage" }
                        }
                    }
                    button {
                        role: "menuitem",
                        onclick: move |_| {
                            session::sign_out();
                            nav.replace(Route::Login {});
                        },
                        "Switch user"
                    }
                }
            }
        }
    }
}

/// Keyed by route, so a page whose id changes (one library to another) mounts afresh; otherwise
/// the router reuses it and its hooks keep loading the old id.
#[component]
fn CurrentPage() -> Element {
    rsx! { Outlet::<Route> {} }
}

/// Signed-in chrome: top bar and the page.
#[component]
fn Shell() -> Element {
    let mut session = session::use_session();
    let nav = navigator();
    let route = use_route::<Route>();
    // Back walks back through screens; on Home it leaves the app.
    // Read the route at press time (hooks cannot run inside callbacks).
    let back_router = router();
    tv::use_back(move || {
        if matches!(back_router.current::<Route>(), Route::Home {}) {
            tv::exit_app();
        } else {
            back_router.go_back();
        }
    });
    use_effect(use_reactive!(|route| {
        let _ = &route;
        tv::focus_main_action();
    }));
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
    #[cfg(not(target_arch = "wasm32"))]
    use_hook(|| {
        // Not session::client(): that reads context, a hook, which cannot run inside this one.
        let client = session.peek().client.clone();
        if client.token().is_some() {
            downloads::resume_unfinished(client);
        }
    });
    let user = session.read().user.clone();
    let libraries = use_resource(|| async move { session::client().libraries().await });
    // Hooks above run on every render; only then leave for sign-in.
    if session.read().client.token().is_none() {
        nav.replace(Route::Login {});
        return rsx! {};
    }
    let here = |r: Route| {
        if route == r { "pill current" } else { "pill" }
    };
    rsx! {
        header { class: "bar",
            Link { class: "brand", to: Route::Home {}, "Reelhouse" }
            Link {
                class: if route == (Route::Search {}) { "bar-round current" } else { "bar-round" },
                to: Route::Search {},
                "aria-label": "Search",
                title: "Search",
                svg { view_box: "0 0 24 24", "aria-hidden": "true",
                    circle { cx: "10.5", cy: "10.5", r: "6.5" }
                    path { d: "M15.5 15.5 20 20" }
                }
            }
            nav { class: "bar-pills", "aria-label": "Main",
                Link { class: here(Route::Home {}), to: Route::Home {}, "Home" }
                if let Some(Ok(libs)) = &*libraries.read() {
                    for lib in libs.clone() {
                        Link { key: "{lib.id}", class: here(Route::LibraryPage { id: lib.id }), to: Route::LibraryPage { id: lib.id }, "{lib.name}" }
                    }
                }
                Link { class: here(Route::LiveTv {}), to: Route::LiveTv {}, "Live TV" }
                if cfg!(not(target_arch = "wasm32")) {
                    Link { class: here(Route::Downloads {}), to: Route::Downloads {}, "Downloads" }
                }
            }
            if let Some(u) = user.clone() {
                Profile { name: u.name.clone(), is_admin: u.is_admin }
            }
        }
        main {
            // A one-item keyed list: keys only remount among list siblings.
            for r in [route.to_string()] {
                CurrentPage { key: "{r}" }
            }
        }
    }
}
