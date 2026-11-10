use dioxus::prelude::*;
use reelhouse_api::NewUser;

use crate::Route;
use crate::session::{self, client};
use crate::widgets::describe;

#[component]
pub fn Login() -> Element {
    let mut name = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut server = use_signal(|| if session::needs_server() { "http://".to_string() } else { String::new() });
    let mut error = use_signal(|| None::<String>);
    let ask_server = session::needs_server();
    let nav = navigator();
    let submit = move |e: FormEvent| async move {
        e.prevent_default();
        if ask_server {
            session::set_server(&server());
        }
        match client().login(&name(), &password()).await {
            Ok(res) => {
                session::sign_in(res.token, res.user);
                nav.replace(Route::Home {});
            }
            Err(reelhouse_core::ClientError::Unauthorized) => {
                error.set(Some("That name and password don't match.".into()))
            }
            Err(e) => error.set(Some(describe(&e))),
        }
    };
    rsx! {
        div { class: "auth",
            h1 { class: "brand big", "Reelhouse" }
            form { class: "auth-form", onsubmit: submit,
                if ask_server {
                    label { "Server address, e.g. http://192.168.1.10:8420"
                        input { autofocus: true, r#type: "url", value: "{server}", oninput: move |e| server.set(e.value()) }
                    }
                }
                label { "Name"
                    input { autofocus: !ask_server, value: "{name}", oninput: move |e| name.set(e.value()) }
                }
                label { "Password"
                    input { r#type: "password", value: "{password}", oninput: move |e| password.set(e.value()) }
                }
                if let Some(msg) = error() {
                    p { class: "notice", "{msg}" }
                }
                button { class: "primary", r#type: "submit", "Sign in" }
            }
            p { class: "aside", "New server? " Link { to: Route::Setup {}, "Create the admin account" } }
        }
    }
}

#[component]
pub fn Setup() -> Element {
    let mut name = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let nav = navigator();
    let submit = move |e: FormEvent| async move {
        e.prevent_default();
        let c = client();
        let new = NewUser {
            name: name(),
            password: password(),
            is_admin: true,
        };
        let result = match c.setup(&new).await {
            Ok(_) => c.login(&new.name, &new.password).await,
            Err(e) => Err(e),
        };
        match result {
            Ok(res) => {
                session::sign_in(res.token, res.user);
                nav.replace(Route::Admin {});
            }
            Err(reelhouse_core::ClientError::Status(409, _)) => error.set(Some(
                "This server already has an admin. Sign in instead.".into(),
            )),
            Err(e) => error.set(Some(describe(&e))),
        }
    };
    rsx! {
        div { class: "auth",
            h1 { class: "brand big", "Set up Reelhouse" }
            p { class: "aside", "This account manages libraries and users." }
            form { class: "auth-form", onsubmit: submit,
                label { "Name"
                    input { autofocus: true, value: "{name}", oninput: move |e| name.set(e.value()) }
                }
                label { "Password (8 characters or more)"
                    input { r#type: "password", value: "{password}", oninput: move |e| password.set(e.value()) }
                }
                if let Some(msg) = error() {
                    p { class: "notice", "{msg}" }
                }
                button { class: "primary", r#type: "submit", "Create admin account" }
            }
        }
    }
}
